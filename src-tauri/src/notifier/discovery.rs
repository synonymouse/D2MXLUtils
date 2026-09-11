//! Ordered nearby-room discovery and verified marker-BFS candidate consumption.

use super::{DropScanner, GoblinDetectedEvent, ItemDropEvent};
use crate::d2types::UnitAny;
use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{d2client, paths, stat_list, unit, unit_type};
use crate::scanner_state::{BfsItemCandidate, CachedFilterDecision};
use crate::stat_telemetry::StatConsumer;
use crate::unit_stats_reader::fallback::StatReadContext;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

/// MonStats.txt class IDs that count as "goblins" for the alert sound.
/// Ported verbatim from `D2Stats.au3:$g_goblinIds`.
const GOBLIN_CLASS_IDS: &[u32] = &[
    2774, 2775, 2776, 2779, 2780, 2781, 2784, 2785, 2786, 2787, 2788, 2789, 2790, 2791, 2792, 2793,
    2794, 2795, 2799, 2802, 2803, 2805,
];

pub(super) const MAX_ITEM_SCAN_PATHS: usize = 1024;
pub(super) const MAX_ITEM_SCAN_UNITS_PER_PATH: usize = 4096;

pub(super) fn capped_item_scan_path_count(i_paths: usize) -> (usize, bool) {
    let capped = i_paths.min(MAX_ITEM_SCAN_PATHS);
    (capped, i_paths > capped)
}

pub(super) fn item_scan_unit_index_in_bounds(index: usize) -> bool {
    index < MAX_ITEM_SCAN_UNITS_PER_PATH
}

pub(super) fn should_enrich_bfs_candidate(
    candidate: &BfsItemCandidate,
    current_item_ids: &HashSet<u32>,
    recent_filter_decisions: &HashMap<u32, CachedFilterDecision>,
    current_generation: u64,
) -> bool {
    if current_item_ids.contains(&candidate.unit_id) {
        return false;
    }
    !matches!(
        recent_filter_decisions.get(&candidate.unit_id),
        Some(decision) if decision.generation == current_generation
    )
}

impl DropScanner {
    /// Scan ground items (pPaths pass) and return fresh notification events.
    ///
    /// Intentionally excludes the map-marker BFS pass so callers can emit
    /// `item-drop` events before the (potentially expensive) marker
    /// reconciliation runs. The marker pass is handled by `MarkerScanner::tick`.
    pub fn tick_items(&mut self) -> Vec<ItemDropEvent> {
        let mut events = Vec::new();

        if !self.is_ingame() {
            return events;
        }

        if self.class_cache.is_none() {
            match self.build_class_cache() {
                Ok(cache) => {
                    log_info(&format!("Class cache built: {} classes", cache.len()));
                    self.class_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build class cache: {}", e));
                    // Install an empty cache so we don't keep retrying every tick.
                    self.class_cache = Some(Vec::new());
                }
            }
        }

        if self.unique_cache.is_none() {
            match self.build_unique_items_cache() {
                Ok(cache) => {
                    log_info(&format!("Unique cache built: {} records", cache.len()));
                    self.unique_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build unique cache: {}", e));
                    self.unique_cache = Some(Vec::new());
                }
            }
        }

        if self.set_cache.is_none() {
            match self.build_set_items_cache() {
                Ok(cache) => {
                    log_info(&format!("Set cache built: {} records", cache.len()));
                    self.set_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build set cache: {}", e));
                    self.set_cache = Some(Vec::new());
                }
            }
        }

        // Read paths structure to iterate through rooms/units
        let base_ptr = self.state.ctx.d2_client + d2client::PLAYER_UNIT;

        // Follow pointer chain: [base] -> [+0x2C] -> [+0x1C] -> pPaths (at +0x0) and iPaths (at +0x24)
        let ptr1 = match self.state.ctx.process.read_memory::<u32>(base_ptr) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        // Sampled once per tick (not per item) — clvl/class don't change
        // between items in the same scan pass.
        {
            let injector = self.state.injector.lock().unwrap();
            if let Ok(value) = StatReadContext::new(&self.state.ctx, &injector, StatConsumer::Level)
                .read_stat(ptr1 as u32, u32::from(stat_list::STAT_LEVEL))
            {
                self.char_level = u32::from_ne_bytes(value.to_ne_bytes());
            }
        }
        if let Ok(class) = self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr1 + unit::CLASS)
        {
            self.player_class = class;
        }

