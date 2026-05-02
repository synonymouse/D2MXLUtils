# Marker Thread Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple the map-marker BFS pass from the item-scanning pass by running it on a dedicated thread, so `tick_items` cadence is no longer capped by BFS duration. Targets the residual cost documented in `docs/drop-notification-latency.md` ("Known remaining cost").

**Architecture:** Split the current single-threaded scanner loop into two threads sharing a thread-safe state bundle. Items thread keeps emitting `item-drop` events at ~30ms cadence and writes enriched events to a shared `recent_events` map. Markers thread runs its own loop, snapshots `recent_events`, and reconciles automap markers. `D2Injector` is wrapped in a `Mutex` so both threads can call `CreateRemoteThread` safely without two scratch arenas. `D2Context` is shared via `Arc` (its `ProcessHandle` is a `HANDLE` which is safe to share for `ReadProcessMemory`/`CreateRemoteThread`). Single hook injection — `LootFilterHook` stays in items thread, marker thread does not touch it.

**Tech Stack:** Rust, `std::sync::{Arc, Mutex, RwLock}`, `std::sync::atomic::AtomicBool`, `std::thread`, Tauri v2 (already in use).

**Risk profile:** Mostly compile-checked refactor (Rust forces `Send + Sync` correctness). Main runtime risk is lock contention and area-change races; both are mitigated by the snapshot pattern below. Behavior visible to the user must remain identical except for faster `tick_items` cadence.

**Out of scope:**
- Reducing BFS depth, throttling BFS, or caching `filter.decide` (those are separate optimizations from the original doc, untouched here).
- Replacing per-field `ReadProcessMemory` with batched reads.
- Any UI changes.

---

## File Structure

**Modified:**
- `src-tauri/src/process.rs` — add `unsafe impl Send + Sync for ProcessHandle` (HANDLE is a kernel-object reference, safe for read/inject ops from multiple threads).
- `src-tauri/src/notifier.rs` — significant: extract marker pass into a new module, change shared state types, drop `tick_map_markers`/`run_map_marker_pass` from `DropScanner`.
- `src-tauri/src/main.rs` — replace single scanner loop with two threads; wire stop signal to both.
- `src-tauri/src/injection.rs` — possibly add `Send + Sync` impl if not already present (compile errors will tell).

**Created:**
- `src-tauri/src/marker_scanner.rs` — new module hosting `MarkerScanner` and its tick loop.
- `src-tauri/src/scanner_state.rs` — new module hosting `SharedScannerState` (the bundle of `Arc`-wrapped fields shared by both threads).

**Untouched (verify nothing leaks):**
- `src-tauri/src/map_marker.rs` — `MapMarkerManager` stays single-owner inside `MarkerScanner`. No locking added inside.
- `src-tauri/src/loot_filter_hook.rs` — owned by `DropScanner`, not shared.
- `src-tauri/src/loot_history.rs` — already `Arc<RwLock<LootHistory>>`, unchanged.
- `src-tauri/src/rules/**` — unchanged.

---

## Pre-flight

- [ ] **Step 0.1: Verify clean working tree, branch off**

```bash
git status
git switch -c perf/marker-thread-split
```

- [ ] **Step 0.2: Note current performance baseline**

Launch `pnpm tauri dev`, attach D2, walk into a populated area (e.g. crowded town or end-of-map pile). Open `d2mxlutils.log` next to the exe. There is no per-tick timer logging today — set baseline by feel: "drop, count seconds until notification, count seconds until automap marker appears". Write the numbers in commit message of Task 7. This is for sanity-check at the end, not gating any task.

---

### Task 1: Make `ProcessHandle` and `D2Injector` thread-safe primitives

**Why first:** Every later task depends on these being shareable. If this doesn't compile, nothing else will.

**Files:**
- Modify: `src-tauri/src/process.rs` (after line 40)
- Modify: `src-tauri/src/injection.rs` (verify `D2Injector` Send/Sync — see step 1.3)

- [ ] **Step 1.1: Add `Send + Sync` to `ProcessHandle`**

Open `src-tauri/src/process.rs`. After the `Drop` impl for `ProcessHandle` (around line 40), add:

