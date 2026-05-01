# Loot History Overlay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-game overlay panel that lists items dropped during the current session, indicating for each whether the local hero picked it up, a teammate / vendor / corpse took it, or it was lost. Toggleable by hotkey (default `N`). Multiplayer-honest — a teammate's pickup is *labeled as theirs*, never marked as ours.

**Architecture:** Reuse the existing transparent overlay window. Inside the existing `tick_items` cadence, push session entries when the loot filter fires `notify`. Each tick, classify every still-`Pending` entry by reading `pUnitData + INV_OWNER` (Variant 2) — NULL = on ground / freed; non-NULL → dereference to `Inventory.pOwner.unit_id` and compare to the local hero's id. This yields four pickup states: `Pending`, `PickedUp`, `TakenByOther`, `Lost`. The Phase 0 reverse engineering succeeded (`item_data::INV_OWNER = 0x5C`, `inventory::OWNER = 0x08`), so Variant 2 is the primary path; the inventory-chain walk (Variant 3) is kept as a documented Phase 7 fallback only.

**Tech Stack:** Rust (Tauri v2 backend, `windows` crate for ReadProcessMemory), Svelte 5 + TypeScript frontend, vanilla CSS.

**Spec:** `docs/superpowers/specs/2026-05-01-loot-history-design.md`.

---

## File Structure

**Create:**
- `src-tauri/src/loot_history.rs` — `LootEntry`, `PickupState`, `LootHistory` ring buffer (300-entry cap, FIFO)
- `src/components/LootHistoryPanel.svelte` — scrollable list UI rendered inside overlay
- `src/stores/loot-history.svelte.ts` — Svelte store, listens to backend events
- `docs/loot-history-reverse-engineering.md` — Phase 0 findings (offset + validation, or "not found" + path forward)

**Modify:**
- `src-tauri/src/main.rs` — register module, add Tauri commands and event emits, wire session reset
- `src-tauri/src/notifier.rs` — extend `tick_items` to push entries and classify pickups via INV_OWNER
- `src-tauri/src/d2types.rs` — optional: extend `Inventory` struct only if Phase 7 fallback is invoked (Variant 2 reads raw memory, no struct change needed)
- `src-tauri/src/offsets.rs` — ✅ already updated by Phase 0: `item_data::INV_OWNER = 0x5C` and `inventory::OWNER = 0x08`
- `src-tauri/src/hotkeys.rs` — add `LootHistoryHotkeyState` (separate watcher, like `EditModeState`)
- `src-tauri/src/settings.rs` — add `loot_history_hotkey: HotkeyConfig` field
- `src/views/OverlayWindow.svelte` — embed panel, handle toggle event, set_overlay_interactive
- `src/views/GeneralTab.svelte` — hotkey-binding row for "Loot History"
- `src/components/index.ts` — export `LootHistoryPanel`
- `src/stores/index.ts` — export loot-history store

---

## Phase 0: Locate `pInvOwner` Offset (Research Gate) — ✅ DONE

> **Status (2026-05-01):** Completed. Cross-validated against BH Maphack,
> D2BS (noah-), PlugY (Speakus fork) and 1.11B (jankowskib) headers — all
> four agree:
>
> - `item_data::INV_OWNER = 0x5C` — `Inventory*` back-pointer on ItemData
> - `inventory::OWNER     = 0x08` — `UnitAny* pOwner` on Inventory (needed
>   to classify *whose* inventory holds the item)
>
> Both constants are now in `src-tauri/src/offsets.rs`. Validation log:
> `docs/loot-history-reverse-engineering.md`.
>
> **Consequence:** the implementation phases below now use **Variant 2**
> (per-Pending single-read of `INV_OWNER`) as the primary path. The old
> Variant 3 (full player-inventory walk) becomes the optional fallback in
> Phase 7. This is also what enables the `TakenByOther` pickup state — a
> walk over our own inventory cannot tell us that a teammate took it; an
> `INV_OWNER` read can.

This phase decides whether we use single-read ownership (cheap) or inventory walk (slightly more reads). Implementation phases work either way; this is informational.

### Task 0.1: Search public sources for ItemData ownership offset

**Files:**
- Create: `docs/loot-history-reverse-engineering.md`