        let ptr2 = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr1 + paths::TO_PATHS_PTR[1])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let ptr3 = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr2 + paths::TO_PATHS_PTR[2])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let p_paths = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_PTR[3])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let i_paths = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_COUNT[3])
        {
            Ok(p) => p as usize,
            _ => return events,
        };
        let (path_count, path_count_capped) = capped_item_scan_path_count(i_paths);
        if path_count_capped {
            log_error(&format!(
                "item scan path count cap hit; scanning first {} paths",
                MAX_ITEM_SCAN_PATHS
            ));
        }

        let mut current_item_ids: HashSet<u32> = HashSet::new();

        // Two passes keep cleanup aware of current ids without storing pUnit snapshots.
        for i in 0..path_count {
            let p_path = match self.state.ctx.process.read_memory::<u32>(p_paths + 4 * i) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };

            let mut p_unit = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_path + paths::PATH_TO_UNIT)
            {
                Ok(p) if p != 0 => p,
                _ => continue,
            };

            let mut units_visited = 0;
            let mut unit_cap_hit = false;
            while p_unit != 0 {
                if !item_scan_unit_index_in_bounds(units_visited) {
                    unit_cap_hit = true;
                    break;
                }
                units_visited += 1;
                let unit: UnitAny = match self.state.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };

                if unit.unit_type == unit_type::ITEM {
                    current_item_ids.insert(unit.unit_id);
                } else if unit.unit_type == unit_type::MONSTER
                    && GOBLIN_CLASS_IDS.contains(&unit.class)
                    && self.seen_goblins.insert(unit.unit_id)
                {
                    self.last_goblin_events.push(GoblinDetectedEvent {
                        unit_id: unit.unit_id,
                        class: unit.class,
                    });
                }

                p_unit = unit.p_next_unit;
            }
            if unit_cap_hit {
                log_error(&format!(
                    "item scan unit cap hit during current-id pass; max={} units per path",
                    MAX_ITEM_SCAN_UNITS_PER_PATH
                ));
            }
        }

        for i in 0..path_count {
            let p_path = match self.state.ctx.process.read_memory::<u32>(p_paths + 4 * i) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };

            let mut p_unit = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_path + paths::PATH_TO_UNIT)
            {
                Ok(p) if p != 0 => p,
                _ => continue,
            };

            let mut units_visited = 0;
            let mut unit_cap_hit = false;
            while p_unit != 0 {
                if !item_scan_unit_index_in_bounds(units_visited) {
                    unit_cap_hit = true;
                    break;
                }
                units_visited += 1;
                let unit: UnitAny = match self.state.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };
                let next_unit = unit.p_next_unit;

                if unit.unit_type != unit_type::ITEM {
                    p_unit = next_unit;
                    continue;
                }
                current_item_ids.insert(unit.unit_id);

                if self.loot_hook.is_injected() {
                    let already_seen = self.seen_items.contains(&unit.unit_id);
                    if already_seen {
                        self.retry_pending_visibility_ops(unit.unit_id);
                    } else if !self.reset_departed_mask_collision(unit.unit_id, &current_item_ids) {
                        p_unit = next_unit;
                        continue;
                    }
                }

                if let Some(scanned) = self.scan_unit(p_unit, &unit) {
                    self.process_scanned_item(scanned, &mut events);
                }
                p_unit = next_unit;
            }
            if unit_cap_hit {
                log_error(&format!(
                    "item scan unit cap hit during scan pass; max={} units per path",
                    MAX_ITEM_SCAN_UNITS_PER_PATH
                ));
            }
        }

        let mut bfs_candidates: Vec<BfsItemCandidate> = self
            .state
            .recent_bfs_items
            .read()
            .map(|items| items.values().copied().collect())
            .unwrap_or_default();
        bfs_candidates.sort_by_key(|candidate| candidate.unit_id);
        let current_generation = self.state.filter_generation.load(Ordering::SeqCst);
        let decision_snapshot: HashMap<u32, CachedFilterDecision> = self
            .state
            .recent_filter_decisions
            .read()
            .map(|decisions| decisions.clone())
            .unwrap_or_default();
        for candidate in bfs_candidates {
            let needs_enrichment = should_enrich_bfs_candidate(
                &candidate,
                &current_item_ids,
                &decision_snapshot,
                current_generation,
            );
            if current_item_ids.contains(&candidate.unit_id) {
                continue;
            }

            let unit: UnitAny = match self
                .state
                .ctx
                .process
                .read_memory(candidate.p_unit as usize)
            {
                Ok(unit) => unit,
                Err(_) => continue,
            };
            if unit.unit_type != unit_type::ITEM || unit.unit_id != candidate.unit_id {
                continue;
            }
            current_item_ids.insert(unit.unit_id);

            if self.loot_hook.is_injected() {
                let already_seen = self.seen_items.contains(&unit.unit_id);
                if already_seen {
                    self.retry_pending_visibility_ops(unit.unit_id);
                } else if !self.reset_departed_mask_collision(unit.unit_id, &current_item_ids) {
                    continue;
                }
            }

            if !needs_enrichment {
                continue;
            }
            if let Some(scanned) = self.scan_unit(candidate.p_unit, &unit) {
                self.process_scanned_item(scanned, &mut events);
            }
        }

        // Keep hook-mask cleanup independent from `seen_items` pruning.
        let to_clear = self.hook_bits.plan_clears(&current_item_ids);
        if !to_clear.is_empty() && self.loot_hook.is_injected() {
            match self
                .loot_hook
                .clear_unit_id_bits(&self.state.ctx, &to_clear)
            {
                Ok(()) => {
                    self.hook_bits.confirm_cleared(&to_clear);
                    self.hook_cleanup_failure_logs.reset();
                }
                Err(e) => {
                    if let Some(suppressed) = self.hook_cleanup_failure_logs.record_failure() {
                        let suppressed = if suppressed > 0 {
                            format!(" (suppressed {} repeated cleanup failures)", suppressed)
                        } else {
                            String::new()
                        };
                        log_error(&format!(
                            "Failed to clear hook bits for {} departed items ({} overdue, {} tracked): {}{}",
                            to_clear.len(),
                            self.hook_bits.overdue_len(),
                            self.hook_bits.tracked_len(),
                            e,
                            suppressed
                        ));
                    }
                }
            }
        } else {
            self.hook_cleanup_failure_logs.reset();
        }

        // dwUnitId stays stable when an item moves between ground and
        // inventory, so without pruning a re-dropped item would never notify.
        self.seen_items.retain(|id| current_item_ids.contains(id));
        self.pending_visibility_ops
            .retain_current(&current_item_ids);
        self.state
            .recent_events
            .write()
            .unwrap()
            .retain(|id, _| current_item_ids.contains(id));
        self.state
            .recent_filter_decisions
            .write()
            .unwrap()
            .retain(|id, _| current_item_ids.contains(id));

        // Pickup resolution: walk the local hero's inventory once and
        // promote any matching Pending entries to PickedUp. Skip when no
        // entry is Pending — saves the inventory walk.
        let has_pending = self
            .loot_history
            .read()
            .map(|h| h.has_pending())
            .unwrap_or(false);
        if has_pending {
            let our_ids = self.read_player_inventory_ids();
            if let Ok(mut hist) = self.loot_history.write() {
                let resolved = hist.resolve_pending(&our_ids);
                self.last_pickup_updates.extend(resolved);
            }
        }

        if self.debug_get_item_stats_calls > 0 {
            if self.verbose_filter_logging {
                log_info(&format!(
                    "[Filter] runtime enrichment calls: GetItemStats={}",
                    self.debug_get_item_stats_calls
                ));
            }
            self.debug_get_item_stats_calls = 0;
        }

        events
    }
}