```rust
// SAFETY: HANDLE is a kernel-object reference. ReadProcessMemory,
// CreateRemoteThread, VirtualAllocEx, and the other Win32 calls we make
// against this handle are all thread-safe per MSDN. The `pid` field is a
// plain u32. CloseHandle in Drop happens once, when the last Arc is
// released, by definition single-threaded.
unsafe impl Send for ProcessHandle {}
unsafe impl Sync for ProcessHandle {}
```

- [ ] **Step 1.2: Build and confirm it compiles**

Run: `cd src-tauri && cargo check`
Expected: Clean build (no errors related to `ProcessHandle`).

- [ ] **Step 1.3: Audit `D2Injector` for `Send + Sync`**

Open `src-tauri/src/injection.rs`. Read the `D2Injector` struct definition. List its fields. Two cases:

- **Case A:** All fields are `usize`/`u32`/`Vec<u8>`/`String` plus `ProcessHandle` references — already `Send + Sync` after step 1.1. Skip step 1.4.
- **Case B:** Has any raw pointer (`*mut`, `*const`) field — needs explicit `unsafe impl Send + Sync` on `D2Injector`.

If Case B, proceed to 1.4. Otherwise commit and move on (step 1.5).

- [ ] **Step 1.4: Add `Send + Sync` to `D2Injector` if needed**

If 1.3 found raw pointers, append to `injection.rs`:

```rust
// SAFETY: `D2Injector` owns a remote-allocated arena in the D2 process,
// addressed by usize. The raw pointers it holds reference D2-process
// memory, not Rust-process memory. All access goes through Win32 which
// is thread-safe; cross-thread access from Rust is safe as long as we
// guard the *arena* (scratch buffer offsets) with a Mutex at the
// caller side, which we do (see Task 2).
unsafe impl Send for D2Injector {}
unsafe impl Sync for D2Injector {}
```

- [ ] **Step 1.5: Build clean and commit**

```bash
cargo check
cargo fmt
git add src-tauri/src/process.rs src-tauri/src/injection.rs
git commit -m "refactor(scanner): mark ProcessHandle and D2Injector Send+Sync"
```

---

### Task 2: Introduce `SharedScannerState` and wrap injector in `Mutex`

**Goal:** Bundle the cross-thread state into one struct, so both threads receive a single `Arc<SharedScannerState>` and we don't pass six separate Arcs around. `DropScanner` is rewired to use this bundle internally.

**Files:**
- Create: `src-tauri/src/scanner_state.rs`
- Modify: `src-tauri/src/notifier.rs`
- Modify: `src-tauri/src/main.rs:lib.rs` (or wherever modules are declared)

- [ ] **Step 2.1: Create `scanner_state.rs`**

Find the file that declares modules (likely `src-tauri/src/main.rs` or `src-tauri/src/lib.rs`). Find the `mod notifier;` line. After it, plan to add `mod scanner_state;`.

Create `src-tauri/src/scanner_state.rs`:

```rust
//! Shared state bundle accessed by both the items scanner thread and the
//! marker scanner thread. Wrap-once at startup, clone the outer `Arc` to
//! pass to each thread.
//!
//! Locking discipline:
//! - `injector` Mutex is held only for the duration of one CreateRemoteThread
//!   call sequence. Never hold it across a `recent_events` lock.
//! - `recent_events` writers (items thread) acquire the write lock briefly
//!   to insert/retain. Readers (marker thread) snapshot via `clone()` under
//!   read lock, then release before doing per-item work.
//! - `filter_config` is the existing Arc<RwLock<FilterConfig>> from notifier.
//!   Both threads read it.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use crate::injection::D2Injector;
use crate::notifier::ItemDropEvent;
use crate::process::D2Context;
use crate::rules::FilterConfig;

pub struct SharedScannerState {
    pub ctx: Arc<D2Context>,
    pub injector: Arc<Mutex<D2Injector>>,
    pub filter_config: Arc<RwLock<Option<Arc<RwLock<FilterConfig>>>>>,
    pub filter_enabled: Arc<AtomicBool>,
    /// Enriched events from the items thread, keyed by `dwUnitId`. Marker
    /// thread reads this for regex matching against item name/stats.
    pub recent_events: Arc<RwLock<HashMap<u32, ItemDropEvent>>>,
    /// Set when the scanner threads should exit their loops.
    pub stop: Arc<AtomicBool>,
}

impl SharedScannerState {
    pub fn new(ctx: D2Context, injector: D2Injector) -> Self {
        Self {
            ctx: Arc::new(ctx),
            injector: Arc::new(Mutex::new(injector)),
            filter_config: Arc::new(RwLock::new(None)),
            filter_enabled: Arc::new(AtomicBool::new(false)),
            recent_events: Arc::new(RwLock::new(HashMap::new())),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}
```

