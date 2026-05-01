//! Session-only loot history: items that fired a `notify` rule, with
//! per-entry pickup state resolved against the local player's inventory.
//!
//! This module is pure data — no Win32, no D2 memory access. The scanner
//! (`notifier.rs`) drives state transitions by calling `push`, then
//! `resolve_pending` once per tick with the current inventory ids.

use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum entries kept per session. Older entries are evicted FIFO.
pub const MAX_ENTRIES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupState {
    /// On the ground, or in flight between ground and an inventory, or
    /// out of view (different area, town). Stays Pending until we have
    /// positive evidence of pickup — map changes do NOT auto-transition.
    Pending,
    /// In our local hero's inventory (terminal).
    PickedUp,
    /// Session ended while still Pending (player left the game). Terminal:
    /// no longer reachable from this session.
    Lost,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LootEntry {
    pub unit_id: u32,
    /// Milliseconds since UNIX epoch (set at push time).
    pub timestamp_ms: u64,
    /// Final display name as it appears in the notification.
    pub name: String,
    /// Lowercase color keyword from the winning rule's `color` flag (e.g.
    /// `"lime"`, `"gold"`). `None` = default color (frontend falls back to
    /// quality color or a neutral foreground).
    pub color: Option<String>,
    pub pickup: PickupState,
    /// `dwSeed` (item random seed) at offset `0x14` of `ItemData`. Stable
    /// per-item across area unload/reload in MP. Used as the dedup key:
    /// when the engine assigns a new `unit_id` to the same physical item
    /// after a teleport-away/return cycle, we re-key the existing entry
    /// instead of creating a duplicate row in the panel. Also serves as
    /// the stable identity for the frontend (the indexable key — `unit_id`
    /// can change underneath us via merge).
    ///
    /// `0` means "unknown" (read failed at push time) → falls back to
    /// `unit_id` for dedup, and the frontend keys by `unit_id` for that row.
    #[serde(default)]
    pub seed: u32,
}

/// Result of [`LootHistory::push`]. `Inserted` and `Merged` both leave
/// a row in the panel; `Duplicate` is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// New row created in the panel.
    Inserted,
    /// Existing row found by `seed`; its `unit_id` was updated to the new
    /// sighting. No new row produced.
    Merged,
    /// Same `unit_id` already present, or `seed` matches a terminal entry — no-op.
    Duplicate,
}

