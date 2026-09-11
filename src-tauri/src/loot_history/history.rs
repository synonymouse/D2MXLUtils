use std::collections::{HashMap, HashSet, VecDeque};

use super::{LootEntry, PickupState, PushOutcome, MAX_ENTRIES};

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