- [ ] **Step 2.2: Register the module**

In whichever file declares `mod notifier;` (search for it: `grep -rn "mod notifier" src-tauri/src/`), add immediately after:

```rust
mod scanner_state;
```

If `notifier` is `pub mod notifier`, match the visibility: `pub mod scanner_state;`.

- [ ] **Step 2.3: Build clean**

Run: `cd src-tauri && cargo check`
Expected: Compiles. The new struct is unused so far — that's fine.

- [ ] **Step 2.4: Commit**

```bash
git add src-tauri/src/scanner_state.rs src-tauri/src/main.rs
# or src-tauri/src/lib.rs if mods live there
git commit -m "refactor(scanner): introduce SharedScannerState skeleton"
```

---

### Task 3: Migrate `DropScanner` to use `SharedScannerState`

**Goal:** Replace `DropScanner`'s owned `ctx`, `injector`, `filter_config`, `filter_enabled`, `recent_events` fields with reads through `Arc<SharedScannerState>`. Behavior identical, single-threaded for now.

**Files:**
- Modify: `src-tauri/src/notifier.rs` (heavy)
- Modify: `src-tauri/src/main.rs` (DropScanner construction)

- [ ] **Step 3.1: Change `DropScanner` field types**

In `src-tauri/src/notifier.rs`, find the `DropScanner` struct (around line 70-110, near the existing fields). Replace the affected fields:

```rust
// Before:
//   ctx: D2Context,
//   injector: D2Injector,
//   filter_config: Option<Arc<RwLock<FilterConfig>>>,
//   filter_enabled: bool,
//   recent_events: HashMap<u32, ItemDropEvent>,

// After:
pub struct DropScanner {
    state: Arc<SharedScannerState>,
    // (keep all other fields: seen_items, class_cache, unique_cache,
    //  set_cache, map_marker, loot_hook, loot_history,
    //  last_pickup_updates, verbose_filter_logging, ...)
    // map_marker is REMOVED in Task 5 — leave it here for now.
    seen_items: HashSet<u32>,
    class_cache: Option<Vec<ClassInfo>>,
    unique_cache: Option<Vec<UniqueInfo>>,
    set_cache: Option<Vec<String>>,
    map_marker: MapMarkerManager,
    loot_hook: LootFilterHook,
    loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
    last_pickup_updates: Vec<(u32, u32, crate::loot_history::PickupState)>,
    verbose_filter_logging: bool,
}
```

(Adapt the field list to whatever fields actually exist — keep them all, only the five listed above are removed.)

Add the import at the top of `notifier.rs`:

```rust
use crate::scanner_state::SharedScannerState;
```

- [ ] **Step 3.2: Update `DropScanner::new` signature**

Change `pub fn new(loot_history: ...)` to take the shared state:

```rust
pub fn new(
    state: Arc<SharedScannerState>,
    loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
) -> Result<Self, String> {
    // Initialize and inject the loot filter hook
    let mut loot_hook = LootFilterHook::new();
    if state.ctx.d2_sigma != 0 {
        if let Err(e) = loot_hook.inject(&state.ctx) {
            log_error(&format!("Failed to inject LootFilterHook: {}", e));
        }
    }

    Ok(Self {
        state,
        seen_items: HashSet::new(),
        class_cache: None,
        unique_cache: None,
        set_cache: None,
        map_marker: MapMarkerManager::new(),
        loot_hook,
        loot_history,
        last_pickup_updates: Vec::new(),
        verbose_filter_logging: false,
    })
}
```

- [ ] **Step 3.3: Replace all `self.ctx` / `self.injector` / `self.filter_config` / `self.filter_enabled` / `self.recent_events` references**

