//! BFS over the room graph and reconcile the automap-marker chain. Owns
//! `MapMarkerManager` exclusively. Reads cached filter decisions via snapshot
//! to keep the items thread unblocked and avoid duplicate rule matching.

#![cfg(any(target_os = "windows", target_os = "linux"))]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::logger::error as log_error;
use crate::map_marker::{self, MapMarkerManager, MarkerItem};
use crate::offsets::{d2client, unit};
use crate::rules::Visibility;
use crate::scanner_state::{BfsItemCandidate, CachedFilterDecision, SharedScannerState};

pub struct MarkerScanner {
    state: Arc<SharedScannerState>,
    map_marker: MapMarkerManager,
    markers_cleared: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedMarkerDecision {
    Place,
    DoNotPlace,
    Unknown,
}

fn cached_marker_decision(
    decision: Option<&CachedFilterDecision>,
    current_generation: u64,
) -> CachedMarkerDecision {
    let Some(decision) = decision else {
        return CachedMarkerDecision::Unknown;
    };
    if decision.generation != current_generation {
        return CachedMarkerDecision::Unknown;
    }
    if decision.place_on_map && decision.visibility != Visibility::Hide {
        CachedMarkerDecision::Place
    } else {
        CachedMarkerDecision::DoNotPlace
    }
}

fn take_marker_clear_needed(markers_cleared: &mut bool) -> bool {
    if *markers_cleared {
        false
    } else {
        *markers_cleared = true;
        true
    }
}

fn mark_marker_path_active(markers_cleared: &mut bool) {
    *markers_cleared = false;
}

fn bfs_candidates_from_positions(
    positions: &[(u32, i32, i32)],
    unit_ids: &HashMap<u32, u32>,
) -> HashMap<u32, BfsItemCandidate> {
    positions
        .iter()
        .filter_map(|&(p_unit, sub_x, sub_y)| {
            unit_ids.get(&p_unit).map(|&unit_id| {
                (
                    unit_id,
                    BfsItemCandidate {
                        unit_id,
                        p_unit,
                        sub_x,
                        sub_y,
                    },
                )
            })
        })
        .collect()
}

fn replace_recent_bfs_items(
    state: &SharedScannerState,
    candidates: HashMap<u32, BfsItemCandidate>,
) {
    if let Ok(mut guard) = state.recent_bfs_items.write() {
        *guard = candidates;
    }
}

fn clear_recent_bfs_items(state: &SharedScannerState) {
    replace_recent_bfs_items(state, HashMap::new());
}

impl MarkerScanner {
    pub fn new(state: Arc<SharedScannerState>) -> Self {
        Self {
            state,
            map_marker: MapMarkerManager::new(),
            markers_cleared: true,
        }
    }

