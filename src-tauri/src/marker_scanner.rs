//! BFS over the room graph, filter-decide per item, reconcile the
//! automap-marker chain. Owns `MapMarkerManager` exclusively. Reads
//! `recent_events` via snapshot to keep the items thread unblocked.

#![cfg(target_os = "windows")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::logger::error as log_error;
use crate::map_marker::{self, MapMarkerManager, MarkerItem};
use crate::notifier::ItemDropEvent;
use crate::offsets::{d2client, unit};
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

    /// One BFS + marker reconciliation pass. No-op outside of a live game;
    /// clears markers when filtering is disabled.
    pub fn tick(&mut self) {
        let p_player = self
            .state
            .ctx
            .process
            .read_memory::<u32>(self.state.ctx.d2_client + d2client::PLAYER_UNIT)
            .unwrap_or(0);
        if p_player == 0 {
            return;
        }

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
        // Inner read held across the full BFS. Safe because `set_filter_config`
        // swaps the outer Arc instead of writing through this lock — don't add
        // a `filter_arc.write()` without revisiting.
        let filter = match filter_arc.read() {
            Ok(f) => f,
            Err(_) => return,
        };

        // Depth 10 reaches past what the engine typically keeps loaded; BFS
        // stops early when `ppRoomsNear` runs out.
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

        // Snapshot, then release the read lock — items thread mustn't block
        // on inserts during per-item decide() below.
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

        let injector = match self.state.injector.lock() {
            Ok(i) => i,
            Err(p) => p.into_inner(),
        };
        if let Err(e) = self.map_marker.tick(
            &self.state.ctx,
            &*injector,
            &newly_matched,
            &bfs_unit_ids,
            player_sub,
        ) {
            log_error(&format!("map_marker tick failed: {}", e));
        }
    }

    /// Drop all markers; called on game-entry transitions.
    pub fn clear(&mut self) {
        if let Err(e) = self.map_marker.clear(&self.state.ctx) {
            log_error(&format!("map_marker clear (game-entry) failed: {}", e));
        }
    }

    /// Drop all markers; called when D2 closes or the scanner stops.
    pub fn shutdown(&mut self) {
        if let Err(e) = self.map_marker.clear(&self.state.ctx) {
            log_error(&format!("map_marker clear on shutdown failed: {}", e));
        }
    }
}