In `notifier.rs`, do these replacements (each one is a small mechanical edit; do them carefully, not via `sed`):

| Old | New |
|---|---|
| `self.ctx` | `&self.state.ctx` (or `self.state.ctx.as_ref()` where coercion needs help) |
| `&self.ctx` | `&self.state.ctx` |
| `self.injector` (read access) | `self.state.injector.lock().unwrap()` — bind to a local for the call duration |
| `self.filter_enabled` | `self.state.filter_enabled.load(Ordering::Relaxed)` |
| `self.filter_config` (read) | `self.state.filter_config.read().unwrap().clone()` (returns `Option<Arc<RwLock<FilterConfig>>>`) |
| `self.recent_events.insert(...)` | `self.state.recent_events.write().unwrap().insert(...)` |
| `self.recent_events.get(...)` | (only used in marker pass — moves out of DropScanner in Task 5) |
| `self.recent_events.retain(...)` | `self.state.recent_events.write().unwrap().retain(...)` |

For setters that flip `filter_enabled`:

```rust
// Was:  self.filter_enabled = enabled;
self.state.filter_enabled.store(enabled, Ordering::Relaxed);
```

For setters that update `filter_config`:

```rust
// Was:  self.filter_config = Some(arc);
*self.state.filter_config.write().unwrap() = Some(arc);
```

Add the import:

```rust
use std::sync::atomic::Ordering;
```

**Critical for injector locking:** Whenever you call `self.injector.foo(...)` today, replace with:

```rust
{
    let injector = self.state.injector.lock().unwrap();
    injector.foo(...)
}
```

Do **not** hold the lock across calls that themselves take other locks (e.g. `self.state.recent_events.write()`). If a call site does both, drop the injector lock first by scoping it.

- [ ] **Step 3.4: Update Tauri commands in `main.rs`**

Find every place `DropScanner::new(...)` is called. Update construction:

```rust
// Before:
//   let scanner = DropScanner::new(loot_history.clone())?;

// After:
let ctx = D2Context::new()?;
let injector = D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common, ctx.d2_lang)?;
let shared_state = Arc::new(SharedScannerState::new(ctx, injector));
let scanner = DropScanner::new(shared_state.clone(), loot_history.clone())?;
// keep `shared_state` in scope — Task 6 will hand it to the marker thread
```

Find Tauri commands that today call `scanner.set_filter_enabled(...)` / `scanner.set_filter_config(...)`. They keep working unchanged because we kept the same method signatures on `DropScanner` — only the implementation shifted to writing through the shared state.

Add to `main.rs` imports:

```rust
use crate::scanner_state::SharedScannerState;
```

- [ ] **Step 3.5: Build clean**

Run: `cd src-tauri && cargo check`
Expected: Compiles. If not, the most likely error is a missed `self.ctx` somewhere — grep for any remaining `self\.\(ctx\|injector\|filter_config\|filter_enabled\|recent_events\)` in `notifier.rs`.

```bash
grep -n 'self\.\(ctx\|injector\|filter_config\|filter_enabled\|recent_events\)' src-tauri/src/notifier.rs
```

Expected: zero matches (everything goes through `self.state.*` now).

- [ ] **Step 3.6: Run the app, smoke-test**

```bash
pnpm tauri dev
```

Attach D2, drop a couple of items, confirm:
- Notifications still appear
- Automap markers still appear (still single-threaded, just rewired)
- No new errors in `d2mxlutils.log`

If anything regressed, the rewire missed something — fix before commit.

- [ ] **Step 3.7: Commit**

```bash
cargo fmt
git add -A src-tauri/src/notifier.rs src-tauri/src/main.rs
git commit -m "refactor(scanner): migrate DropScanner to SharedScannerState"
```

---

### Task 4: Extract marker pass into `MarkerScanner`

**Goal:** Move `tick_map_markers` and `run_map_marker_pass` out of `DropScanner` into a new `MarkerScanner` struct that owns `MapMarkerManager` and reads through `SharedScannerState`. Still called sequentially from main thread for this task.

**Files:**
- Create: `src-tauri/src/marker_scanner.rs`
- Modify: `src-tauri/src/notifier.rs` (remove the moved code; remove `map_marker` field from `DropScanner`)
- Modify: `src-tauri/src/main.rs` (instantiate `MarkerScanner`, swap call site)

