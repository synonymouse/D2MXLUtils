# Loot History Overlay — Design

**Status:** shipped; documents reflect the implementation as of 2026-05-01 + post-ship refinements (see Phase 7 at end).
**Date:** 2026-05-01

## Overview

A user-facing in-game loot log. The user opens an overlay panel by hotkey (default `N`) and sees a chronological list of items that dropped in the current session, with a visual indicator for each entry showing whether the item was picked up **by the local hero**, taken by **someone else**, or has **gone Lost** from this session. False positives in multiplayer are eliminated by classifying ownership against the local player's inventory only — a teammate's pickup is never marked as ours.

## Goals

- Show the player what dropped during the current game session.
- Mark each entry as picked-up / lost / pending using ground truth from item state, not heuristics.
- Zero false positives in multiplayer: a teammate picking up an item must NEVER be marked as picked up by us.
- Reuse existing overlay window and existing scanner tick — no new background threads, no new windows.

## Non-Goals

- Persistence across sessions (lives in memory, cleared on game exit/relaunch).
- Logging items the user can't see (`hide` rules, `Default` under `hide_all`, items without `notify`).
- Logging stash/cube/belt destinations — main inventory only.
- Statistics, run timers, exports, search — out of scope.
- Active click hooking via DLL injection — passive ownership/inventory check is sufficient.

## Scope Decisions (from brainstorming)

| Question | Decision |
|---|---|
| Lifetime | Session-only. Cleared on `menu → ingame` transitions and on app shutdown. On `ingame → menu` (player exits to character select / main menu), every still-`Pending` entry is force-transitioned to `Lost` since the session is effectively ending. |
| Window | Reuse existing overlay (`OverlayWindow.svelte`); add `LootHistoryPanel` component, toggled visible. |
| Panel layout | Centered, semi-transparent, scrollable list. |
| Hotkey | Default `N`, configurable in General tab. |
| Items captured | Only entries where the loot filter returns `notify = Some(...)`. Same set the user sees in `NotificationStack`. |
| Pickup scope | Local hero's main inventory chain only. Items moved to stash/cube count as "not picked up". |
| Per-entry display | `[time] <pickup-icon> <name-in-filter-color>`. No stats, no quality label, no base name. |
| Sorting | Insertion order (chronological top-to-bottom, no reversal). |
| Cap | 300 entries, FIFO eviction. |
| Dedup across area unload/reload | Same physical item is identified by `dwSeed` (random seed at `ItemData + 0x14`, stable across area reload). Re-sightings update the existing row in place; no duplicate row is appended. |

## Architecture

### Reuse, not rebuild

- **No new Tauri window.** The existing overlay (label `"overlay"`) is fullscreen, already synced with the D2 client rect every 250ms, and already supports a "make-interactive" mode (`set_overlay_interactive`) for edit-mode notifications. The history panel piggybacks on that pattern.
- **No new scanner thread.** Pickup resolution runs inside the existing `tick_items` cadence (~30ms) right after the `pPaths` walk, before the marker pass.

### Component map

```
src-tauri/src/
  notifier.rs              — extend with pickup resolution + LootHistory state
  loot_history.rs          — new module: LootEntry, PickupState, ring buffer
  offsets.rs               — add item_data::INV_OWNER (if Phase 0 finds it)
  hotkeys.rs               — add LootHistoryHotkeyState
  main.rs                  — register Tauri commands + events
  settings.rs              — add loot_history_hotkey field

src/
  components/LootHistoryPanel.svelte  — new: scrollable list UI
  views/OverlayWindow.svelte          — embed panel, handle toggle
  views/GeneralTab.svelte             — hotkey binding row
  stores/loot-history.svelte.ts       — new: in-memory store, listens to events
```

## Data Model

### Backend (Rust)

