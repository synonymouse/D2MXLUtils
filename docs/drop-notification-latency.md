# Drop Notification Latency — Investigation & Fix

## Symptom

Item-drop notifications in the overlay appeared several seconds after an
item landed on the ground, while the in-game MXL label (tooltip) for the
same item showed up promptly. The gap was strongly correlated with the
number of items already on the ground — a single drop in an empty town
worked fine; dropping next to a pile of items lagged heavily.

Regression window: reproducible after the `1.10.0` tag. A `1.10.0` release
binary behaved acceptably; a `pnpm tauri dev` build off the same branch
exhibited the most severe lag.

## How notifications and labels diverge

Both signals are produced by the same scanner tick, but they leave the
scanner at different points:

| Signal | Mechanism | Latency driver |
|---|---|---|
| In-game label on ground | `g_inspected_mask` bit flipped from the **inside** of the `pPaths` loop in `tick()`. MXL's hooked filter polls this bit every frame. | Time to reach the item in the current loop iteration. |
| Overlay notification | Events are pushed into `Vec<ItemDropEvent>` inside the loop, returned from `tick()`, and only then `emit()`-ed by `main.rs`. | **Full** duration of `tick()`. |

So any work `tick()` does after the item is scanned — including the
`run_map_marker_pass()` BFS — delays the notification but not the
tooltip. A slow tick widens the gap proportionally.

## Root causes

### 1. `run_map_marker_pass` runs unconditionally every tick (pre-existing)

Every tick calls `map_marker::bfs_item_positions(ctx, 10)` — a depth-10
BFS over the room graph — followed by `filter.decide()` for each item
found. This is `O(rooms × units)` `ReadProcessMemory` syscalls plus
`O(items × rules)` rule evaluations per tick, **regardless of whether
anything changed or whether any `map:true` rules exist in the filter**.

Measured in dev build on a populated area: 260-2500 ms per tick, steady
state. In release: 700-800 ms. This fixed cost was present on
`v1.10.0`, it just was not noticeable in release because sub-second
tick cadence still felt instant for drops.

### 2. `b3235ec` amplified the symptom

The commit added `seen_items.retain(|id| current_item_ids.contains(id))`
at the end of every tick. Correct for its stated purpose (re-notify an
item that was picked up and dropped again — `dwUnitId` is stable across
container transitions). But it meant any item that was legitimately
missed by a single iteration of `pPaths` — e.g., because a
`read_memory` on a sibling unit failed and the inner loop hit `break`,
or because a room briefly dropped out of `pRoomsNear` — was evicted
from `seen_items` and then re-scanned on the following tick.

`scan_unit` is cheap for already-seen items and expensive for new ones
(two `CreateRemoteThread` round-trips for `GetItemName` / `GetItemStats`,
plus string parsing). Flapping ids turned what should have been zero
work per tick into `2 × k` remote-thread calls per tick, where `k` was
the number of flapping items. On a crowded map, `k` could be dozens.

### 3. `CreateRemoteThread` is not the bottleneck (hypothesis that turned out wrong)

Initial suspicion was that remote-thread latency dominated. Direct
measurement refuted this: 29 `get_item_name` + 29 `get_item_stats`
calls totalled **26 ms** in a 12.7 s dev tick. The rest (≈12.8 s) was
split between `pPaths` iteration / `scan_unit` CPU work (6.2 s) and
`run_map_marker_pass` (6.6 s). Per remote-thread call is sub-1 ms, not
the ≈200 ms guess-timate the earlier analysis produced by dividing
gross tick time by remote-thread count.

## What shipped in this change

### A. Revert included, then restored

`b3235ec` was reverted temporarily to confirm it was contributing. It
was confirmed, then restored — the original motivation (re-notify on
re-drop) is still valid UX. The real fix is the split below; the
flaky-iteration amplification is now absorbed into a tick budget that
does not block notifications.

### B. Split `tick()` → `tick_items()` + `tick_map_markers()`

`tick_items()` does just the `pPaths` pass and returns fresh
`ItemDropEvent`s. `tick_map_markers()` runs the BFS + marker
reconciliation. `main.rs` now does:

```
let events = scanner.tick_items();
for event in events {
    app_handle.emit("item-drop", &event);
}
scanner.tick_map_markers();
```

Notifications no longer wait on the marker pass. Measured impact:

| Scenario | Before | After |
|---|---|---|
| Dev, idle in crowded area | notification waits ~7 s | ~260-500 ms |
| Dev, entering populated map (many scans) | ~12 s | ~1.3 s to emit, BFS trails by ~2-3 s |
| Release, same scenarios | ~800 ms - 1.6 s | ~100-800 ms |