- [ ] **Step 4.1: Create `marker_scanner.rs`**

```rust
//! Map-marker scanner. Runs the BFS pass over the room graph, decides which
//! items get automap markers, and reconciles the marker chain. Reads enriched
//! events from `SharedScannerState::recent_events` produced by the items
//! scanner. Owns `MapMarkerManager` exclusively — never touched from another
//! thread.

#![cfg(target_os = "windows")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::logger::error as log_error;
use crate::map_marker::{self, MapMarkerManager, MarkerItem};
use crate::notifier::ItemDropEvent;
use crate::offsets::unit;
use crate::rules::{MatchContext, Visibility};
use crate::scanner_state::SharedScannerState;

pub struct MarkerScanner {
    state: Arc<SharedScannerState>,
    map_marker: MapMarkerManager,
}

impl MarkerScanner {
    pub fn new(state: Arc<SharedScannerState>) -> Self {
        Self {
            state,
            map_marker: MapMarkerManager::new(),
        }
    }

    /// Run one BFS + marker reconciliation pass. No-op outside of a live
    /// game (caller checks).
    pub fn tick(&mut self) {
        if !self.state.filter_enabled.load(Ordering::Relaxed) {
            if let Err(e) = self.map_marker.clear(&self.state.ctx) {
                log_error(&format!("map_marker clear (disabled) failed: {}", e));
            }
            return;
        }

        let filter_arc = match self.state.filter_config.read() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        let Some(filter_arc) = filter_arc else { return };
        let filter = match filter_arc.read() {
            Ok(f) => f,
            Err(_) => return,
        };

        let positions = match map_marker::bfs_item_positions(&self.state.ctx, 10) {
            Ok(v) => v,
            Err(e) => {
                log_error(&format!("map_marker BFS failed: {}", e));
                return;
            }
        };

        let mut unit_ids: HashMap<u32, u32> = HashMap::new();
        let mut bfs_unit_ids: HashSet<u32> = HashSet::new();
        for &(p_unit, _, _) in &positions {
            if let Ok(uid) = self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_unit as usize + unit::UNIT_ID)
            {
                unit_ids.insert(p_unit, uid);
                bfs_unit_ids.insert(uid);
            }
        }

        // Snapshot recent_events ONCE; release the lock before per-item work.
        // Holding the read lock across decide() + map_marker.tick() would
        // block the items thread on inserts and re-introduce the latency
        // we are trying to remove.
        let snapshot: HashMap<u32, ItemDropEvent> = match self.state.recent_events.read() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };

        let mut newly_matched: Vec<MarkerItem> = Vec::new();
        for (p_unit, sub_x, sub_y) in positions {
            let Some(&unit_id) = unit_ids.get(&p_unit) else {
                continue;
            };
            let Some(event) = snapshot.get(&unit_id) else {
                continue;
            };
            let ctx = MatchContext::new(event);
            let decision = filter.decide(&ctx);
            if !decision.place_on_map || decision.visibility == Visibility::Hide {
                continue;
            }
            let (cx, cy) = map_marker::sub_to_cell(sub_x, sub_y);
            newly_matched.push(MarkerItem {
                unit_id,
                cell_x: cx,
                cell_y: cy,
                sub_x,
                sub_y,
            });
        }

        let player_sub = map_marker::read_player_subtile(&self.state.ctx);

        // Acquire injector lock for the marker reconciliation. This is the
        // ONLY lock held across multiple CreateRemoteThread calls in this
        // pass; items thread blocks here only if it is also calling the
        // injector, which is exclusively for new-item GetItemName/GetItemStats.
        let injector = match self.state.injector.lock() {
            Ok(i) => i,
            Err(p) => p.into_inner(), // poisoned: best-effort recovery
        };
        if let Err(e) = self.map_marker.tick(
            &self.state.ctx,
            &injector,
            &newly_matched,
            &bfs_unit_ids,
            player_sub,
        ) {
            log_error(&format!("map_marker tick failed: {}", e));
        }
    }

    /// Drop all markers and clear cached state. Called when D2 disappears
    /// or the scanner stops.
    pub fn shutdown(&mut self) {
        if let Err(e) = self.map_marker.clear(&self.state.ctx) {
            log_error(&format!("map_marker clear on shutdown failed: {}", e));
        }
    }
}
```