```rust
// loot_history.rs
pub struct LootEntry {
    pub unit_id: u32,                 // current sighting's UnitAny.unit_id
    pub timestamp_ms: u64,            // wall-clock at first sighting
    pub name: String,                 // final display name
    pub color: Option<String>,        // lowercase keyword from winning rule's `color`
                                      // (e.g. "lime", "gold"), None = default
    pub pickup: PickupState,

    // --- runtime fields (skip-serialize); kept fresh by `push` seed-merge ---
    pub p_unit_data: u32,             // ItemData* — for INV_OWNER read
    pub p_unit: u32,                  // UnitAny*  — for slot-freed read
    pub seed: u32,                    // dwSeed @ ItemData+0x14, stable per item
    pub ticks_since_left_ground: u8,  // reserved (no longer drives transitions)
}

#[serde(rename_all = "snake_case")]
pub enum PickupState {
    /// On the ground, in flight, or in an unloaded area. Default after push.
    Pending,
    /// Found in the local hero's inventory. Terminal.
    PickedUp,
    /// Resolved via INV_OWNER to a non-self container (vendor / corpse /
    /// occasionally a visible MP teammate). Terminal.
    TakenByOther,
    /// Slot reclaimed for another live unit (typical: another player picked
    /// it up in MP), or session ended via menu exit. Terminal — but a same-
    /// `seed` re-sighting can resurrect it back to `Pending`.
    Lost,
}

pub struct LootHistory {
    entries:    VecDeque<LootEntry>,     // cap 300, FIFO
    by_unit_id: HashMap<u32, usize>,     // O(1) state updates
    by_seed:    HashMap<u32, usize>,     // dedup across area unload/reload
}

pub enum PushOutcome {
    Inserted,                            // new row in panel
    Merged,                              // existing Pending row re-keyed to new uid
    Resurrected { unit_id: u32 },        // existing Lost row flipped back to Pending
    Duplicate,                           // no-op (uid collision or terminal seed match)
}
```

Both index maps are rebuilt on FIFO eviction. At 300-entry cap and tens of evictions per session this is negligible.

### Frontend payload

```ts
interface LootHistoryEntry {
  unit_id: number;
  timestamp_ms: number;
  name: string;
  color: string | null;
  pickup: 'pending' | 'picked_up' | 'taken_by_other' | 'lost';
}
```

### Events

| Event | Payload | When |
|---|---|---|
| `loot-history-entry`   | `LootHistoryEntry`            | New row inserted into history (`PushOutcome::Inserted` only — dedup-merges and resurrections do NOT re-emit). |
| `loot-history-update`  | `{ unit_id, pickup }`         | Pickup state changed: `PickedUp`, `TakenByOther`, `Lost`, or `Pending` (the last fires on resurrection so the panel can flip the icon back). |
| `loot-history-cleared` | `null`                        | New game detected (`menu → ingame`); frontend wipes its store. |

Frontend store keyed by `unit_id` collapses these naturally. On `Merged` the backend re-keys the entry's `unit_id` to the new sighting; subsequent `loot-history-update` events use the new uid.

## Pickup Detection Algorithm

The shipped algorithm uses **both** the player-inventory walk and the `INV_OWNER` back-pointer, plus a conservative slot-reclaimed signal and a `dwSeed`-based dedup across area transitions. It runs inside the existing `tick_items` cadence (~30ms) and is fully bypassed when no entry is `Pending` (perf opt — zero extra reads in the steady state once everything has terminated).

### Required offsets (all in `src-tauri/src/offsets.rs`)

| Offset | Purpose |
|---|---|
| `d2client::PLAYER_UNIT  = 0x11BBFC` | Local hero UnitAny pointer-of-pointer. |
| `unit::UNIT_ID          = 0x0C`     | `UnitAny.unit_id`. |
| `unit::UNIT_DATA        = 0x14`     | `UnitAny.pUnitData`. |
| `unit::INVENTORY        = 0x60`     | `UnitAny.pInventory`. |
| `inventory::FIRST_ITEM  = 0x0C`     | `Inventory.pFirstItem` (UnitAny*). |
| `inventory::OWNER       = 0x08`     | `Inventory.pOwner` (UnitAny*). |
| `item_data::INV_OWNER   = 0x5C`     | `ItemData.pOwnerInventory` (back-pointer). |
| `item_data::NEXT_ITEM   = 0x64`     | `ItemData.pNextInvItem` for inventory walk. |
| `item_data::SEED        = 0x14`     | `ItemData.dwSeed` — stable per-item identity. |