This is a strict improvement — no change to marker behavior, just a
reordering that lets events leave the scanner thread earlier.

## Phase 2 — marker pass moved to its own thread

The split in §B above kept `tick_items` and `tick_map_markers` in the
same thread, so the *next* `tick_items` still waited on the previous
`tick_map_markers`. On a crowded map this capped tick-cadence at the
BFS duration (~700 ms release).

The marker pass now runs in a dedicated OS thread. `tick_items` runs at
its natural cadence (~30 ms idle, dominated by `pPaths` walk under
load). `MapMarkerManager` is single-owner inside `MarkerScanner` — no
shared state. Cross-thread coordination is limited to:

- `D2Context` — read-only after construction, shared via `Arc`.
- `D2Injector` — `Arc<Mutex<>>`. Both threads call `CreateRemoteThread`
  through the same scratch arena; the mutex is held only for the
  duration of one call sequence. Items-side lock acquisition happens
  on every new item (`GetItemName` + `GetItemStats`); marker-side
  acquisition happens once per BFS for `MapMarkerManager::tick`.
  Contention is negligible at observed call rates.
- `recent_events` — `RwLock<HashMap<u32, ItemDropEvent>>` inside
  `Arc<SharedScannerState>`. Items thread is the sole writer. Marker
  thread snapshots via `clone()` at the start of each BFS pass,
  releases the read lock, then iterates on the local snapshot. This
  avoids holding the lock across `decide()` calls, which would
  re-introduce items-side blocking.
- `clear_markers` — `AtomicBool`. Items thread sets it on game-entry
  transitions (`ingame && !was_ingame`); marker thread reads-and-resets
  it at the top of each tick and calls `MarkerScanner::clear` if set.
  Covers the rare same-area respawn case where
  `MapMarkerManager`'s subtile-jump area-change heuristic would not
  fire.
- `stop` — `AtomicBool`. Items thread sets it after its loop exits;
  marker thread checks at each iteration boundary. Cancellation is
  epoch-grained (mid-BFS exits wait for current pass to finish, up to
  ~700 ms in release on crowded maps).

`SharedScannerState` (in `src-tauri/src/scanner_state.rs`) bundles the
above into one `Arc`-shared struct; both `DropScanner` (items thread)
and `MarkerScanner` (marker thread) hold their own `Arc<>` clone.

## Known remaining cost

After Phase 2, BFS no longer affects notification latency. It remains
~260 ms / ~700 ms per pass (dev / release) on crowded maps, but runs
on its own thread so this only affects automap marker freshness, not
notification cadence.

### Candidate next steps (still unimplemented)

Listed roughly in ascending order of risk/effort. Stop when the
remaining cost stops mattering.

1. **Skip BFS when nothing depends on it.** Short-circuit
   `run_map_marker_pass` if `filter.rules.iter().all(|r| !r.map)` and
   `self.map_marker.persistent.is_empty()`. Users without `map:true`
   rules pay zero marker-pass cost.

2. **Throttle BFS to every N ticks** (e.g. N = 3-5). Marker
   reconciliation becomes slightly less responsive (an item picked up
   could keep its marker for up to N × 30 ms + BFS time) but per-tick
   cost averages down proportionally.

3. **Reduce BFS depth from 10 to 3-5.** Depth 10 covers rooms well
   beyond what's visible on the minimap in most areas. A shallower BFS
   trades a small amount of marker persistence for far fewer memory
   reads.

4. **Cache `filter.decide()` by `unit_id`.** Decisions are pure over
   `(item, rules)`. Invalidate on `filter_config_generation` bump.
   Replaces `O(items × rules)` per tick with `O(items)` + one-time
   `O(rules)` per item.

5. **Batch `ReadProcessMemory`.** Pull whole rooms or whole room
   sub-ranges into a local buffer in one syscall, then parse in Rust.
   Biggest win for BFS and `pPaths` walks. Highest implementation
   cost.

These are now CPU-bandwidth optimizations, not latency fixes. They
matter only for users on weak hardware where the marker thread eating
~10% of one core is felt elsewhere.

## Open questions (for later investigation)

- Why is dev-build `pPaths` per-item CPU work roughly 8× slower than
  release? (`211 ms/scan` dev vs `24 ms/scan` release.) Debug overhead
  on `strip_color_codes` + `filter.decide` is the prime suspect but
  has not been profiled directly.
- Is there a cheap way to fingerprint the BFS room set and skip the
  whole pass when it matches the prior tick? A "nothing has changed"
  hash of `(room pointers, per-room unit counts)` would be much
  cheaper than the current work but has not been prototyped.