/// FIFO ring of session entries. Indexed by both `unit_id` (for live
/// classification updates) and `seed` (for cross-area dedup).
#[derive(Debug, Default)]
pub struct LootHistory {
    entries: VecDeque<LootEntry>,
    by_unit_id: HashMap<u32, usize>,
    /// Maps `dwSeed` → entry index. Lets us recognize the same physical
    /// item after a teleport-away/return cycle (the engine assigns a new
    /// `unit_id` but `seed` is preserved).
    by_seed: HashMap<u32, usize>,
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
        self.by_seed.clear();
    }

    /// Append a new entry, with two layers of dedup:
    /// 1. **By `seed`** — if a `Pending` entry already exists with the same
    ///    item seed, treat the push as a re-sighting of the same physical
    ///    item (typical after teleport-away/return). Update its `unit_id`
    ///    so the classifier can follow the new game-engine handle. No new
    ///    row produced. Terminal entries (PickedUp/Lost) are not disturbed.
    /// 2. **By `unit_id`** — if the uid is already in history (scanner
    ///    flickered the same item off and on `pPaths`), no-op.
    ///
    /// Evicts the oldest entry FIFO when at `MAX_ENTRIES`.
    pub fn push(&mut self, entry: LootEntry) -> PushOutcome {
        // Re-key dedup: same physical item across area unload/reload.
        if entry.seed != 0 {
            if let Some(&idx) = self.by_seed.get(&entry.seed) {
                if let Some(existing) = self.entries.get_mut(idx) {
                    match existing.pickup {
                        PickupState::Pending => {
                            let old_uid = existing.unit_id;
                            existing.unit_id = entry.unit_id;
                            if old_uid != entry.unit_id {
                                self.by_unit_id.remove(&old_uid);
                                self.by_unit_id.insert(entry.unit_id, idx);
                            }
                            return PushOutcome::Merged;
                        }
                        // Terminal — don't disturb the historical record.
                        PickupState::PickedUp | PickupState::Lost => {
                            return PushOutcome::Duplicate;
                        }
                    }
                }
            }
        }

        if self.by_unit_id.contains_key(&entry.unit_id) {
            return PushOutcome::Duplicate;
        }

        if self.entries.len() == MAX_ENTRIES {
            if let Some(evicted) = self.entries.pop_front() {
                self.by_unit_id.remove(&evicted.unit_id);
                if evicted.seed != 0 {
                    self.by_seed.remove(&evicted.seed);
                }
            }
            // VecDeque indices shifted; rebuild the index maps.
            self.by_unit_id.clear();
            self.by_seed.clear();
            for (idx, e) in self.entries.iter().enumerate() {
                self.by_unit_id.insert(e.unit_id, idx);
                if e.seed != 0 {
                    self.by_seed.insert(e.seed, idx);
                }
            }
        }

        let idx = self.entries.len();
        self.by_unit_id.insert(entry.unit_id, idx);
        if entry.seed != 0 {
            self.by_seed.insert(entry.seed, idx);
        }
        self.entries.push_back(entry);
        PushOutcome::Inserted
    }

    /// Walk every entry still in `Pending` and advance to `PickedUp` if
    /// its `unit_id` is found in our local hero's inventory. Returns
    /// `(unit_id, seed, new_state)` transitions so the caller can emit
    /// events keyed by `seed` (stable across rekey).
    ///
    /// `seed` is `0` when the original push failed to read it — frontend
    /// falls back to `unit_id` for indexing in that case.
    pub fn resolve_pending(
        &mut self,
        our_inventory_ids: &HashSet<u32>,
    ) -> Vec<(u32, u32, PickupState)> {
        let mut updates = Vec::new();

        for entry in self.entries.iter_mut() {
            if entry.pickup != PickupState::Pending {
                continue;
            }

            if our_inventory_ids.contains(&entry.unit_id) {
                entry.pickup = PickupState::PickedUp;
                updates.push((entry.unit_id, entry.seed, PickupState::PickedUp));
            }
        }

        updates
    }

    /// True if any entry is still `Pending`. Used by the scanner as an
    /// early-out: if nothing is pending, no need to walk the player's
    /// inventory this tick.
    pub fn has_pending(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.pickup == PickupState::Pending)
    }

    /// Force every `Pending` entry to `Lost` — used when the player exits
    /// the active game (main menu / lobby / disconnect). Returns the list
    /// of `(unit_id, seed, Lost)` transitions so the caller can broadcast
    /// `loot-history-update` events. Items left on the ground when
    /// exiting are effectively gone (game session ended), so this is a
    /// safe terminal.
    pub fn mark_all_pending_lost(&mut self) -> Vec<(u32, u32, PickupState)> {
        let mut updates = Vec::new();
        for entry in self.entries.iter_mut() {
            if entry.pickup == PickupState::Pending {
                entry.pickup = PickupState::Lost;
                updates.push((entry.unit_id, entry.seed, PickupState::Lost));
            }
        }
        updates
    }
}

