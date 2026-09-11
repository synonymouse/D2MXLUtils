use std::collections::{HashMap, HashSet};

use super::VisibilityMaskOp;

const MASK_INDEX_BITS: u32 = 0xFFFF;

#[derive(Debug, Clone)]
pub(in crate::notifier) struct HookBitTracker {
    threshold: u8,
    tracked: HashSet<u32>,
    missed: HashMap<u32, u8>,
}

#[derive(Debug, Clone)]
pub(in crate::notifier) struct HookCleanupFailureLogThrottle {
    suppressed_failure_ticks: u32,
    ticks_since_log: Option<u32>,
    suppressed_failures: u64,
}

#[derive(Debug, Clone, Default)]
pub(in crate::notifier) struct PendingVisibilityMaskOps {
    pending: HashMap<u32, Vec<VisibilityMaskOp>>,
}

impl HookBitTracker {
    pub fn new(threshold: u8) -> Self {
        Self {
            threshold: threshold.max(1),
            tracked: HashSet::new(),
            missed: HashMap::new(),
        }
    }

    pub fn mark_written(&mut self, unit_id: u32) {
        self.tracked.insert(unit_id);
        self.missed.remove(&unit_id);
    }

    pub fn plan_clears(&mut self, current_item_ids: &HashSet<u32>) -> Vec<u32> {
        let mut out = Vec::new();
        let protected_mask_indexes: HashSet<u32> = current_item_ids
            .iter()
            .map(|unit_id| unit_id & MASK_INDEX_BITS)
            .collect();

        for &unit_id in self.tracked.iter() {
            if current_item_ids.contains(&unit_id) {
                self.missed.remove(&unit_id);
                continue;
            }

            if protected_mask_indexes.contains(&(unit_id & MASK_INDEX_BITS)) {
                self.missed.remove(&unit_id);
                continue;
            }

            let count = self.missed.entry(unit_id).or_insert(0);
            *count = count.saturating_add(1);
            if *count >= self.threshold {
                out.push(unit_id);
            }
        }
        out.sort_unstable();
        out
    }

    pub fn confirm_cleared(&mut self, unit_ids: &[u32]) {
        for &unit_id in unit_ids {
            self.tracked.remove(&unit_id);
            self.missed.remove(&unit_id);
        }
    }

    pub fn clear(&mut self) {
        self.tracked.clear();
        self.missed.clear();
    }

    pub fn tracked_len(&self) -> usize {
        self.tracked.len()
    }

    #[cfg(test)]
    pub fn missed_len(&self) -> usize {
        self.missed.len()
    }

    pub fn overdue_len(&self) -> usize {
        self.missed
            .values()
            .filter(|&&count| count >= self.threshold)
            .count()
    }

    pub fn departed_mask_collisions(
        &self,
        unit_id: u32,
        current_item_ids: &HashSet<u32>,
    ) -> Vec<u32> {
        if !current_item_ids.contains(&unit_id) {
            return Vec::new();
        }

        let mask_index = unit_id & MASK_INDEX_BITS;
        let mut out: Vec<u32> = self
            .tracked
            .iter()
            .copied()
            .filter(|&tracked_id| {
                tracked_id != unit_id
                    && !current_item_ids.contains(&tracked_id)
                    && (tracked_id & MASK_INDEX_BITS) == mask_index
            })
            .collect();
        out.sort_unstable();
        out
    }
}

impl HookCleanupFailureLogThrottle {
    pub fn new(suppressed_failure_ticks: u32) -> Self {
        Self {
            suppressed_failure_ticks: suppressed_failure_ticks.max(1),
            ticks_since_log: None,
            suppressed_failures: 0,
        }
    }

    pub fn record_failure(&mut self) -> Option<u64> {
        match self.ticks_since_log {
            None => {
                self.ticks_since_log = Some(0);
                Some(0)
            }
            Some(ticks) if ticks >= self.suppressed_failure_ticks => {
                let suppressed = self.suppressed_failures;
                self.ticks_since_log = Some(0);
                self.suppressed_failures = 0;
                Some(suppressed)
            }
            Some(ticks) => {
                self.ticks_since_log = Some(ticks.saturating_add(1));
                self.suppressed_failures = self.suppressed_failures.saturating_add(1);
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.ticks_since_log = None;
        self.suppressed_failures = 0;
    }
}

impl PendingVisibilityMaskOps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn take(&mut self, unit_id: u32) -> Vec<VisibilityMaskOp> {
        self.pending.remove(&unit_id).unwrap_or_default()
    }

    pub fn record_failed(&mut self, unit_id: u32, failed_ops: Vec<VisibilityMaskOp>) {
        if failed_ops.is_empty() {
            self.pending.remove(&unit_id);
        } else {
            self.pending.insert(unit_id, failed_ops);
        }
    }

    pub fn retain_current(&mut self, current_item_ids: &HashSet<u32>) {
        self.pending
            .retain(|unit_id, _| current_item_ids.contains(unit_id));
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests;