### Per-tick flow inside `tick_items`

```
1. pPaths walk + scan-and-emit (existing code, unchanged shape)
   For each new ITEM whose loot filter returns notify=Some:
     a. Read dwSeed from p_unit_data + 0x14.
     b. Push LootEntry { Pending, p_unit, p_unit_data, seed } via LootHistory::push.
        push() result drives downstream emit:
          - Inserted    → emit `loot-history-entry`
          - Merged      → no event (existing row keeps pointers refreshed)
          - Resurrected → enqueue `loot-history-update { Pending }`
          - Duplicate   → no-op
2. If LootHistory has any Pending entries:
   a. Walk player inventory chain ONCE per tick:
        PLAYER_UNIT → UnitAny.pInventory(0x60)
                    → Inventory.pFirstItem(0x0C)
                    → loop: read UnitAny.unit_id(0x0C), advance via
                            UnitAny.pUnitData(0x14) + ItemData.NEXT_ITEM(0x64)
      Result: our_inventory_ids: HashSet<u32>.
   b. Per-Pending classification (skip uids already in our_inventory_ids):
        - SLOT-FREED CHECK (conservative):
            read p_unit + 0x0C → live_uid.
            If Ok(non-zero) AND live_uid != entry.unit_id → freed_ids.insert(uid).
            Err or Ok(0) → DO NOT mark freed (treated as area-unmapped or
                           transient zero; the seed-merge path handles return).
        - INV_OWNER CHECK (only if not freed):
            read p_unit_data + 0x5C → Inventory*.
            NULL → on ground (or freed) → leave Pending.
            Non-NULL → Inventory + 0x08 → owner UnitAny → +0x0C → owner_uid.
              owner_uid == our_uid → our_inventory_ids.insert(uid).
              owner_uid != our_uid → other_container_ids.insert(uid).
3. resolve_pending(our, other, freed) for every Pending entry, in priority:
     1. uid ∈ our_inventory_ids   → PickedUp     (terminal)
     2. uid ∈ other_container_ids → TakenByOther (terminal)
     3. uid ∈ freed_ids           → Lost         (resurrectable via seed)
     4. else                      → stay Pending
   Emits `loot-history-update` for every transition.
```

### Why no auto-Lost timeout

Earlier iterations had a "tick counter, if off `pPaths` for N ticks → Lost" path. It was removed because it false-positives on every TP / waypoint / town visit when the item is still physically on the ground. Items now stay `Pending` indefinitely without positive evidence. The session-end sweep (see below) covers the case of items left behind at game exit.

### Why "different non-zero live_uid" only, for freed-detection

When another player picks up an item in MP, the area stays loaded; the item's `UnitAny` is freed back to D2's pool and quickly reused for another live unit (monster, missile, item). Reading `cached_p_unit + 0x0C` then yields a new, non-zero `unit_id` ≠ the original — a strong signal "that slot is no longer ours".

When the player teleports / changes area, the old area's memory is unmapped or zeroed. Reading the same address yields `Err` or `Ok(0)`. Treating those as "freed" was the source of false positives in earlier iterations; the conservative rule (`live_uid` must be non-zero AND different) avoids them. Whatever rare false positives remain are absorbed by the seed-resurrection path below.

### Seed-based dedup and resurrection (`LootHistory::push`)

`dwSeed` (random seed used to roll the item) is stable per physical item across area unload/reload and across MP server round-trips. Used as the primary dedup key:

| Existing entry's state | Push outcome | Effect |
|---|---|---|
| no match by `seed` | `Inserted` | New row appended; `loot-history-entry` emitted. |
| `Pending` | `Merged` | Re-key existing row's `unit_id` / `p_unit` / `p_unit_data` to the new sighting. No `entry` event; classifier follows the new handle next tick. |
| `Lost` | `Resurrected { unit_id }` | Same as `Merged` plus flip pickup back to `Pending` and emit a `loot-history-update { pickup: Pending }`. Covers the case where a brief TP cycle tripped the slot-freed check on a still-extant item. |
| `PickedUp` / `TakenByOther` | `Duplicate` | No-op — terminals are truthful about the prior sighting and must not be disturbed. |
| `seed == 0` (read failed) | falls back to `unit_id` dedup | Items with no readable seed are deduped by uid only. |

### Session reset & menu sweep

- **`menu → ingame`** (player picks a character / starts a new game): `LootHistory::clear()` + emit `loot-history-cleared`. Frontend wipes its store.
- **`ingame → menu`** (player exits to character select / main menu): `LootHistory::mark_all_pending_lost()` flips every still-`Pending` entry to `Lost` and emits one `loot-history-update` per transition. Rationale: leaving the game effectively ends the session for those items. The history rows stay visible until the next `menu → ingame`.

Both transitions are detected by the existing `was_ingame`/`ingame` machinery in `main.rs`.

### Performance

The Pending-empty fast-path returns `(empty, empty, empty)` from `classify_pending_pickups` before doing any RPM, so the steady state (everything terminated) costs zero extra reads per tick. With `N` Pending entries + `M` items in the player's inventory, per-tick cost is:
- `M + 3` RPMs for the inventory walk (entry into the chain + 3 per item: unit_id, pUnitData, NEXT_ITEM).
- `1`–`4` RPMs per Pending entry (slot-freed check + optional INV_OWNER chain).
- Typical loaded session: ~30–80 RPMs/tick = 0.1–0.5 ms at ~3 µs/RPM. Negligible against the existing `pPaths` walk.

## UI

### Layout

```
┌──────────────────────────────────────────────────┐
│  Loot History                       Clear   ✕    │
├──────────────────────────────────────────────────┤
│  [12:34:01] ✓  Tyrael's Might SU                 │  (gold,   PickedUp)
│  [12:34:08] ⊘  Sacred Armor                      │  (white,  Lost)
│  [12:34:11] ⏳ Stone of Jordan                    │  (yellow, Pending)
│  [12:34:14] →  Crystal Sword                     │  (blue,   TakenByOther)
│  [12:34:15] ✓  Rune Mal                          │  (orange, PickedUp)
│  ...                                             │
└──────────────────────────────────────────────────┘
```

### Visual specs