/// Milliseconds since UNIX epoch. Wall-clock — frontend renders as HH:MM:SS.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
            seed: 0,
        }
    }

    fn entry_with_seed(unit_id: u32, seed: u32, name: &str) -> LootEntry {
        let mut e = entry(unit_id, name);
        e.seed = seed;
        e
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

    #[test]
    fn pending_becomes_picked_up_when_in_our_inventory() {
        let mut h = LootHistory::new();
        h.push(entry(42, "x"));

        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);

        let updates = h.resolve_pending(&ours);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0], (42, 0, PickupState::PickedUp));
        assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);
    }

    #[test]
    fn pending_stays_pending_when_not_in_inventory() {
        // Map change / area unload / item just sitting on ground:
        // no positive evidence → stay Pending forever.
        let mut h = LootHistory::new();
        h.push(entry(42, "x"));

        let ours: HashSet<u32> = HashSet::new();

        for _ in 0..1000 {
            let updates = h.resolve_pending(&ours);
            assert!(updates.is_empty(), "must not auto-transition");
        }
        assert_eq!(h.snapshot()[0].pickup, PickupState::Pending);
    }

    #[test]
    fn terminal_states_do_not_emit_again() {
        let mut h = LootHistory::new();
        h.push(entry(42, "x"));

        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);

        let _ = h.resolve_pending(&ours); // PickedUp
        let updates = h.resolve_pending(&ours);
        assert!(updates.is_empty(), "terminal state must not re-emit");
    }

    #[test]
    fn seed_merge_rekeys_existing_pending_entry() {
        // Same physical item seen again with a fresh unit_id after the
        // player teleported away and returned: merge into the existing
        // entry, do NOT add a duplicate row.
        let mut h = LootHistory::new();
        let outcome = h.push(entry_with_seed(42, 0xDEADBEEF, "TU Helm"));
        assert_eq!(outcome, PushOutcome::Inserted);
        assert_eq!(h.len(), 1);

        let outcome = h.push(entry_with_seed(99, 0xDEADBEEF, "TU Helm"));
        assert_eq!(outcome, PushOutcome::Merged);
        assert_eq!(h.len(), 1, "seed dedup must not create a 2nd row");

        let snap = h.snapshot();
        assert_eq!(snap[0].unit_id, 99, "uid must be re-keyed to new sighting");
        assert_eq!(snap[0].pickup, PickupState::Pending);
    }

    #[test]
    fn seed_merge_emits_update_under_new_uid_after_rekey() {
        // After seed-merge, resolve_pending must report the *current* uid
        // (the rekeyed one) so the frontend can look up by stable seed.
        let mut h = LootHistory::new();
        h.push(entry_with_seed(42, 0xDEADBEEF, "TU Helm"));
        h.push(entry_with_seed(99, 0xDEADBEEF, "TU Helm")); // rekey to 99

        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(99); // picked up under new uid
        let updates = h.resolve_pending(&ours);
        assert_eq!(updates, vec![(99, 0xDEADBEEF, PickupState::PickedUp)]);
    }

    #[test]
    fn seed_merge_does_not_disturb_terminal_entries() {
        let mut h = LootHistory::new();
        h.push(entry_with_seed(42, 0xCAFEBABE, "Rune"));

        let mut ours: HashSet<u32> = HashSet::new();
        ours.insert(42);
        let _ = h.resolve_pending(&ours);
        assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);

        let outcome = h.push(entry_with_seed(99, 0xCAFEBABE, "Rune"));
        assert_eq!(outcome, PushOutcome::Duplicate);
        assert_eq!(h.len(), 1);
        assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);
        assert_eq!(h.snapshot()[0].unit_id, 42);
    }

    #[test]
    fn lost_terminal_is_not_resurrected_by_seed_match() {
        // After menu-exit Lost, a coincidental seed sighting in a new
        // session must not flip the historical row back to Pending.
        let mut h = LootHistory::new();
        h.push(entry_with_seed(42, 0xABCD1234, "TU Helm"));
        let lost = h.mark_all_pending_lost();
        assert_eq!(lost, vec![(42, 0xABCD1234, PickupState::Lost)]);

        let outcome = h.push(entry_with_seed(99, 0xABCD1234, "TU Helm"));
        assert_eq!(outcome, PushOutcome::Duplicate);
        assert_eq!(h.len(), 1);
        assert_eq!(h.snapshot()[0].pickup, PickupState::Lost);
        assert_eq!(h.snapshot()[0].unit_id, 42);
    }

    #[test]
    fn zero_seed_falls_back_to_unit_id_dedup() {
        // Items whose seed read failed (seed=0) must not collide via
        // the by_seed index — fall through to unit_id dedup.
        let mut h = LootHistory::new();
        let a = h.push(entry_with_seed(1, 0, "A"));
        let b = h.push(entry_with_seed(2, 0, "B"));
        assert_eq!(a, PushOutcome::Inserted);
        assert_eq!(b, PushOutcome::Inserted);
        assert_eq!(h.len(), 2);
    }
}