    /// One BFS + marker reconciliation pass. No-op outside of a live game;
    /// clears markers when the loaded filter has no map rules.
    pub fn tick(&mut self) {
        let p_player = self
            .state
            .ctx
            .process
            .read_memory::<u32>(self.state.ctx.d2_client + d2client::PLAYER_UNIT)
            .unwrap_or(0);
        if p_player == 0 {
            clear_recent_bfs_items(self.state.as_ref());
            return;
        }

        let filter_snapshot = match self.state.filter_config.read() {
            Ok(g) => g.as_ref().map(|filter_arc| {
                (
                    filter_arc.clone(),
                    self.state.filter_generation.load(Ordering::SeqCst),
                )
            }),
            Err(_) => {
                clear_recent_bfs_items(self.state.as_ref());
                return;
            }
        };
        let Some((filter_arc, current_generation)) = filter_snapshot else {
            clear_recent_bfs_items(self.state.as_ref());
            return;
        };
        let has_map_rules = match filter_arc.read() {
            Ok(f) => f.has_map_rules(),
            Err(_) => {
                clear_recent_bfs_items(self.state.as_ref());
                return;
            }
        };
        if !has_map_rules {
            clear_recent_bfs_items(self.state.as_ref());
            if take_marker_clear_needed(&mut self.markers_cleared) {
                if let Err(e) = self.map_marker.clear(&self.state.ctx) {
                    log_error(&format!("map_marker clear (no map rules) failed: {}", e));
                }
            }
            return;
        }
        mark_marker_path_active(&mut self.markers_cleared);

        // Depth 10 reaches past what the engine typically keeps loaded; BFS
        // stops early when `ppRoomsNear` runs out.
        let positions = match map_marker::bfs_item_positions(&self.state.ctx, 10) {
            Ok(v) => v,
            Err(e) => {
                clear_recent_bfs_items(self.state.as_ref());
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
        let candidates = bfs_candidates_from_positions(&positions, &unit_ids);
        replace_recent_bfs_items(self.state.as_ref(), candidates);

        // Snapshot, then release the read lock — items thread mustn't block
        // on inserts while marker reconciliation runs.
        let snapshot: HashMap<u32, CachedFilterDecision> =
            match self.state.recent_filter_decisions.read() {
                Ok(g) => g.clone(),
                Err(_) => return,
            };

        let mut newly_matched: Vec<MarkerItem> = Vec::new();
        let mut explicitly_unmarked: HashSet<u32> = HashSet::new();
        for (p_unit, sub_x, sub_y) in positions {
            let Some(&unit_id) = unit_ids.get(&p_unit) else {
                continue;
            };
            match cached_marker_decision(snapshot.get(&unit_id), current_generation) {
                CachedMarkerDecision::Place => {
                    let (cx, cy) = map_marker::sub_to_cell(sub_x, sub_y);
                    newly_matched.push(MarkerItem {
                        unit_id,
                        cell_x: cx,
                        cell_y: cy,
                        sub_x,
                        sub_y,
                    });
                }
                CachedMarkerDecision::DoNotPlace => {
                    explicitly_unmarked.insert(unit_id);
                }
                CachedMarkerDecision::Unknown => {}
            }
        }

        let player_sub = map_marker::read_player_subtile(&self.state.ctx);

        let injector = match self.state.injector.lock() {
            Ok(i) => i,
            Err(p) => p.into_inner(),
        };
        if let Err(e) = self.map_marker.tick(
            &self.state.ctx,
            &*injector,
            &newly_matched,
            &explicitly_unmarked,
            &bfs_unit_ids,
            player_sub,
        ) {
            log_error(&format!("map_marker tick failed: {}", e));
        }
    }

    /// Drop all markers; called on game-entry transitions.
    pub fn clear(&mut self) {
        clear_recent_bfs_items(self.state.as_ref());
        if let Err(e) = self.map_marker.clear(&self.state.ctx) {
            log_error(&format!("map_marker clear (game-entry) failed: {}", e));
        }
        self.markers_cleared = true;
    }

    /// Drop all markers; called when D2 closes or the scanner stops.
    pub fn shutdown(&mut self) {
        clear_recent_bfs_items(self.state.as_ref());
        if let Err(e) = self.map_marker.clear(&self.state.ctx) {
            log_error(&format!("map_marker clear on shutdown failed: {}", e));
        }
        self.markers_cleared = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::Visibility;
    use crate::scanner_state::CachedFilterDecision;

    #[test]
    fn cached_marker_decision_requires_current_visible_map_decision() {
        let current = CachedFilterDecision {
            generation: 7,
            visibility: Visibility::Show,
            place_on_map: true,
        };
        let stale = CachedFilterDecision {
            generation: 6,
            visibility: Visibility::Show,
            place_on_map: true,
        };
        let hidden = CachedFilterDecision {
            generation: 7,
            visibility: Visibility::Hide,
            place_on_map: true,
        };
        let no_map = CachedFilterDecision {
            generation: 7,
            visibility: Visibility::Show,
            place_on_map: false,
        };

        assert_eq!(
            cached_marker_decision(Some(&current), 7),
            CachedMarkerDecision::Place
        );
        assert_eq!(
            cached_marker_decision(Some(&stale), 7),
            CachedMarkerDecision::Unknown
        );
        assert_eq!(
            cached_marker_decision(Some(&hidden), 7),
            CachedMarkerDecision::DoNotPlace
        );
        assert_eq!(
            cached_marker_decision(Some(&no_map), 7),
            CachedMarkerDecision::DoNotPlace
        );
        assert_eq!(
            cached_marker_decision(None, 7),
            CachedMarkerDecision::Unknown
        );
    }

    #[test]
    fn marker_clear_gate_clears_once_until_marker_path_is_active_again() {
        let mut markers_cleared = false;

        assert!(take_marker_clear_needed(&mut markers_cleared));
        assert!(!take_marker_clear_needed(&mut markers_cleared));

        mark_marker_path_active(&mut markers_cleared);

        assert!(take_marker_clear_needed(&mut markers_cleared));
    }
}