- [ ] **Step 4.2: Register the module**

Add `mod marker_scanner;` next to `mod scanner_state;` in the module-declaring file (from Task 2.2).

- [ ] **Step 4.3: Remove marker code from `DropScanner`**

In `notifier.rs`:

1. Delete the methods `tick_map_markers` and `run_map_marker_pass` (around lines 638–726).
2. Delete the `map_marker: MapMarkerManager` field from the struct definition.
3. Delete `map_marker: MapMarkerManager::new(),` from `DropScanner::new`.
4. Delete the imports that are now unused: `MapMarkerManager`, `MarkerItem`, `map_marker` if only used in deleted code. (`cargo check` will tell you which.)
5. Delete the `is_ingame()` early-return guard from the now-removed `tick_map_markers`. The new `MarkerScanner::tick` has its own guard via `filter_enabled` — but **also** add an `is_ingame` check. Since `is_ingame` is on `DropScanner`, copy that helper into `marker_scanner.rs` as a free function or duplicate the check inline.

To duplicate the check in `MarkerScanner::tick`, look up how `DropScanner::is_ingame` is implemented, then add the same logic at the top of `MarkerScanner::tick` — typically reading the player-unit pointer and bailing if zero. Add this block before the `filter_enabled` check:

```rust
// is_ingame check: bail if no player unit. Avoids touching D2 memory
// during pre-game / character-select.
let p_player = self
    .state
    .ctx
    .process
    .read_memory::<u32>(self.state.ctx.d2_client + crate::offsets::d2client::PLAYER_UNIT)
    .unwrap_or(0);
if p_player == 0 {
    return;
}
```

- [ ] **Step 4.4: Wire `MarkerScanner` into `main.rs`**

Find the scanner-loop function in `main.rs` (around line 320–435 based on prior grep). Where it currently calls `scanner.tick_map_markers()` (line 403):

```rust
// Before:
//   scanner.tick_map_markers();

// After:
marker_scanner.tick();
```

Above the loop, where the scanner is instantiated (around the `DropScanner::new` call updated in Task 3.4), add:

```rust
let mut marker_scanner = MarkerScanner::new(shared_state.clone());
```

After the loop exits and before the thread returns:

```rust
marker_scanner.shutdown();
```

Add the import:

```rust
use crate::marker_scanner::MarkerScanner;
```

- [ ] **Step 4.5: Build clean**

```bash
cd src-tauri && cargo check
```