- [x] **Step 1: Probe D2MOO** *(done — original paths 404'd; the live D2MOO header `D2Common/include/D2Items.h` only carries a forward declaration. Acceptable: 4 other sources cross-validated.)*

Use WebFetch to look at the following raw GitHub URLs (D2MOO is a clean re-implementation of D2 1.13c that uses original symbol names):

- `https://raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Items/Items.h`
- `https://raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Units/UnitsTypes.h`
- `https://raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Inventory/Inventory.h`

Search each for: `pOwnerInventory`, `pInvOwner`, `pNextInvItem`, `pPrevInvItem`, `D2ItemDataStrc`, `D2InventoryStrc`. Look at the struct layout and compute the byte offset of any owner-back-reference field by summing the sizes of preceding fields (DWORDs = 4 bytes, WORDs = 2 bytes, etc).

- [x] **Step 2: Probe D2BS** *(done — actual path is `noah-/d2bs/master/D2Structs.h`. Yielded `pOwnerInventory` @ 0x5C.)*

WebFetch:
- `https://raw.githubusercontent.com/noah-/d2bs/master/branch/1.13c/d2bs/Constants.h`
- `https://raw.githubusercontent.com/noah-/d2bs/master/branch/1.13c/d2bs/D2Structs.h`

If those URLs 404, search the repo via `https://github.com/noah-/d2bs/search?q=pInvOwner` (a results page WebFetch can read).

Search for: `ItemData`, `ItemDataStrc`, `pInvOwner`, `pOwnerInventory`, `0x60` near `Inventory`, `0x6C` near `ItemData`.

- [x] **Step 3: Probe BH Maphack** *(done — yielded `pOwnerInventory` @ 0x5C with explicit `WORD _10 // 0x5A` padding, matching the alignment math.)*

WebFetch:
- `https://raw.githubusercontent.com/planqi/slashdiablo-maphack/master/BH/D2Structs.h`
- `https://raw.githubusercontent.com/planqi/slashdiablo-maphack/master/BH/Drawing/UI/D2Stuff.cpp`

- [x] **Step 4: Probe PlugY** *(done — `L1ghtFox/plugy` 404'd, `Speakus/plugy/master/Commons/D2UnitStruct.h` succeeded. Field `ptInventory` @ +0x5C.)*

WebFetch:
- `https://raw.githubusercontent.com/L1ghtFox/plugy/master/PlugY/D2Structs.h`

PlugY is closest to MXL's 1.13c base, so its struct layout is most likely to apply directly.

- [x] **Step 5: Record findings** *(done — `docs/loot-history-reverse-engineering.md` written; decision: 0x5C, 4 sources agree.)*

Write `docs/loot-history-reverse-engineering.md` with:
- Each URL probed, success/fail
- Any candidate offset found (e.g., "D2MOO `D2ItemDataStrc` has `pOwnerInventory` at offset 0x6C from struct start")
- Cross-validation: confirm at least 2 sources agree, OR report disagreement with reasoning
- **Decision:** offset value (e.g. `0x6C`) OR "not found, fall back to inventory walk"

- [x] **Step 6: If offset found, add it to `offsets.rs`** *(done — `item_data::INV_OWNER = 0x5C` and `inventory::OWNER = 0x08` both added.)*

In `src-tauri/src/offsets.rs`, in the `item_data` module, append:

```rust
    /// Pointer back to the `Inventory` that owns this item. NULL when the
    /// item is on the ground / freed. Cross-referenced from D2MOO
    /// (`D2ItemDataStrc.pOwnerInventory`). See
    /// `docs/loot-history-reverse-engineering.md` for validation.
    pub const INV_OWNER: usize = 0xNN; // <-- replace with actual offset
```

Also add `inventory::OWNER` (the `UnitAny*` pOwner field at `+0x08`) so the
scanner can dereference one more step to compare owner unit_id against the
local hero. Skip these if Phase 0 ended in "not found".

- [ ] **Step 7: Commit**

```
git add docs/loot-history-reverse-engineering.md src-tauri/src/offsets.rs
git commit -m "docs(loot-history): record pInvOwner offset reverse-engineering"
```

(Single file commit if offset wasn't found.)

### Task 0.2 (manual, if 0.1 yielded "not found"): Cheat Engine reverse — ⏭ N/A

**Skipped:** Task 0.1 yielded a confidently cross-validated offset; no Cheat
Engine pass needed.

**Owner:** user (cannot be performed by agent — requires running game + CE).

If Step 5 of Task 0.1 ended with "not found," stop the agent here and post the following instructions to the user:

> Cheat Engine reverse protocol:
> 1. Launch D2 + MXL, attach CE.
> 2. Go in-game, drop a known unique (e.g., a TU charm).
> 3. In our app's `d2mxlutils.log`, find the `[Filter]` line for that drop and copy its `p_unit_data` value (you can also enable verbose filter logging in General tab).
> 4. In CE: "Add Address Manually" → paste the `p_unit_data` value (hex), type = "Array of byte", length = 256. Save snapshot ("Generate auto assemble script" or just screenshot the byte values).
> 5. Pick up the item. Re-read the same address. Diff bytes 0x00–0xFF.
> 6. Look for any 4-byte field that flipped from `00 00 00 00` to a non-zero pointer. Read that pointer in CE — does it land inside `D2Client.dll` near where the local player UnitAny lives, or inside an Inventory struct that points back to player? Either is a hit.
> 7. Cross-validate with a second item.
> 8. Reply with: the offset (e.g., `0x6C`), the value seen on ground (typically `0x00000000`), the value seen after pickup, and one sentence on which struct it points to.
>
> If you can't find such a field after 30 minutes, that's also a valid result — reply "not found" and we'll go with inventory walk.

Once user replies, document in `docs/loot-history-reverse-engineering.md`, add the offset to `offsets.rs` if found, and continue from Phase 1.

---

## Phase 1: Backend `loot_history` Module (Pure Data + Unit Tests)

### Task 1.1: Create `loot_history.rs` skeleton with types

**Files:**
- Create: `src-tauri/src/loot_history.rs`
- Modify: `src-tauri/src/main.rs:3-16` (add `mod loot_history;`)

- [ ] **Step 1: Create the module file**

Write `src-tauri/src/loot_history.rs`:

```rust
//! Session-only loot history: items that fired a `notify` rule, with
//! per-entry pickup state resolved against the local player's inventory.
//!
//! This module is pure data — no Win32, no D2 memory access. The scanner
//! (`notifier.rs`) drives state transitions by calling `push`, then
//! `resolve_pending` once per tick with the current ground/inventory ids.

use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum entries kept per session. Older entries are evicted FIFO.
pub const MAX_ENTRIES: usize = 300;

/// Number of scanner ticks an entry can be off-ground without appearing in
/// inventory before being marked `Lost`. At ~30ms cadence, 5 ticks ≈ 150ms,
/// which absorbs cursor-frame and container-shuffle races.
pub const LOST_TIMEOUT_TICKS: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupState {
    /// On the ground, or in flight between ground and an inventory.
    Pending,
    /// In our local hero's inventory (terminal).
    PickedUp,
    /// In someone else's inventory: another player, vendor, or corpse
    /// container (terminal). Multiplayer-honest — never claimed by us.
    /// Phrased neutrally because the same back-pointer would also fire
    /// if an item ended up in a non-player container.
    TakenByOther,
    /// Off-ground for too long without showing up anywhere we can read
    /// (terminal). Typical cause: the item despawned, or D2 freed it.
    Lost,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LootEntry {
    pub unit_id: u32,
    /// Milliseconds since UNIX epoch (set at push time).
    pub timestamp_ms: u64,
    /// Final display name as it appears in the notification.
    pub name: String,
    /// Hex/keyword color from the winning rule. `None` = default color.
    pub color: Option<String>,
    pub pickup: PickupState,
    /// Counter for the Lost timeout. Incremented while the item is off
    /// ground AND not in inventory. Resets to 0 on push.
    pub ticks_since_left_ground: u8,
    /// Cached `pUnitData` pointer captured at push time. The scanner
    /// re-reads `INV_OWNER` (offset `0x5C`) from this pointer per tick to
    /// classify pickup state. May become stale if D2 frees the item slot;
    /// in that case `ReadProcessMemory` returns Err and the entry falls
    /// through to the `Lost` timeout. Not serialized — pointers are
    /// useless to the frontend and confusing in JSON.
    #[serde(skip, default)]
    pub p_unit_data: u32,
}

/// FIFO ring of session entries. Indexed by `unit_id` for O(1) updates.
#[derive(Debug, Default)]
pub struct LootHistory {
    entries: VecDeque<LootEntry>,
    by_unit_id: HashMap<u32, usize>,
}

impl LootHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn snapshot(&self) -> Vec<LootEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_unit_id.clear();
    }
}
```

- [ ] **Step 2: Register module in `main.rs`**

In `src-tauri/src/main.rs`, locate the `mod` declarations near the top (around line 3-16) and add `loot_history` alphabetically:

```rust
mod loot_filter_hook;
mod loot_history;
mod logger;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success, no errors. (Warnings about unused fields are fine for now.)

- [ ] **Step 4: Commit**

```
git add src-tauri/src/loot_history.rs src-tauri/src/main.rs
git commit -m "feat(loot-history): add module skeleton with LootEntry types"
```

### Task 1.2: Implement `push` with FIFO eviction (TDD)

**Files:**
- Modify: `src-tauri/src/loot_history.rs`

- [ ] **Step 1: Write failing tests**

Append to `src-tauri/src/loot_history.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(unit_id: u32, name: &str) -> LootEntry {
        LootEntry {
            unit_id,
            timestamp_ms: 1000 + unit_id as u64,
            name: name.to_string(),
            color: None,
            pickup: PickupState::Pending,
            ticks_since_left_ground: 0,
            p_unit_data: 0, // Tests don't read memory; placeholder is fine.
        }
    }

    #[test]
    fn push_adds_entry_to_back_and_index() {
        let mut h = LootHistory::new();
        h.push(entry(1, "A"));
        h.push(entry(2, "B"));
        assert_eq!(h.len(), 2);
        assert_eq!(h.snapshot()[0].name, "A");
        assert_eq!(h.snapshot()[1].name, "B");
    }

    #[test]
    fn push_skips_duplicate_unit_id() {
        let mut h = LootHistory::new();
        h.push(entry(1, "A"));
        h.push(entry(1, "A again"));
        assert_eq!(h.len(), 1, "duplicate unit_id should not produce a 2nd entry");
        assert_eq!(h.snapshot()[0].name, "A");
    }

    #[test]
    fn push_evicts_oldest_when_at_cap() {
        let mut h = LootHistory::new();
        for i in 0..MAX_ENTRIES as u32 {
            h.push(entry(i, "x"));
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        h.push(entry(9999, "newest"));
        assert_eq!(h.len(), MAX_ENTRIES);
        let snap = h.snapshot();
        assert_eq!(snap[0].unit_id, 1, "oldest (unit_id=0) should be evicted");
        assert_eq!(snap[MAX_ENTRIES - 1].unit_id, 9999);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib loot_history::tests`
Expected: FAIL with "no method named `push` found".

- [ ] **Step 3: Implement `push`**

Inside `impl LootHistory`, add:

```rust
    /// Append a new entry. No-op if `unit_id` is already in the history
    /// (drop scanner can re-emit if an item flickers off pPaths and back).
    /// Evicts the oldest entry FIFO when at `MAX_ENTRIES`.
    pub fn push(&mut self, entry: LootEntry) {
        if self.by_unit_id.contains_key(&entry.unit_id) {
            return;
        }

        if self.entries.len() == MAX_ENTRIES {
            if let Some(evicted) = self.entries.pop_front() {
                self.by_unit_id.remove(&evicted.unit_id);
            }
            // VecDeque indices shifted; rebuild the index map.
            self.by_unit_id.clear();
            for (idx, e) in self.entries.iter().enumerate() {
                self.by_unit_id.insert(e.unit_id, idx);
            }
        }

        let idx = self.entries.len();
        self.by_unit_id.insert(entry.unit_id, idx);
        self.entries.push_back(entry);
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib loot_history::tests`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```
git add src-tauri/src/loot_history.rs
git commit -m "feat(loot-history): implement FIFO push with 300-entry cap"
```

### Task 1.3: Implement `resolve_pending` state machine (TDD)

**Files:**
- Modify: `src-tauri/src/loot_history.rs`

- [ ] **Step 1: Write failing tests**

Append a new test block inside the existing `mod tests`:

```rust
    fn pending_off_ground(unit_id: u32) -> LootEntry {
        let mut e = entry(unit_id, "x");
        e.ticks_since_left_ground = 0;
        e
    }

    #[test]
    fn pending_becomes_picked_up_when_in_our_inventory() {
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let ground: HashSet<u32> = HashSet::new();
        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);
        let other: HashSet<u32> = HashSet::new();

        let updates = h.resolve_pending(&ground, &ours, &other);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0], (42, PickupState::PickedUp));
        assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);
    }

    #[test]
    fn pending_becomes_taken_by_other_when_in_foreign_container() {
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let ground: HashSet<u32> = HashSet::new();
        let ours: HashSet<u32> = HashSet::new();
        let mut other: HashSet<u32> = HashSet::new();
        other.insert(42);

        let updates = h.resolve_pending(&ground, &ours, &other);
        assert_eq!(updates, vec![(42, PickupState::TakenByOther)]);
        assert_eq!(h.snapshot()[0].pickup, PickupState::TakenByOther);
    }

    #[test]
    fn ours_takes_priority_over_other_if_both_sets_collide() {
        // Defensive: scanner shouldn't put the same uid in both sets, but
        // if it does, prefer the more positive classification (PickedUp).
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let ground: HashSet<u32> = HashSet::new();
        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);
        let mut other: HashSet<u32> = HashSet::new();
        other.insert(42);

        let updates = h.resolve_pending(&ground, &ours, &other);
        assert_eq!(updates, vec![(42, PickupState::PickedUp)]);
    }

    #[test]
    fn pending_stays_pending_while_on_ground() {
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let mut ground: HashSet<u32> = HashSet::new();
        ground.insert(42);
        let ours: HashSet<u32> = HashSet::new();
        let other: HashSet<u32> = HashSet::new();

        for _ in 0..10 {
            let updates = h.resolve_pending(&ground, &ours, &other);
            assert!(updates.is_empty(), "no transition while on ground");
        }
        assert_eq!(h.snapshot()[0].pickup, PickupState::Pending);
        assert_eq!(h.snapshot()[0].ticks_since_left_ground, 0);
    }

    #[test]
    fn pending_becomes_lost_after_timeout_off_ground() {
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let ground: HashSet<u32> = HashSet::new();
        let ours: HashSet<u32> = HashSet::new();
        let other: HashSet<u32> = HashSet::new();

        // Tick LOST_TIMEOUT_TICKS times — still Pending, counter incrementing.
        for tick in 1..=LOST_TIMEOUT_TICKS {
            let updates = h.resolve_pending(&ground, &ours, &other);
            assert!(updates.is_empty(), "tick {} should not transition", tick);
            assert_eq!(h.snapshot()[0].ticks_since_left_ground, tick);
        }

        // (LOST_TIMEOUT_TICKS + 1)-th tick triggers Lost.
        let updates = h.resolve_pending(&ground, &ours, &other);
        assert_eq!(updates, vec![(42, PickupState::Lost)]);
        assert_eq!(h.snapshot()[0].pickup, PickupState::Lost);
    }

    #[test]
    fn terminal_states_do_not_emit_again() {
        let mut h = LootHistory::new();
        h.push(pending_off_ground(42));

        let ground: HashSet<u32> = HashSet::new();
        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);
        let other: HashSet<u32> = HashSet::new();

        let _ = h.resolve_pending(&ground, &ours, &other); // PickedUp
        let updates = h.resolve_pending(&ground, &ours, &other);
        assert!(updates.is_empty(), "terminal state must not re-emit");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib loot_history::tests`
Expected: FAIL with "no method named `resolve_pending`".

- [ ] **Step 3: Implement `resolve_pending`**

Inside `impl LootHistory`, add:

```rust
    /// Walk every entry still in `Pending` and advance its state. Returns
    /// `(unit_id, new_state)` transitions so the caller can emit events.
    ///
    /// Inputs (built by the scanner each tick):
    /// - `ground_ids` — uids the scanner saw on `pPaths` this tick.
    /// - `our_inventory_ids` — uids whose `INV_OWNER` resolves back to the
    ///   local hero's `UnitAny` (via `Inventory.pOwner.unit_id`).
    /// - `other_container_ids` — uids whose `INV_OWNER` is non-NULL but
    ///   resolves to *something else* (another player, vendor, corpse).
    ///
    /// Priority of classification (Pending entries only):
    /// 1. In `our_inventory_ids`            → `PickedUp` (terminal)
    /// 2. In `other_container_ids`          → `TakenByOther` (terminal)
    /// 3. In `ground_ids`                   → tick counter reset to 0
    /// 4. Else → counter += 1; if > `LOST_TIMEOUT_TICKS` → `Lost`
    ///
    /// (1) is checked before (2) so a brief moment where the scanner sees
    /// the item in both sets — possible during transient races between
    /// reads — never falsely accuses a teammate.
    pub fn resolve_pending(
        &mut self,
        ground_ids: &HashSet<u32>,
        our_inventory_ids: &HashSet<u32>,
        other_container_ids: &HashSet<u32>,
    ) -> Vec<(u32, PickupState)> {
        let mut updates = Vec::new();

        for entry in self.entries.iter_mut() {
            if entry.pickup != PickupState::Pending {
                continue;
            }

            if our_inventory_ids.contains(&entry.unit_id) {
                entry.pickup = PickupState::PickedUp;
                updates.push((entry.unit_id, PickupState::PickedUp));
                continue;
            }

            if other_container_ids.contains(&entry.unit_id) {
                entry.pickup = PickupState::TakenByOther;
                updates.push((entry.unit_id, PickupState::TakenByOther));
                continue;
            }

            if ground_ids.contains(&entry.unit_id) {
                entry.ticks_since_left_ground = 0;
                continue;
            }

            // Off-ground, in nobody's container we can read: count down.
            entry.ticks_since_left_ground = entry.ticks_since_left_ground.saturating_add(1);
            if entry.ticks_since_left_ground > LOST_TIMEOUT_TICKS {
                entry.pickup = PickupState::Lost;
                updates.push((entry.unit_id, PickupState::Lost));
            }
        }

        updates
    }

    /// Lightweight projection used by the scanner: every still-`Pending`
    /// entry's `(unit_id, cached p_unit_data)`. The scanner re-reads
    /// `INV_OWNER` from these pointers to build the `our_inventory_ids` /
    /// `other_container_ids` sets passed back into `resolve_pending`.
    /// No string clones — just two `u32`s per entry.
    pub fn pending_uids_with_ptrs(&self) -> Vec<(u32, u32)> {
        self.entries
            .iter()
            .filter(|e| e.pickup == PickupState::Pending)
            .map(|e| (e.unit_id, e.p_unit_data))
            .collect()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib loot_history::tests`
Expected: 9 passed (3 from Task 1.2 + 6 new in this task).

- [ ] **Step 5: Commit**

```
git add src-tauri/src/loot_history.rs
git commit -m "feat(loot-history): resolve_pending state machine with grace window"
```

### Task 1.4: Add timestamp helper

**Files:**
- Modify: `src-tauri/src/loot_history.rs`

- [ ] **Step 1: Add timestamp helper**

Append to `src-tauri/src/loot_history.rs` (above `#[cfg(test)]`):

```rust
/// Milliseconds since UNIX epoch. Wall-clock — frontend renders as HH:MM:SS.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 3: Commit**

```
git add src-tauri/src/loot_history.rs
git commit -m "feat(loot-history): add now_ms timestamp helper"
```

---

## Phase 2: Scanner Integration

### Task 2.1: Classify pending pickups via `INV_OWNER` read

**Files:**
- Modify: `src-tauri/src/notifier.rs`

Per-Pending classification (Variant 2, enabled by Phase 0):

```
p_unit_data + 0x5C → Inventory*           (INV_OWNER)
                ↓ (if non-NULL)
            Inventory + 0x08 → UnitAny*    (inventory::OWNER)
                                  ↓
                              UnitAny + 0x0C → unit_id
```

Compare `unit_id` to the local hero's `unit_id` (read once per tick). Equal
→ `our_inventory_ids`; not equal → `other_container_ids`. NULL `INV_OWNER`
→ skip (state machine resolves via `ground_ids` / timeout). All read
failures are treated as "not classifiable this tick" — the entry stays
Pending and will eventually time out to `Lost`.

Required offsets (all already in `offsets.rs`):
- `d2client::PLAYER_UNIT = 0x11BBFC`
- `unit::UNIT_ID = 0x0C`
- `item_data::INV_OWNER = 0x5C` ← Phase 0
- `inventory::OWNER = 0x08` ← Phase 0

- [ ] **Step 1: Add the classifier helper inside `impl DropScanner`**

Find the `impl DropScanner` block in `src-tauri/src/notifier.rs` (around line 173). At the end of the impl (just before `/// Strip D2 color codes...` at line 1071), insert:

```rust
    /// For each `(unit_id, p_unit_data)` pair still in `Pending`, classify
    /// it as belonging to our hero, to someone else, or unknown — by
    /// reading the cached `INV_OWNER` slot. Returns
    /// `(our_inventory_ids, other_container_ids)` ready to feed into
    /// `LootHistory::resolve_pending`.
    ///
    /// All reads are best-effort: any `Err` (item slot freed, race) just
    /// means "not classifiable this tick" and the entry stays Pending —
    /// the state machine's `LOST_TIMEOUT_TICKS` handles permanent loss.
    fn classify_pending_pickups(
        &self,
        pending: &[(u32, u32)],
    ) -> (HashSet<u32>, HashSet<u32>) {
        let mut ours = HashSet::new();
        let mut other = HashSet::new();
        if pending.is_empty() {
            return (ours, other);
        }

        // Resolve local player's unit_id once per tick.
        let player_unit_ptr_addr = self.ctx.d2_client + d2client::PLAYER_UNIT;
        let player_ptr = match self.ctx.process.read_memory::<u32>(player_unit_ptr_addr) {
            Ok(p) if p != 0 => p as usize,
            _ => return (ours, other), // Not in game — can't classify.
        };
        let our_uid = match self
            .ctx
            .process
            .read_memory::<u32>(player_ptr + unit::UNIT_ID)
        {
            Ok(v) => v,
            Err(_) => return (ours, other),
        };

        for &(uid, p_unit_data) in pending {
            if p_unit_data == 0 {
                continue;
            }

            // ItemData + 0x5C → Inventory*.
            let inv_ptr = match self.ctx.process.read_memory::<u32>(
                p_unit_data as usize + crate::offsets::item_data::INV_OWNER,
            ) {
                Ok(p) => p,
                Err(_) => continue, // Slot freed; let timeout fire.
            };
            if inv_ptr == 0 {
                continue; // On the ground (or freed) — handled elsewhere.
            }

            // Inventory + 0x08 → owner UnitAny*.
            let owner_unit = match self.ctx.process.read_memory::<u32>(
                inv_ptr as usize + crate::offsets::inventory::OWNER,
            ) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };

            // UnitAny + 0x0C → unit_id.
            let owner_uid = match self
                .ctx
                .process
                .read_memory::<u32>(owner_unit + unit::UNIT_ID)
            {
                Ok(v) => v,
                Err(_) => continue,
            };

            if owner_uid == our_uid {
                ours.insert(uid);
            } else {
                other.insert(uid);
            }
        }

        (ours, other)
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success. (Function is currently unused — that's fine for this step.)

- [ ] **Step 3: Commit**

```
git add src-tauri/src/notifier.rs
git commit -m "feat(loot-history): classify pending pickups via INV_OWNER read"
```

### Task 2.2: Wire `LootHistory` into `DropScanner` and push entries

**Files:**
- Modify: `src-tauri/src/notifier.rs`

- [ ] **Step 1: Add `LootHistory` field to `DropScanner` and accept a shared handle**

In `notifier.rs`, find the `DropScanner` struct (line ~61). Add a field:

```rust
    /// Session loot history. Shared with main thread so Tauri commands can
    /// snapshot it. Updated each tick.
    loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
```

Update `DropScanner::new()` (line ~175) to construct it. Change the signature to accept a shared handle:

```rust
    pub fn new(loot_history: Arc<RwLock<crate::loot_history::LootHistory>>) -> Result<Self, String> {
        let ctx = D2Context::new()?;
        let injector = D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common, ctx.d2_lang)?;

        let mut loot_hook = LootFilterHook::new();
        if ctx.d2_sigma != 0 {
            if let Err(e) = loot_hook.inject(&ctx) {
                log_error(&format!("Failed to inject LootFilterHook: {}", e));
            }
        }

        Ok(Self {
            ctx,
            injector,
            seen_items: HashSet::new(),
            filter_config: None,
            filter_enabled: false,
            verbose_filter_logging: false,
            loot_hook,
            class_cache: None,
            unique_cache: None,
            set_cache: None,
            map_marker: MapMarkerManager::new(),
            recent_events: HashMap::new(),
            loot_history,
        })
    }
```

- [ ] **Step 2: Push entries at the same point we emit `item-drop`**

In `tick_items` (around line 533–536, where `events.push(event)` lives), modify the block so that when an event is emitted we also push to history:

```rust
                    // Cache enriched event for the map-marker pass.
                    self.recent_events.insert(event.unit_id, event.clone());

                    if should_emit {
                        // Push to session history (only filter-matched items
                        // — same gate as overlay notifications).
                        if event.filter.is_some() {
                            let color = event
                                .filter
                                .as_ref()
                                .and_then(|n| n.color.clone());
                            let entry = crate::loot_history::LootEntry {
                                unit_id: event.unit_id,
                                timestamp_ms: crate::loot_history::now_ms(),
                                name: event.name.clone(),
                                color,
                                pickup: crate::loot_history::PickupState::Pending,
                                ticks_since_left_ground: 0,
                                // Cache the pUnitData pointer so the
                                // classifier can re-read INV_OWNER on every
                                // subsequent tick without re-scanning pPaths.
                                p_unit_data: event.p_unit_data,
                            };
                            if let Ok(mut hist) = self.loot_history.write() {
                                hist.push(entry);
                            }
                        }
                        events.push(event);
                    }
```

(Replace ONLY the existing `if should_emit { events.push(event); }` block — the surrounding lines above are already there.)

Notes for the implementer:
- `Notification` struct (`event.filter`) lives in `rules` module. Check that it has a `color: Option<String>` field by reading `src-tauri/src/rules/mod.rs` — if the field name differs (e.g., `color_str`, `color_keyword`), match the actual name.
- The event payload must already carry the item's `p_unit_data` (it does on `ScannedItem` in `d2types.rs`). If `ItemDropEvent` doesn't surface it, plumb it through — required by the classifier.

- [ ] **Step 3: Resolve pending pickups at end of `tick_items`**

Append this block right before `events` is returned (just before the `events` line at end of `tick_items`, after `seen_items.retain(...)` and `recent_events.retain(...)`):

```rust
        // Pickup resolution: classify each pending entry via INV_OWNER,
        // then advance the state machine. `current_item_ids` is the set
        // of uids visible on pPaths this tick (already built earlier).
        let pending_pairs = self
            .loot_history
            .read()
            .map(|h| h.pending_uids_with_ptrs())
            .unwrap_or_default();
        let (our_ids, other_ids) = self.classify_pending_pickups(&pending_pairs);
        if let Ok(mut hist) = self.loot_history.write() {
            self.last_pickup_updates =
                hist.resolve_pending(&current_item_ids, &our_ids, &other_ids);
        } else {
            self.last_pickup_updates.clear();
        }
```

For this we need a new field on the scanner. Add it to the struct:

```rust
    /// Pickup-state transitions produced by the latest `tick_items` call.
    /// Drained by main loop into `loot-history-update` events.
    last_pickup_updates: Vec<(u32, crate::loot_history::PickupState)>,
```

And initialize in `new()`:

```rust
            last_pickup_updates: Vec::new(),
```

Add a public drain method on the impl:

```rust
    /// Take the pickup updates produced by the latest `tick_items` call.
    pub fn drain_pickup_updates(&mut self) -> Vec<(u32, crate::loot_history::PickupState)> {
        std::mem::take(&mut self.last_pickup_updates)
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: errors at the call sites of `DropScanner::new()` in `main.rs` (signature changed). Fix in next task.

- [ ] **Step 5: Update non-Windows stub**

In the bottom of `notifier.rs` find `#[cfg(not(target_os = "windows"))]` block (around line 1116). Update the stub `DropScanner::new` signature to match:

```rust
    pub fn new(_loot_history: Arc<RwLock<crate::loot_history::LootHistory>>) -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
```

And add a no-op stub:

```rust
    pub fn drain_pickup_updates(&mut self) -> Vec<(u32, crate::loot_history::PickupState)> {
        Vec::new()
    }
```

- [ ] **Step 6: Commit**

```
git add src-tauri/src/notifier.rs
git commit -m "feat(loot-history): push entries and resolve pickups in scanner tick"
```

### Task 2.3: Wire `LootHistory` shared handle into `main.rs`

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add field to `AppState`**

Find the `struct AppState` block in `main.rs` (around line 64). Append:

```rust
    /// Session loot history shared with scanner thread.
    loot_history: Arc<RwLock<loot_history::LootHistory>>,
```

- [ ] **Step 2: Initialize in app setup**

Find the place in `main.rs` `tauri::Builder` setup where `AppState` is constructed. Initialize:

```rust
let loot_history: Arc<RwLock<loot_history::LootHistory>> =
    Arc::new(RwLock::new(loot_history::LootHistory::new()));
```

Pass it into `AppState`:

```rust
loot_history: loot_history.clone(),
```

- [ ] **Step 3: Update `start_scanner_internal` and `spawn_auto_scanner` signatures to thread it through**

Both functions currently take `is_scanning`, `filter_config`, etc. Add `loot_history: Arc<RwLock<loot_history::LootHistory>>` to each. Pass through.

In the scanner thread closure inside `start_scanner_internal`, replace the existing:

```rust
let mut scanner = match DropScanner::new() {
```

with:

```rust
let mut scanner = match DropScanner::new(loot_history.clone()) {
```

- [ ] **Step 4: Add `use` for the module**

At the top of `main.rs`, add:

```rust
use loot_history::{LootEntry, LootHistory, PickupState};
```

(`LootEntry` and `PickupState` are needed by Tauri commands and event payloads in Phase 3.)

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 6: Commit**

```
git add src-tauri/src/main.rs
git commit -m "feat(loot-history): wire shared LootHistory handle through scanner spawn"
```

### Task 2.4: Clear history on `menu → ingame` transition

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Locate the existing transition handler**

In `main.rs`, in the scanner thread main loop, find the existing block (around line 209-225):

```rust
                if ingame && !was_ingame {
                    log_info("Entered game");
                    scanner.clear_cache();
                    pending_set_always_show = true;
                    last_emitted_always_show = None;
                    if let Err(e) = app_handle.emit("game-status", "ingame") {
                        ...
                    }
```

- [ ] **Step 2: Add history clear and emit**

Inside the `if ingame && !was_ingame` block, after `scanner.clear_cache();`, add:

```rust
                    if let Ok(mut hist) = loot_history.write() {
                        hist.clear();
                    }
                    if let Err(e) = app_handle.emit("loot-history-cleared", ()) {
                        log_error(&format!("Failed to emit loot-history-cleared: {}", e));
                    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 4: Commit**

```
git add src-tauri/src/main.rs
git commit -m "feat(loot-history): clear session on menu->ingame transition"
```

---

## Phase 3: Tauri Commands and Event Emit

### Task 3.1: Add `get_loot_history` and `clear_loot_history` commands

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add command functions**

Append to `main.rs` (in the same area as `get_game_status` etc., around line 405):

```rust
#[tauri::command]
fn get_loot_history(state: tauri::State<AppState>) -> Vec<LootEntry> {
    state
        .loot_history
        .read()
        .map(|h| h.snapshot())
        .unwrap_or_default()
}

#[tauri::command]
fn clear_loot_history(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if let Ok(mut h) = state.loot_history.write() {
        h.clear();
    }
    app_handle
        .emit("loot-history-cleared", ())
        .map_err(|e| format!("Failed to emit loot-history-cleared: {}", e))
}
```

- [ ] **Step 2: Register in `invoke_handler`**

Find the `tauri::Builder::invoke_handler!` call in `main.rs`. Add `get_loot_history` and `clear_loot_history` to the macro's argument list, alphabetically.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 4: Commit**

```
git add src-tauri/src/main.rs
git commit -m "feat(loot-history): add get_loot_history and clear_loot_history commands"
```

### Task 3.2: Emit `loot-history-entry` and `loot-history-update` from scanner thread

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Snapshot recently-pushed entries inside the scanner loop and emit them**

In `main.rs` scanner thread, find the block where `item-drop` events are emitted (around line 303-308):

```rust
                    let items = scanner.tick_items();
                    for item in items {
                        if let Err(e) = app_handle.emit("item-drop", &item) {
                            log_error(&format!("Failed to emit item-drop event: {}", e));
                        }
                    }
                    scanner.tick_map_markers();
```

Replace with:

```rust
                    let items = scanner.tick_items();
                    for item in items {
                        // Mirror the gate the scanner uses to push history.
                        if item.filter.is_some() {
                            // Re-derive the entry payload — a snapshot read
                            // would race with another tick. Use the same
                            // fields the scanner pushed.
                            #[derive(serde::Serialize, Clone)]
                            struct LootHistoryEntryPayload<'a> {
                                unit_id: u32,
                                timestamp_ms: u64,
                                name: &'a str,
                                color: Option<&'a str>,
                                pickup: PickupState,
                            }
                            let color = item
                                .filter
                                .as_ref()
                                .and_then(|n| n.color.as_deref());
                            // Read the just-pushed entry's timestamp from history.
                            let timestamp_ms = state_ref
                                .loot_history
                                .read()
                                .ok()
                                .and_then(|h| {
                                    h.snapshot()
                                        .iter()
                                        .find(|e| e.unit_id == item.unit_id)
                                        .map(|e| e.timestamp_ms)
                                })
                                .unwrap_or(0);
                            let payload = LootHistoryEntryPayload {
                                unit_id: item.unit_id,
                                timestamp_ms,
                                name: &item.name,
                                color,
                                pickup: PickupState::Pending,
                            };
                            if let Err(e) = app_handle.emit("loot-history-entry", &payload) {
                                log_error(&format!(
                                    "Failed to emit loot-history-entry: {}",
                                    e
                                ));
                            }
                        }
                        if let Err(e) = app_handle.emit("item-drop", &item) {
                            log_error(&format!("Failed to emit item-drop event: {}", e));
                        }
                    }

                    // Drain pickup-state transitions and broadcast them.
                    for (unit_id, pickup) in scanner.drain_pickup_updates() {
                        #[derive(serde::Serialize)]
                        struct LootHistoryUpdatePayload {
                            unit_id: u32,
                            pickup: PickupState,
                        }
                        let payload = LootHistoryUpdatePayload { unit_id, pickup };
                        if let Err(e) = app_handle.emit("loot-history-update", &payload) {
                            log_error(&format!(
                                "Failed to emit loot-history-update: {}",
                                e
                            ));
                        }
                    }

                    scanner.tick_map_markers();
```

`state_ref` is a placeholder — we need the scanner thread to have access to `loot_history`. It already does (Task 2.3 passed it in). Use the local `loot_history` clone instead of `state_ref.loot_history`. Adjust:

```rust
                            let timestamp_ms = loot_history
                                .read()
                                .ok()
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 3: Run full backend test suite**

Run: `cd src-tauri && cargo test`
Expected: all `loot_history::tests` pass; existing tests in `rules::*` and `map_marker::*` still pass.

- [ ] **Step 4: Commit**

```
git add src-tauri/src/main.rs
git commit -m "feat(loot-history): emit entry and update events from scanner thread"
```

---

## Phase 4: Frontend Store and Panel

### Task 4.1: Create `loot-history.svelte.ts` store

**Files:**
- Create: `src/stores/loot-history.svelte.ts`
- Modify: `src/stores/index.ts`

- [ ] **Step 1: Read existing store for pattern reference**

Read `src/stores/items-dictionary.svelte.ts` so the new store follows the same shape (import from `@tauri-apps/api/event`, expose state via `$state`).

- [ ] **Step 2: Create the store**

Write `src/stores/loot-history.svelte.ts`:

```typescript
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type PickupState = 'pending' | 'picked_up' | 'taken_by_other' | 'lost';

export interface LootHistoryEntry {
  unit_id: number;
  timestamp_ms: number;
  name: string;
  color: string | null;
  pickup: PickupState;
}

interface LootHistoryUpdate {
  unit_id: number;
  pickup: PickupState;
}

class LootHistoryStore {
  // Insertion-ordered list of entries; renderable directly.
  entries = $state<LootHistoryEntry[]>([]);
  // Index for O(1) updates.
  #indexByUnitId = new Map<number, number>();
  #unlisteners: UnlistenFn[] = [];
  #initialized = false;

  async initialize(): Promise<void> {
    if (this.#initialized) return;
    this.#initialized = true;

    const initial = await invoke<LootHistoryEntry[]>('get_loot_history');
    this.#replaceAll(initial);

    this.#unlisteners.push(
      await listen<LootHistoryEntry>('loot-history-entry', (event) => {
        this.#append(event.payload);
      }),
      await listen<LootHistoryUpdate>('loot-history-update', (event) => {
        this.#applyUpdate(event.payload);
      }),
      await listen<null>('loot-history-cleared', () => {
        this.#replaceAll([]);
      }),
    );
  }

  destroy(): void {
    for (const u of this.#unlisteners) u();
    this.#unlisteners = [];
    this.#initialized = false;
  }

  async clear(): Promise<void> {
    await invoke('clear_loot_history');
  }

  #replaceAll(items: LootHistoryEntry[]) {
    this.entries = items;
    this.#indexByUnitId.clear();
    items.forEach((e, i) => this.#indexByUnitId.set(e.unit_id, i));
  }

  #append(entry: LootHistoryEntry) {
    if (this.#indexByUnitId.has(entry.unit_id)) return;
    this.#indexByUnitId.set(entry.unit_id, this.entries.length);
    this.entries = [...this.entries, entry];
  }

  #applyUpdate(update: LootHistoryUpdate) {
    const idx = this.#indexByUnitId.get(update.unit_id);
    if (idx === undefined) return;
    const next = this.entries.slice();
    next[idx] = { ...next[idx], pickup: update.pickup };
    this.entries = next;
  }
}

export const lootHistoryStore = new LootHistoryStore();
```

- [ ] **Step 3: Re-export from `src/stores/index.ts`**

Open `src/stores/index.ts` and add:

```typescript
export { lootHistoryStore, type LootHistoryEntry, type PickupState } from './loot-history.svelte';
```

- [ ] **Step 4: Verify it compiles (TypeScript)**

Run: `pnpm tsc --noEmit -p tsconfig.json`

If `tsconfig.json` doesn't expose a script, instead trigger Vite type-check via dev server boot (next phase).

Expected: no TS errors related to loot-history.

- [ ] **Step 5: Commit**

```
git add src/stores/loot-history.svelte.ts src/stores/index.ts
git commit -m "feat(loot-history): add Svelte store with event subscriptions"
```

### Task 4.2: Create `LootHistoryPanel.svelte`

**Files:**
- Create: `src/components/LootHistoryPanel.svelte`
- Modify: `src/components/index.ts`

- [ ] **Step 1: Create the component**

Write `src/components/LootHistoryPanel.svelte`:

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { lootHistoryStore, type LootHistoryEntry } from '../stores';

  let { onClose } = $props<{ onClose: () => void }>();

  let scrollContainer: HTMLDivElement | null = $state(null);
  let stickToBottom = $state(true);

  function formatTime(ms: number): string {
    const d = new Date(ms);
    const hh = d.getHours().toString().padStart(2, '0');
    const mm = d.getMinutes().toString().padStart(2, '0');
    const ss = d.getSeconds().toString().padStart(2, '0');
    return `${hh}:${mm}:${ss}`;
  }

  function pickupIcon(state: LootHistoryEntry['pickup']): string {
    switch (state) {
      case 'picked_up': return '✓';
      case 'taken_by_other': return '→';
      case 'lost': return '⊘';
      case 'pending': return '⏳';
    }
  }

  function pickupClass(state: LootHistoryEntry['pickup']): string {
    return `pickup pickup-${state}`;
  }

  function nameColor(entry: LootHistoryEntry): string {
    return entry.color ?? 'var(--text-primary)';
  }

  function onScroll() {
    if (!scrollContainer) return;
    const el = scrollContainer;
    stickToBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 50;
  }

  // Auto-scroll to bottom only when the user is already near the bottom.
  $effect(() => {
    void lootHistoryStore.entries.length;
    if (stickToBottom && scrollContainer) {
      queueMicrotask(() => {
        if (scrollContainer) {
          scrollContainer.scrollTop = scrollContainer.scrollHeight;
        }
      });
    }
  });

  onMount(() => {
    void lootHistoryStore.initialize();
  });
</script>

<div class="loot-history-panel" role="dialog" aria-label="Loot history">
  <header>
    <h2>Loot History</h2>
    <button type="button" class="close" onclick={onClose} aria-label="Close">×</button>
  </header>
  <div
    class="list"
    bind:this={scrollContainer}
    onscroll={onScroll}
  >
    {#each lootHistoryStore.entries as entry (entry.unit_id)}
      <div class="row">
        <span class="time">[{formatTime(entry.timestamp_ms)}]</span>
        <span class={pickupClass(entry.pickup)}>{pickupIcon(entry.pickup)}</span>
        <span class="name" style:color={nameColor(entry)}>{entry.name}</span>
      </div>
    {/each}
    {#if lootHistoryStore.entries.length === 0}
      <div class="empty">No drops in this session yet.</div>
    {/if}
  </div>
</div>

<style>
  .loot-history-panel {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    max-width: min(700px, 60vw);
    width: 100%;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    background: rgba(0, 0, 0, 0.85);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.2));
    border-radius: var(--radius-md, 8px);
    color: var(--text-primary, #fff);
    pointer-events: auto;
    font-family: var(--font-mono, monospace);
    font-size: 13px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .close {
    background: transparent;
    border: none;
    color: inherit;
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .close:hover { color: #f88; }
  .list {
    overflow-y: auto;
    padding: 6px 12px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    gap: 8px;
  }
  .time { color: rgba(255, 255, 255, 0.5); }
  .pickup { width: 1em; text-align: center; }
  .pickup-picked_up { color: #5cd66a; }
  .pickup-taken_by_other { color: #6cabf0; }  /* neutral blue: someone else has it */
  .pickup-lost { color: rgba(255, 255, 255, 0.4); }
  .pickup-pending { color: #f0b400; }
  .name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty { padding: 16px; text-align: center; color: rgba(255, 255, 255, 0.4); }
</style>
```

- [ ] **Step 2: Re-export from `src/components/index.ts`**

Open `src/components/index.ts` and add:

```typescript
export { default as LootHistoryPanel } from './LootHistoryPanel.svelte';
```

- [ ] **Step 3: Commit**

```
git add src/components/LootHistoryPanel.svelte src/components/index.ts
git commit -m "feat(loot-history): add overlay panel component"
```

### Task 4.3: Embed panel into `OverlayWindow.svelte`

**Files:**
- Modify: `src/views/OverlayWindow.svelte`

- [ ] **Step 1: Add toggle state and event listener**

In `src/views/OverlayWindow.svelte`, modify the `<script lang="ts">` block:

After the existing imports add:

```typescript
import { LootHistoryPanel } from '../components';
```

Inside the existing `let editActive = $state(false);` area add:

```typescript
let historyVisible = $state(false);
```

Inside the `onMount` callback, after the existing edit-mode listener, add:

```typescript
listen<{ visible?: boolean }>('toggle-loot-history', async (event) => {
  const next = event.payload?.visible ?? !historyVisible;
  if (next === historyVisible) return;
  historyVisible = next;
  try {
    await invoke('set_overlay_interactive', { active: historyVisible || editActive });
  } catch (err) {
    console.error('[Overlay] set_overlay_interactive (history) failed:', err);
  }
}).then(u => unlisteners.push(u));
```

- [ ] **Step 2: Render panel inside `<main class="overlay">`**

Inside the existing `<main class="overlay">…</main>` block, alongside the `NotificationStack`:

```svelte
  {#if historyVisible}
    <LootHistoryPanel onClose={() => {
      historyVisible = false;
      invoke('set_overlay_interactive', { active: editActive }).catch(() => {});
    }} />
  {/if}
```

- [ ] **Step 3: Manual smoke test**

Run: `pnpm tauri dev`

In the dev console of the overlay window (`Ctrl+Shift+I` if devtools are enabled, or trigger via main window if there's a debug button), simulate the event:

```js
window.__TAURI__.event.emit('toggle-loot-history', { visible: true });
```

Expected: panel appears centered. Empty state message visible if no drops yet.

Close via `×` button. Expected: panel disappears.

- [ ] **Step 4: Commit**

```
git add src/views/OverlayWindow.svelte
git commit -m "feat(loot-history): embed panel in overlay with toggle event"
```

---

## Phase 5: Hotkey Integration

### Task 5.1: Add `loot_history_hotkey` to settings

**Files:**
- Modify: `src-tauri/src/settings.rs`

- [ ] **Step 1: Add the field**

In `AppSettings` struct (around line 18-78), append:

```rust
    /// Hotkey to toggle the in-game loot history overlay panel.
    #[serde(default = "default_loot_history_hotkey")]
    pub loot_history_hotkey: HotkeyConfig,
```

- [ ] **Step 2: Add the default function**

After `default_reveal_hidden_hotkey` (around line 136), add:

```rust
fn default_loot_history_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x4E, // 'N'
        modifiers: 0,
        display: "N".to_string(),
    }
}
```

- [ ] **Step 3: Add to `Default for AppSettings`**

In the `impl Default for AppSettings` block (around line 144), inside the struct literal, add the field initializer:

```rust
            loot_history_hotkey: default_loot_history_hotkey(),
```

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 5: Commit**

```
git add src-tauri/src/settings.rs
git commit -m "feat(loot-history): add settings field for toggle hotkey (default N)"
```

### Task 5.2: Add a hotkey watcher for loot history

**Files:**
- Modify: `src-tauri/src/hotkeys.rs`
- Modify: `src-tauri/src/main.rs`

The existing `EditModeState` and `RevealHiddenState` watch a key state and emit events when held/pressed. Loot-history is a toggle (press to flip), not held. Model on `RevealHiddenState`'s key-state polling but emit only on rising-edge press.

- [ ] **Step 1: Read existing hotkeys.rs to understand the watcher pattern**

Read `src-tauri/src/hotkeys.rs` from line 100 onwards to see how `EditModeState` and `RevealHiddenState` are constructed and started.

- [ ] **Step 2: Add `LootHistoryHotkeyState`**

Append to `hotkeys.rs`:

```rust
/// Watcher for the "toggle loot history" hotkey. Polls the configured key
/// every ~30ms and emits `toggle-loot-history` to the overlay webview on
/// rising-edge press. Toggle semantics — frontend manages visibility state.
pub struct LootHistoryHotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
}

impl LootHistoryHotkeyState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(HotkeyConfig::default())),
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            self.stop();
            thread::sleep(std::time::Duration::from_millis(50));
        }
        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }
        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();

        #[cfg(target_os = "windows")]
        thread::spawn(move || {
            let mut prev_down = false;
            while is_running.load(Ordering::SeqCst) {
                let cfg = current_hotkey.lock().map(|g| g.clone()).ok();
                if let Some(cfg) = cfg {
                    if cfg.key_code != 0 {
                        let key_down = unsafe {
                            (GetAsyncKeyState(cfg.key_code as i32) as u16 & 0x8000) != 0
                        };
                        // Modifier check: require ALL configured modifiers
                        // to be currently held (or none if cfg.modifiers==0).
                        let mods_ok = check_modifiers(cfg.modifiers);
                        let active = key_down && mods_ok;
                        if active && !prev_down {
                            if let Err(e) = app_handle.emit("toggle-loot-history", ()) {
                                log_error(&format!(
                                    "Failed to emit toggle-loot-history: {}",
                                    e
                                ));
                            }
                        }
                        prev_down = active;
                    }
                }
                thread::sleep(std::time::Duration::from_millis(30));
            }
        });

        #[cfg(not(target_os = "windows"))]
        log_info("Loot-history hotkey is only supported on Windows");
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn check_modifiers(mods: u32) -> bool {
    // Bit definitions match HOT_KEY_MODIFIERS values used by RegisterHotKey.
    const MOD_ALT: u32 = 0x0001;
    const MOD_CTRL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;

    let alt_held = unsafe { (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 };
    let ctrl_held = unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
    let shift_held = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
    let win_held = unsafe {
        (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
    };

    let want = |bit: u32, held: bool| if (mods & bit) != 0 { held } else { !held };
    want(MOD_ALT, alt_held)
        && want(MOD_CTRL, ctrl_held)
        && want(MOD_SHIFT, shift_held)
        && want(MOD_WIN, win_held)
}
```

If `check_modifiers` already exists in `hotkeys.rs` for another watcher, reuse it instead of duplicating. Read the existing implementations of `EditModeState`/`RevealHiddenState` and align style.

- [ ] **Step 3: Wire into `main.rs` AppState and startup**

In `main.rs`:

Add to imports:

```rust
use crate::hotkeys::{EditModeState, HotkeyState, LootHistoryHotkeyState, RevealHiddenState};
```

Add to `AppState`:

```rust
    loot_history_hotkey: Arc<LootHistoryHotkeyState>,
```

In setup, construct it and call `start()` after settings load (mirror what's done for `RevealHiddenState`):

```rust
let loot_history_hotkey = Arc::new(LootHistoryHotkeyState::new());
loot_history_hotkey.start(app_handle.clone(), settings.loot_history_hotkey.clone());
```

Pass `loot_history_hotkey: loot_history_hotkey.clone()` into `AppState`.

- [ ] **Step 4: React to settings changes**

The existing `save_settings` command emits `settings-updated`. Find the listener that re-applies hotkey configs (search for `RevealHiddenState` or `edit_overlay_hotkey` to locate it). Add a re-start of the loot-history hotkey when its settings change:

```rust
state.loot_history_hotkey.start(
    app_handle.clone(),
    new_settings.loot_history_hotkey.clone(),
);
```

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: success.

- [ ] **Step 6: Commit**

```
git add src-tauri/src/hotkeys.rs src-tauri/src/main.rs
git commit -m "feat(loot-history): hotkey watcher emits toggle-loot-history on press"
```

### Task 5.3: Add hotkey-binding row to General tab

**Files:**
- Modify: `src/views/GeneralTab.svelte`

- [ ] **Step 1: Read existing hotkey-binding row**

Open `src/views/GeneralTab.svelte` and locate the section for `edit_overlay_hotkey` or `reveal_hidden_hotkey`. The component used for hotkey rebinding is reusable; identify its tag (e.g., `<HotkeyInput>` or `<HotkeyBinding>`).

- [ ] **Step 2: Add a new section "Loot History"**

Add a new section after the existing hotkey rows:

```svelte
<section class="setting-section">
  <h3>Loot History</h3>
  <div class="setting-row">
    <label for="loot-history-hotkey">Toggle hotkey</label>
    <HotkeyInput
      id="loot-history-hotkey"
      value={settings.loot_history_hotkey}
      onchange={(hk) => updateSetting('loot_history_hotkey', hk)}
    />
    <span class="hint">Press to toggle the in-game loot log overlay.</span>
  </div>
</section>
```

Replace the component name, settings accessor, and update fn names with whatever the existing rows use — match the file's conventions exactly.

- [ ] **Step 3: Manual smoke test**

Run: `pnpm tauri dev`

- Open main window → General tab → confirm the new "Loot History" section appears with `N` displayed.
- Rebind to `Shift+L`, save.
- Restart the app, confirm `Shift+L` is preserved.

- [ ] **Step 4: Commit**

```
git add src/views/GeneralTab.svelte
git commit -m "feat(loot-history): add hotkey rebind row to General tab"
```

---

## Phase 6: Manual QA

These cannot be automated — they require a running D2 + MXL.

### Task 6.1: SP smoke test

- [ ] **Step 1: Run a normal-difficulty area with notifies enabled**

Run a normal-difficulty Cow Level or Tristram run. Filter should be configured to `notify` on at least uniques and rares.

Expected:
- Each filter-matched drop appears in the panel as `⏳ Pending` immediately, in chronological order.
- Picked-up items transition to `✓ PickedUp` within ~150ms of the pickup animation finishing.
- Items left on the ground that despawn (move to a different area, wait it out) transition to `⊘ Lost` within seconds of leaving the area.

If any drop fails to transition, check `d2mxlutils.log` for `Failed to emit loot-history-*` errors.

### Task 6.2: MP false-positive check

Requires a second D2 client (LAN or co-op).

- [ ] **Step 1: Drop two items, pick up one**

Have player A run an area, both players see the drops. Player B (us) picks up item 1; player A picks up item 2.

Expected on player B's history:
- Item 1 → `✓ PickedUp`
- Item 2 → `→ TakenByOther` (NEVER `PickedUp`; should NOT settle as `Lost`
  unless player A drops it back on the ground for long enough to time out).

If item 2 ever shows `PickedUp`, the `INV_OWNER` classifier is comparing
unit_ids incorrectly — investigate before shipping. If item 2 settles as
`Lost` instead of `TakenByOther`, the dereference chain
(`INV_OWNER → Inventory.pOwner → unit_id`) is failing — likely a stale
pointer or wrong offset; double-check `inventory::OWNER` is `0x08`.

### Task 6.3: Stress / cap

- [ ] **Step 1: Spam 350+ drops in one session**

Use `/players 8` and farm a high-density area until 300+ entries are pushed.

Expected:
- After 300 entries, oldest entries are evicted. Panel scrollback is bounded.
- Frame rate of overlay stays smooth (no Svelte rendering lag).
- No memory growth in `d2mxlutils.exe` Task Manager view between minute 1 and minute 10 of farming.

### Task 6.4: Session lifecycle

- [ ] **Step 1: Test transitions**

- Town ↔ area transition (Waypoint, TP) — history persists, no clear.
- Exit to character select, re-enter same character — history clears at `menu→ingame`.
- Quit MXL entirely, restart app — history is empty.

### Task 6.5: Hotkey conflict and rapid toggle

- [ ] **Step 1: Edge tests**

- Rebind hotkey to a key already used by D2 (e.g., `S` for stamina) — verify settings layer warns about conflict (existing behavior).
- Spam-press the toggle hotkey for 5 seconds. Expected: panel toggles cleanly, no leak in `set_overlay_interactive` calls (game still receives clicks correctly when the panel is closed).

---

## Phase 7 (Fallback): Inventory-Walk Plan B — only if `INV_OWNER` proves unreliable in production

> **Status:** Not needed by default. Phase 0 found `INV_OWNER` and Phase 2
> uses it as the primary classifier. This phase exists as a documented
> Plan B if Phase 6 manual QA reveals systemic misclassification (e.g.,
> stale `pUnitData` pointers cause many `Lost` entries that should be
> `PickedUp`, or `TakenByOther` mis-fires for own-corpse pickups).

**Files (only if invoked):**
- Modify: `src-tauri/src/notifier.rs` — re-add `read_inventory_unit_ids()`
  helper (player UnitAny → +0x60 → Inventory* → +0x0C `pFirstItem` → walk
  via `pUnitData + 0x64` chain, capped at 256). Use it to populate
  `our_inventory_ids`.
- Trade-off: walking the player's inventory each tick is a few extra
  `ReadProcessMemory` syscalls but does not require the cached
  `p_unit_data` pointer to remain valid — robust against item-slot
  recycling. Cost: cannot detect `TakenByOther`, so foreign pickups
  collapse to `Lost` after the timeout (acceptable graceful degradation).
- Acceptance: re-run unit tests (`cargo test`) and manual QA tasks
  6.1–6.4. Skip 6.2's `TakenByOther` check; expect item 2 → `Lost`.

If both classifiers turn out usable, prefer `INV_OWNER` for the richer
state set; this phase is purely a safety net.

---

## Self-Review Notes

**Spec coverage check:**
- Lifetime (session-only, clear on `menu → ingame`): Tasks 2.4, 1.1 (no persistence layer touched). ✓
- Window placement (centered, semi-transparent, reuse overlay): Task 4.2 + Task 4.3. ✓
- Hotkey N default + General-tab rebind: Tasks 5.1, 5.2, 5.3. ✓
- Items captured = `notify == Some`: Task 2.2 Step 2. ✓
- Pickup scope = ours vs others vs lost via `INV_OWNER` read: Task 2.1. ✓
- Per-entry display (time + icon + name colored): Task 4.2 component + Task 4.1 store payload. ✓
- Cap 300 FIFO: Task 1.2 + tests. ✓
- 4-state pickup machine with grace window: Task 1.3 + tests (9 cases). ✓
- Phase 0 reverse engineering pre-gate: Phase 0 entire — DONE, offset 0x5C / 0x08 confirmed. ✓
- Phase 7 fallback to inventory walk: covered as Plan B only. ✓

**Type consistency:**
- `LootEntry` field names (`unit_id`, `timestamp_ms`, `name`, `color`, `pickup`) used identically across Rust struct (1.1), payload struct (3.2), TypeScript interface (4.1). The Rust-only `p_unit_data` is `#[serde(skip)]` — never reaches TS. ✓
- `PickupState` variants serialize as snake_case (`pending`, `picked_up`, `taken_by_other`, `lost`) via `#[serde(rename_all = "snake_case")]`; TS union `'pending' | 'picked_up' | 'taken_by_other' | 'lost'` matches. ✓
- `loot_history_hotkey` field consistent across `settings.rs` (5.1), `main.rs` startup (5.2), and frontend updateSetting call (5.3). ✓

**Required offsets (all in `src-tauri/src/offsets.rs`):**
- `d2client::PLAYER_UNIT = 0x11BBFC` — local hero ptr-of-ptr.
- `unit::UNIT_ID = 0x0C` — `UnitAny.unit_id`.
- `unit::UNIT_DATA = 0x14` — `UnitAny.pUnitData`.
- `item_data::INV_OWNER = 0x5C` — Phase 0; `Inventory*` back-pointer.
- `inventory::OWNER = 0x08` — Phase 0; `UnitAny* pOwner` of an Inventory.
- (Phase 7 fallback only) `unit::INVENTORY = 0x60`,
  `inventory::FIRST_ITEM = 0x0C`, `item_data::NEXT_ITEM = 0x64`.

**Open assumptions documented:**
- `Notification.color` field (referenced in 2.2 Step 2 and 3.2): the implementer must verify the actual field name in `rules/mod.rs` and adjust if different. Called out in Task 2.2 Step 2.
- `ItemDropEvent` must surface `p_unit_data` to the history-push call in 2.2. Already on `ScannedItem`; verify the event type carries it.
- `HotkeyInput` component name in 5.3: must be replaced with the actual component name used by other hotkey rows. Called out in Task 5.3 Step 1.

---

## Phase 8 — Post-shipping refinements (2026-05-01, iterative)

The phase-1..5 plan above shipped, then real-world testing surfaced four
issues. Each was fixed in turn. The shipped pickup-detection logic
documented in the spec reflects this final state.

### 8.1 Inventory-walk added as primary `PickedUp` signal

**Symptom:** Items the user clearly picked up stayed `Pending` (then aged
to `⊘ Lost` via the timeout) instead of flipping to `✓ PickedUp`.

**Root cause:** Phase 2's classifier read `INV_OWNER` from a *cached*
`p_unit_data`. After pickup, D2 may free or repurpose that ItemData
slot, so `INV_OWNER` no longer resolves usefully.

**Fix:** Added `read_player_inventory_ids()` in `notifier.rs` that walks
`PLAYER_UNIT → +0x60 Inventory* → +0x0C pFirstItem → loop UnitAny.unit_id +
pUnitData + 0x64` (the Phase 7 fallback, promoted to primary). Its
result seeds `our_inventory_ids` on every tick from live pointers, robust
against any cached-pointer staleness. `INV_OWNER` is retained for
`TakenByOther` attribution.

### 8.2 `Lost` timeout removed; items stay `Pending` indefinitely

**Symptom:** An item left on the ground turned `⊘ Lost` ~150 ms after the
player walked or teleported away, even though the item was still
physically there.

**Root cause:** `LOST_TIMEOUT_TICKS = 5` (~150 ms) on absence-from-`pPaths`
fired on every area transition.

**Fix:** Deleted the timeout entirely. `resolve_pending` no longer
transitions `Pending → Lost` on absence. Items stay `Pending` unless
positively classified as `PickedUp`/`TakenByOther`, OR the slot-freed
check (8.4) fires, OR session ends (8.5).

### 8.3 Seed-based dedup across area unload/reload

**Symptom:** Tеleporting away and returning produced a *second* row for
the same physical item (the engine reassigns `unit_id` on area reload).

**Root cause:** Dedup was keyed only by `unit_id`, which isn't stable
across unload/reload.

**Fix:**
- Added `item_data::SEED = 0x14` and read `dwSeed` at push time into
  `LootEntry.seed`.
- `LootHistory` now indexes by both `by_unit_id` and `by_seed`.
- `LootHistory::push` returns `PushOutcome::{Inserted, Merged,
  Resurrected, Duplicate}`. On `seed`-match into a `Pending` entry,
  re-key (`Merged`) the existing row's `unit_id` / `p_unit` / `p_unit_data`
  to the new sighting and skip the `loot-history-entry` emit.
  `PickedUp` / `TakenByOther` matches return `Duplicate` (terminals are
  truthful — don't disturb).

`ItemDropEvent` gained `seed: u32` and `history_pushed: bool` fields;
`main.rs` only fires `loot-history-entry` when `history_pushed == true`.

### 8.4 Conservative slot-freed → `Lost` (MP "another player took it")

**Symptom (regression after 8.2):** When a teammate picked up an item in
MP, the entry stayed `Pending` forever instead of flipping to `Lost`.

**Root cause:** With the timeout gone (8.2), there was no signal at all
for "item is gone".

**Fix:** Cache `p_unit` (UnitAny pointer) at push time alongside
`p_unit_data`. Per tick, read back `p_unit + 0x0C` (live `unit_id`):

| Read result | Interpretation | Action |
|---|---|---|
| `Ok(uid)` where `uid == entry.unit_id` | Slot still ours | continue (stay Pending) |
| `Ok(uid)` where `uid != 0 && uid != entry.unit_id` | Slot reused for new live unit | `freed_ids.insert(uid)` → Lost |
| `Ok(0)` | Slot zeroed (likely unmapped during area transition) | leave Pending |
| `Err` | Page unmapped (area unload) | leave Pending |

This conservative rule fires on the strong "different live unit lives
here now" signal — typical when D2 reuses a freed slot in a still-loaded
area (i.e., exactly the MP-pickup case) — but ignores the area-unload
pattern. See 8.6 for the safety net when this still false-fires.

`LootEntry` gained `p_unit: u32`. `pending_uids_with_ptrs()` returns
`(u32, u32, u32)`. `classify_pending_pickups` returns
`(our, other, freed)`. `resolve_pending` takes a 4th `freed_ids: &HashSet<u32>`
parameter; priority order is `our → other → freed → stay-Pending`.

### 8.5 Menu-exit sweep: all `Pending` → `Lost`

**Feature, not bug fix.** On `ingame → menu` (player exits to character
select / main menu), every still-`Pending` entry is force-transitioned
to `Lost` via `LootHistory::mark_all_pending_lost()`. One
`loot-history-update` per transition is emitted to the panel. Rationale:
the session is effectively over for those items.

The `menu → ingame` clear remains unchanged (next game wipes the panel).

### 8.6 Resurrection: `Lost` → `Pending` via seed-merge

**Safety net for 8.4.** Items can occasionally trip the freed-detection
during a fast TP-and-back (D2 may briefly reuse the slot before the area
fully unloads). Without help these would stick on `⊘ Lost` even though
the item is still physically on the ground in the original area.

**Fix:** On `LootHistory::push`, if a `seed` match lands on an existing
`Lost` entry, flip it back to `Pending`, re-key the pointers, and return
`PushOutcome::Resurrected { unit_id }`. The scanner enqueues a
`(unit_id, Pending)` entry into `last_pickup_updates`, which `main.rs`
broadcasts as `loot-history-update` so the panel flips the icon back to
`⏳`. `PickedUp` / `TakenByOther` are still treated as `Duplicate` — only
`Lost` is resurrectable, since only `Lost` is potentially a false positive.

### 8.7 Perf: skip per-tick work when nothing is `Pending`

`classify_pending_pickups` early-returns empty sets when its `pending`
input is empty, **before** the player-inventory walk. Once every history
row has reached a terminal state, the per-tick cost of the loot-history
subsystem drops to zero RPMs.

### Final per-tick cost (in-game, with N Pending entries, M items in
hero inventory)

- 0 RPMs when `N == 0` (steady-state after farming).
- `M + 3` RPMs for the inventory walk when `N > 0`.
- 1 RPM per Pending entry for the slot-freed check.
- Up to 3 additional RPMs per Pending entry for the `INV_OWNER` chain
  (only when slot still ours and not in our inventory walk).
- Plus 1 RPM per pushed entry for `dwSeed`.

Typical loaded session (e.g. 10 Pending, 40 inventory items): ~50–60
RPMs/tick = ~0.1–0.2 ms at ~3 µs/RPM. Negligible against the existing
`pPaths` walk.

### Summary of pickup-state transitions (final)

```
                  ┌──────────────┐
                  │   (push)     │
                  └──────┬───────┘
                         ▼
                  ┌──────────────┐
                  │   Pending    │◀─────────┐
                  └─┬─────┬─────┬┘          │
                    │     │     │           │
   our_inventory ───┘     │     └─── slot_freed
                          │                 │
                  other_container       (Lost)─── seed-resurrect ──┘
                          │                 │
                          ▼                 ▼
                  ┌──────────────┐  ┌──────────────┐
                  │ TakenByOther │  │     Lost     │
                  └──────────────┘  └──────────────┘
                  ┌──────────────┐
                  │   PickedUp   │  ◀── from our_inventory
                  └──────────────┘

  menu→ingame:  clears entire history
  ingame→menu:  every Pending → Lost (sweep)
```