- **Position:** centered (`position: fixed; top: 50%; left: 50%; transform: translate(-50%, -50%)`).
- **Size:** ~50–60% of overlay viewport (`max-width: min(700px, 60vw); max-height: 70vh`).
- **Background:** `rgba(0, 0, 0, 0.85)` — slight transparency so the game shows through.
- **Border:** existing overlay border tokens.
- **Scroll:** `overflow-y: auto`. New entries appended at bottom; container auto-scrolls to bottom only if user is already near bottom (don't yank scroll if user is reading older entries).
- **No virtualization** — 300 rows is trivial for a Svelte list.
- **Pickup icons:**
  - `✓` PickedUp — green tint
  - `→` TakenByOther — neutral blue (someone else has it; never claimed by us)
  - `⊘` Lost — gray, slightly faded
  - `⏳` Pending — yellow/amber, animated pulse optional
- **Name color:** from `entry.color`; fallback to neutral white. Same color logic as `Notification.svelte`.
- **Time format:** `HH:MM:SS` based on `timestamp_ms` (relative to session start? or wall-clock?). **Decision:** wall-clock, `HH:MM:SS`. Easier mental anchor for the player.

### Interactive mode toggle

When the panel becomes visible:

1. Frontend calls `invoke('set_overlay_interactive', { active: true })` (existing command, already used by edit-mode for notifications).
2. Panel rendered with `pointer-events: auto`, container can receive scroll.
3. Game stops receiving clicks while the panel is open. Acceptable — the user opened a UI.

When hidden: reverse, returns to `WS_EX_TRANSPARENT` click-through.

`NotificationStack` continues rendering on top of the panel (or behind, depending on z-index — **decision:** above, so the user sees fresh drops while reviewing history).

### General tab UI

New section "Loot History" added to `GeneralTab.svelte`:

```
Loot History
  Hotkey:  [N            ]   (rebind)
  Hint:    Press the hotkey to toggle the in-game loot log.
```

Reuses the existing keybinding component used for other hotkeys.

## Hotkey Integration

- New `LootHistoryHotkeyState` in `hotkeys.rs`, modeled on `EditModeState`.
- Setting field: `loot_history_hotkey: KeyBinding` in `settings.rs`. Default = bare `N`.
- Watcher: when hotkey is pressed AND D2 window is foreground AND scanner is in-game, emit `toggle-loot-history` to the overlay webview.
- Frontend overlay listens, flips `historyVisible` state, calls `set_overlay_interactive`.

## Implementation Phases

### Phase 0 — Locate `pInvOwner` offset (blocking gate)

0.1. Search public sources for ItemData ownership field offset, in priority order:
   - **D2MOO** (`D2ItemsTxt.h`, `D2Inventory.h`, `D2UnitStrc`)
   - **D2BS** (`Constants.h`)
   - **PlugY** source (closest to MXL's 1.13c base)
   - **BH Maphack** / **D2Hackit**

0.2. If not found in sources, reverse with Cheat Engine:
   - Run D2 + MXL, attach CE.
   - Drop a known item, capture `p_unit_data` from our scanner log.
   - Add `p_unit_data` to CE; record byte snapshot of ItemData (256 bytes).
   - Pick up item; diff bytes. The field that flipped from 0 → local-player-UnitAny-ptr (or to an Inventory ptr that backlinks to player) is `pInvOwner`.
   - Cross-validate with a second item.

0.3. Document findings in `docs/loot-history-reverse-engineering.md` (offset value, validation approach, MXL build version).

0.4. **Decision gate:**
   - Found offset → use single-read variant (`pInvOwner`).
   - Not found → fall back to inventory-walk variant. Phase 1+ unblocked either way.

### Phase 1 — Backend pickup detection

1.1. Create `loot_history.rs` with `LootEntry`, `PickupState`, ring buffer (`VecDeque<LootEntry>` cap 300, `HashMap<unit_id, idx>`).

1.2. Wire into `DropScanner`:
   - Hold `Arc<RwLock<LootHistory>>`.
   - On each drop matching filter with `notify=Some`, append `Pending` entry.
   - After pPaths pass, build `current_inventory_ids` via inventory walk (or read `pInvOwner` if 0.4 succeeded).
   - Resolve all `Pending` entries; emit updates.

1.3. Session reset: hook into existing `was_ingame` → `ingame` transition in `main.rs`.

1.4. Unit tests:
   - FIFO: push 301 entries, head is gone, tail is newest, `by_unit_id` consistent.
   - State machine: Pending → PickedUp on inventory hit; Pending → Lost after 6 ticks off-ground without inventory hit.
   - Re-drop case: same `unit_id` reappears on ground after PickedUp — does NOT create a second history entry (drop already in cache; we keep the original, but state stays PickedUp). Document this as the chosen behavior.

### Phase 2 — Tauri commands and events

2.1. `get_loot_history()` → `Vec<LootHistoryEntry>` snapshot.
2.2. `clear_loot_history()` (manual; e.g., debug button or future feature).
2.3. Events: `loot-history-entry`, `loot-history-update`, `loot-history-cleared`.
2.4. Register in `main.rs` `tauri::Builder`.

### Phase 3 — Frontend overlay panel

3.1. `src/stores/loot-history.svelte.ts`:
   - Holds `Map<unit_id, LootHistoryEntry>` + ordered ID list.
   - Subscribes to all three events on mount.
   - On `loot-history-entry`: append.
   - On `loot-history-update`: mutate by id.
   - On `loot-history-cleared`: reset.

3.2. `src/components/LootHistoryPanel.svelte`:
   - Reads from store.
   - CSS-grid 3 cols (time / icon / name).
   - Auto-scroll-to-bottom on new entry only if user is near bottom (track `scrollTop + clientHeight >= scrollHeight - 50`).
   - Color resolution mirrors `Notification.svelte`.

3.3. Wire into `OverlayWindow.svelte`:
   - `historyVisible` state.
   - On toggle: call `set_overlay_interactive(true/false)` (same as edit-mode pattern).
   - Listen for `toggle-loot-history` event.

### Phase 4 — Hotkey

4.1. `LootHistoryHotkeyState` in `hotkeys.rs`.
4.2. Default `N` in `settings.rs`.
4.3. UI row in `GeneralTab.svelte`.
4.4. Watcher emits `toggle-loot-history` to overlay webview.

### Phase 5 — Polish & QA

5.1. Manual SP run (Cow Level / Diaclo / Tran Athulua): 100+ drops, mix of picked/skipped, verify states.
5.2. Manual MP run: teammate picks up filtered drops — verify they show as `Lost`, never `PickedUp`.
5.3. Stress: spam-drop 350+ items via `/players 8` farm, verify FIFO and UI smoothness.
5.4. Edge cases:
   - Town↔area transition (Waypoint, TP) — history persists.
   - Exit to menu and re-enter (new game) — history clears.
   - App restart — history is gone.
   - Item picked up that wasn't in history (e.g. dropped by player from inventory and then re-picked) — no history entry, no error.
   - Hotkey toggled rapidly — no leak in `set_overlay_interactive` calls.

## Open Risks & Known Limitations

- **`INV_OWNER` resolved (Phase 0).** Cross-validated against four sources (D2BS, BH Maphack, PlugY, 1.11B headers) at `0x5C`; `inventory::OWNER` at `0x08`. Both shipped. The per-Pending `INV_OWNER` read is the primary `TakenByOther` signal; the player-inventory walk is the primary `PickedUp` signal (robust against stale `p_unit_data` after pickup).
- **MP `TakenByOther` reach is limited.** D2 only mirrors another player's inventory contents into your client when the item is in a directly-visible container (your view of a vendor / corpse / your own follower). Items that go straight into a remote teammate's inventory are NOT visible — those resolve as `Lost` via the slot-freed check instead.
- **TP / area unload no longer false-fires `Lost`.** The freed-detection only fires on a *different non-zero* `unit_id` read; area-unmapped pages return `Err` or `Ok(0)` and stay `Pending`. If a fast slot reuse during TP ever does trip Lost, returning to the area resurrects the entry to `Pending` via `seed`.
- **Resurrection only flips Lost → Pending.** `PickedUp` and `TakenByOther` are truthful terminals; a same-`seed` re-sighting is treated as `Duplicate`. (This protects against a pickup being undone by a re-drop — the original pickup record is preserved.)
- **Items with `seed == 0`.** A read failure at push time leaves seed=0; those entries dedupe by `unit_id` only and won't survive an area-reload uid reassignment. Rare in practice — `dwSeed` is populated for every random-roll item.
- **Auto-stack items (runes, gold).** Gold typically doesn't match `notify` rules. Stacking runes whose engine-side `unit_id` is consumed by an existing stack's id will stay `Pending` until the next signal (typically Lost on session end). The log is best-effort; the in-game feedback is authoritative.
- **Hotkey conflict.** Bare `N` is uncommitted in MXL/D2 default keys. Settings layer already validates duplicates and rebinds via `update_loot_history_hotkey`.