Expected: clean. Most likely fixups:
- Unused imports in `notifier.rs` — delete them as flagged.
- Visibility: `ItemDropEvent` may need `pub` (it's a return type already — likely already pub). If `MatchContext` or `Visibility` aren't pub, bump their visibility — they need to be reachable from `marker_scanner`.

- [ ] **Step 4.6: Smoke-test**

```bash
pnpm tauri dev
```

Same scenario as Task 3.6: drops, markers, no log spam.

- [ ] **Step 4.7: Commit**

```bash
cargo fmt
git add -A src-tauri/src/notifier.rs src-tauri/src/marker_scanner.rs src-tauri/src/main.rs
git commit -m "refactor(scanner): extract MarkerScanner"
```

---

### Task 5: Spawn marker thread

**Goal:** Run `MarkerScanner::tick()` in its own OS thread instead of inline after `tick_items`. This is the change that delivers the perf win.

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 5.1: Find the scanner-loop function**

In `main.rs`, locate the function that contains `thread::sleep(Duration::from_millis(30))` at line ~433. This function is spawned as a thread today. We will spawn a sibling thread for markers.

- [ ] **Step 5.2: Extract a marker thread spawner**

Add this helper near the top of `main.rs` (or in the same module as the existing scanner spawner — match the existing pattern):

```rust
fn spawn_marker_thread(
    state: Arc<SharedScannerState>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut marker_scanner = MarkerScanner::new(state.clone());
        loop {
            if state.stop.load(Ordering::Relaxed) {
                break;
            }
            marker_scanner.tick();
            // Cadence is set by BFS duration itself (~700ms release on
            // crowded maps). The sleep is a small floor for idle ticks.
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        marker_scanner.shutdown();
    })
}
```

Add imports if missing:

```rust
use std::sync::atomic::Ordering;
```

- [ ] **Step 5.3: Remove the inline marker call from the items loop**

Where Task 4.4 placed `marker_scanner.tick();` after `scanner.tick_items()` — **delete** that line and the `MarkerScanner::new(...)` / `marker_scanner.shutdown();` lines from the items loop. Marker work moves entirely to its own thread.

The items loop body should now end with the `tick_items` work + pickup updates + the dictionary publish block + `thread::sleep(30ms)` — exactly what was there before the split commit, minus the marker call.

- [ ] **Step 5.4: Spawn the marker thread alongside the items thread**

Find where the items thread is spawned (it's the `thread::spawn(move || { ... })` containing the loop). Right after that spawn, add:

```rust
let marker_handle = spawn_marker_thread(shared_state.clone());
```

Keep the join-handles in scope so the threads aren't detached prematurely. The existing items handle was likely returned/stored in a struct field — store `marker_handle` next to it, same lifecycle.

- [ ] **Step 5.5: Wire stop signal to both threads**

Find where the existing scanner stops today (search for `is_scanning.store(false`). Today the stop signal is the `is_scanning` AtomicBool. Repurpose `state.stop` as the marker-thread stop signal — set both at the same point:

```rust
// On stop:
shared_state.stop.store(true, Ordering::Relaxed);
is_scanning.store(false, Ordering::SeqCst);
// then join both handles
```

Where the existing scanner thread is joined on shutdown (search for `.join()`), add the marker thread join right after:

```rust
let _ = marker_handle.join();
```

If the existing items thread isn't explicitly joined today (it just exits when D2 closes), do the same for the marker thread — but make sure `state.stop` is set somewhere on app shutdown (e.g. in a Tauri exit hook). If the project doesn't currently bother with clean shutdown of the scanner thread, neither do we for the marker thread — match existing precedent. Detached-thread-on-app-exit is acceptable for both.

- [ ] **Step 5.6: Build clean**

```bash
cd src-tauri && cargo check
```

Expected: clean.

- [ ] **Step 5.7: Smoke-test (critical)**

```bash
pnpm tauri dev
```

Test plan:
1. Attach D2.
2. Walk into a populated area.
3. Drop an item next to a pile.
4. **Notification should appear within ~100-300ms** (release ~30-100ms; dev a bit slower). This is the win — previously it could wait 700ms+ for BFS.
5. **Automap marker should still appear** within ~1s (BFS cadence unchanged).
6. Pick up the item — marker disappears.
7. Re-drop it — marker reappears (verifies `b3235ec` re-notify still works).
8. Walk to a different area — markers from old area cleared.
9. Close D2 — no crash, no log spam from "ReadProcessMemory failed: (PID gone)" repeated forever (a few are OK; the threads should exit).
10. Re-open D2 — scanner re-attaches, both threads resume.

If item 4 still shows multi-second latency, the marker thread is somehow still blocking items. Most likely cause: holding `recent_events.read()` across BFS — verify Step 4.1 used the snapshot pattern (`.clone()` then release).

If markers stop updating: marker thread crashed. Check log for panics.

If the app crashes on close: the marker thread is reading freed memory. Add explicit join on shutdown.

- [ ] **Step 5.8: Commit**

```bash
cargo fmt
git add -A src-tauri/src/main.rs
git commit -m "perf(scanner): run map-marker pass on dedicated thread

Decouples item-drop notification cadence from BFS duration. On crowded
maps tick_items now runs every ~30-60ms regardless of whether the marker
thread is mid-BFS, so fresh drops surface in the overlay within
~100-300ms instead of waiting on the ~700ms BFS pass.

MapMarkerManager remains single-owner inside MarkerScanner (no shared
state). Cross-thread sharing is limited to: D2Context (read-only),
D2Injector (Mutex-guarded scratch arena), filter_config (existing
RwLock), filter_enabled (AtomicBool), recent_events (RwLock + snapshot
pattern at the marker side to avoid contention).

Closes the 'Known remaining cost' tracked in
docs/drop-notification-latency.md."
```

---

### Task 6: Update the latency doc

**Files:**
- Modify: `docs/drop-notification-latency.md`

- [ ] **Step 6.1: Add a "Phase 2" section after "What shipped in this change"**

Append a new section before "Known remaining cost":

```markdown
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
- `recent_events` — `Arc<RwLock<HashMap<u32, ItemDropEvent>>>`. Items
  thread is the sole writer. Marker thread snapshots via `clone()` at
  the start of each BFS pass, releases the read lock, then iterates
  on the local snapshot. This avoids holding the lock across
  `decide()` calls, which would re-introduce items-side blocking.
```

Then update "Known remaining cost":

```markdown
## Known remaining cost

After Phase 2, BFS no longer affects notification latency. It remains
~260 ms / ~700 ms per pass (dev / release) on crowded maps, but runs
on its own thread so this only affects automap marker freshness.

### Candidate next steps (still unimplemented)

(... existing 1–5 list, unchanged ...)

These are now CPU-bandwidth optimizations, not latency fixes. They
matter only for users on weak hardware where the marker thread eating
~10% of one core is felt elsewhere.
```

- [ ] **Step 6.2: Commit**

```bash
git add docs/drop-notification-latency.md
git commit -m "docs(perf): document marker thread split (phase 2)"
```

---

### Task 7: End-to-end verification on real D2

**This is not an optional step.** The whole change is built around runtime behavior; type-check passes are necessary but not sufficient.

- [ ] **Step 7.1: Build release**

```bash
pnpm tauri build
```

Wait for the release binary. Run it (not `tauri dev`).

- [ ] **Step 7.2: Repro the original problem**

Same scenario as Step 0.2. Crowded area, drop next to a pile.

Record:
- Time from drop to overlay notification (target: <500ms).
- Time from drop to automap marker (target: ~1s, unchanged from before).
- Tick cadence under load (no easy way to measure without adding a log; eyeball: notifications stream in promptly while you're picking items up).

- [ ] **Step 7.3: Stress scenarios**

- Walk between three areas in succession; verify no leftover markers, no log spam.
- Pick up + redrop + pick up + redrop a single item; verify pickup transitions still fire (loot-history feature).
- Force a `pnpm tauri dev` build and repeat — dev is the worst case for BFS cost. Nothing should freeze for >1s now.
- Close D2 with the app running. Reopen. Verify scanner reattaches without restart.
- Close the app while D2 is running. Verify clean exit (no orphan threads — Task Manager).

- [ ] **Step 7.4: Compare against baseline (Step 0.2)**

If notifications are now noticeably faster on crowded maps and markers still work, the change succeeded. Update the commit message of Task 5 with the measured numbers if you didn't earlier.

- [ ] **Step 7.5: If everything passes, merge**

```bash
git switch master
git merge --no-ff perf/marker-thread-split
```

If anything regressed, do not merge. File a follow-up issue describing what broke and either fix on the branch or revert and rethink.

---

## Self-Review Notes

**Spec coverage:** All five risk vectors from the chat conversation are addressed —
- Mutex<D2Injector> (Tasks 1, 2, 4) → covers concurrent CreateRemoteThread.
- Snapshot pattern for recent_events (Task 4.1) → covers RwLock contention.
- MapMarkerManager single-owner (Task 4) → covers area-change races.
- Single hook injection (untouched) → covers no double-inject.
- Stop signal + thread joins (Task 5.5) → covers clean shutdown.

**Placeholder scan:** Each task has concrete code blocks. The one exception is the "list its fields" step in 1.3 — that's deliberate, the engineer is expected to read the file. Acceptable because the work that follows is conditional on what they find, and the alternatives are both spelled out.

**Type consistency:** `SharedScannerState` is defined in 2.1, used in 3, 4, 5. Methods called on it (`filter_enabled.load`, `recent_events.read`/`.write`, `injector.lock`, `stop.load`) match the field types declared in 2.1. `MarkerScanner::new` and `MarkerScanner::tick` and `MarkerScanner::shutdown` are defined in 4.1 and called in 4.4, 5.2, 5.3, 5.4 with matching signatures.

**Out-of-scope reminders:** Plan deliberately does not touch BFS depth, throttling, or filter.decide caching. Those are listed as candidate future work in the existing doc and remain there.
