//! Automap markers for loot-filter matches.
//!
//! Allocates `AutomapCell`s via `D2Injector::new_automap_cell` and prepends
//! our chain at the root of the layer's `pObjects` BST. Engine-owned icons
//! (quests, waypoints) stay intact, linked below our tail.
//!
//! A per-area `persistent` cache keeps markers sticky when the player walks
//! past an item and its room unloads. Entries are evicted only when BFS
//! misses them AND the player is within `PICKUP_THRESHOLD_SUBTILES` —
//! assumed pickup rather than out-of-range.
//!
//! Area change is detected via player-subtile jump: a hop of
//! `AREA_CHANGE_JUMP_SUBTILES` or more can only come from a waypoint /
//! portal / Act transition. `AutomapLayer.nLayerNo` flips on every Room2
//! crossing and is NOT an area id.
//!
//! Never mutate `pFloors` or `pWalls` — that corrupts revealed terrain.
//! See `docs/map-marker-reverse-engineering.md` for offset calibration.

#![cfg(target_os = "windows")]

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use crate::injection::D2Injector;
use crate::logger::info as log_info;
use crate::offsets::{
    automap_cell, automap_layer, d2client, item_path, paths, player_path, room1, unit, unit_type,
};
use crate::process::D2Context;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MarkerItem {
    pub unit_id: u32,
    pub cell_x: i32,
    pub cell_y: i32,
    pub sub_x: i32,
    pub sub_y: i32,
}

/// Missing-from-BFS markers within this Manhattan subtile distance of the
/// player count as "picked up" (room is loaded so BFS would have seen it).
/// One D2 screen ≈ 32 subtiles.
pub const PICKUP_THRESHOLD_SUBTILES: i32 = 32;

/// Player subtile jump above which we treat the tick as a true area change.
/// Walking moves 2-3 subtiles; even Teleport caps around 16.
pub const AREA_CHANGE_JUMP_SUBTILES: i32 = 60;

pub struct MapMarkerManager {
    last_layer: u32,
    /// Remote address of the slot whose value = our chain head. Always
    /// `layer + P_OBJECTS` (we prepend at root); zero when not attached.
    chain_parent_slot: u32,
    /// Cells we've allocated, in chain order. `placed[0]` is the head; the
    /// last cell's `pLess` points at the engine's original tree.
    placed: Vec<u32>,
    last_hash: u64,
    persistent: HashMap<u32, MarkerItem>,
    last_player_sub: Option<(i32, i32)>,
}

impl MapMarkerManager {
    pub fn new() -> Self {
        Self {
            last_layer: 0,
            chain_parent_slot: 0,
            placed: Vec::new(),
            last_hash: 0,
            persistent: HashMap::new(),
            last_player_sub: None,
        }
    }

    /// Detach our chain and wipe all state (cache + bookkeeping). Safe to
    /// call repeatedly.
    pub fn clear(&mut self, ctx: &D2Context) -> Result<(), String> {
        let layer = read_layer(ctx).unwrap_or(0);
        if layer != 0 && layer == self.last_layer {
            let _ = self.detach_chain(ctx);
        }
        self.chain_parent_slot = 0;
        self.placed.clear();
        self.last_hash = 0;
        self.persistent.clear();
        self.last_layer = 0;
        self.last_player_sub = None;
        Ok(())
    }

    pub fn tick(
        &mut self,
        ctx: &D2Context,
        injector: &D2Injector,
        newly_matched: &[MarkerItem],
        bfs_unit_ids: &HashSet<u32>,
        player_sub: Option<(i32, i32)>,
    ) -> Result<(), String> {
        let layer = read_layer(ctx)?;
        if layer == 0 {
            // Out of game / loading screen — forget the chain but keep the
            // cache: we're likely just between screens.
            self.last_layer = 0;
            self.chain_parent_slot = 0;
            self.placed.clear();
            self.last_hash = 0;
            return Ok(());
        }

        // Area change: a big player-subtile jump is the only reliable
        // signal (nLayerNo and layer pointer both flip on Room2 crossings).
        if let (Some((px, py)), Some((lx, ly))) = (player_sub, self.last_player_sub) {
            let jump = (px - lx).abs() + (py - ly).abs();
            if jump >= AREA_CHANGE_JUMP_SUBTILES && !self.persistent.is_empty() {
                log_info(&format!(
                    "map_marker: area change (player jumped {} subtiles), wiping {} markers",
                    jump,
                    self.persistent.len()
                ));
                self.persistent.clear();
                self.chain_parent_slot = 0;
                self.placed.clear();
                self.last_hash = 0;
            }
        }

        // Layer switch (Room2 crossing or layer reallocation): forget the
        // chain, keep the cache — reconcile will re-splice on the new layer.
        if layer != self.last_layer {
            self.chain_parent_slot = 0;
            self.placed.clear();
            self.last_hash = 0;
            self.last_layer = layer;
        }

        self.last_player_sub = player_sub;

        // Tamper check: if `pObjects` no longer points at our head, the
        // engine or MXL overwrote us and our chain is orphaned. Force
        // rebuild.
        if self.chain_parent_slot != 0 {
            if let Some(&head) = self.placed.first() {
                let current = ctx
                    .process
                    .read_memory::<u32>(self.chain_parent_slot as usize)
                    .unwrap_or(0);
                if current != head {
                    self.chain_parent_slot = 0;
                    self.placed.clear();
                    self.last_hash = 0;
                }
            }
        }

        let wanted = reconcile_persistent(
            &mut self.persistent,
            newly_matched,
            bfs_unit_ids,
            player_sub,
            PICKUP_THRESHOLD_SUBTILES,
        );

        let hash = hash_markers(&wanted);
        if hash == self.last_hash && self.chain_parent_slot != 0 {
            return Ok(());
        }
        if wanted.is_empty() && self.chain_parent_slot == 0 {
            self.last_hash = hash;
            return Ok(());
        }

        self.detach_chain(ctx)?;

        if wanted.is_empty() {
            self.last_hash = hash;
            return Ok(());
        }

        self.attach_chain(ctx, injector, layer, &wanted)?;
        self.last_hash = hash;
        Ok(())
    }

    /// Restore `pObjects` to whatever our tail was linking to (= engine's
    /// tree). No-op if we aren't actually at root (someone replaced us).
    fn detach_chain(&mut self, ctx: &D2Context) -> Result<(), String> {
        if self.chain_parent_slot == 0 || self.placed.is_empty() {
            self.chain_parent_slot = 0;
            self.placed.clear();
            return Ok(());
        }
        let current = ctx
            .process
            .read_memory::<u32>(self.chain_parent_slot as usize)
            .unwrap_or(0);
        if current == self.placed[0] {
            let tail = *self.placed.last().unwrap();
            let tail_pless = ctx
                .process
                .read_memory::<u32>((tail + automap_cell::P_LESS as u32) as usize)
                .unwrap_or(0);
            ctx.process
                .write_buffer(self.chain_parent_slot as usize, &tail_pless.to_le_bytes())?;
        }
        self.chain_parent_slot = 0;
        self.placed.clear();
        Ok(())
    }

    /// Allocate cells for `wanted` and prepend them at `pObjects` root.
    /// Our tail's `pLess` points at the engine's previous root so existing
    /// quest/waypoint icons stay alive.
    fn attach_chain(
        &mut self,
        ctx: &D2Context,
        injector: &D2Injector,
        layer: u32,
        wanted: &[MarkerItem],
    ) -> Result<(), String> {
        let objects_slot = layer + automap_layer::P_OBJECTS as u32;
        let old_root = ctx
            .process
            .read_memory::<u32>(objects_slot as usize)
            .unwrap_or(0);

        let mut cells: Vec<u32> = Vec::with_capacity(wanted.len());
        for item in wanted {
            let cell = injector.new_automap_cell(&ctx.process)?;
            if cell == 0 {
                return Err("NewAutomapCell returned NULL".to_string());
            }
            write_cell_fields(ctx, cell, item.cell_x, item.cell_y)?;
            cells.push(cell);
        }

        for i in 0..cells.len() {
            let next = if i + 1 < cells.len() {
                cells[i + 1]
            } else {
                old_root
            };
            let pless_slot = (cells[i] + automap_cell::P_LESS as u32) as usize;
            ctx.process.write_buffer(pless_slot, &next.to_le_bytes())?;
        }

        ctx.process
            .write_buffer(objects_slot as usize, &cells[0].to_le_bytes())?;

        self.chain_parent_slot = objects_slot;
        self.placed = cells;
        Ok(())
    }
}

/// Reconcile `persistent` against this tick's BFS results. Pure so it can
/// be unit-tested without a live process. Returns the sorted list of
/// markers to render (deterministic for stable hashing).
fn reconcile_persistent(
    persistent: &mut HashMap<u32, MarkerItem>,
    newly_matched: &[MarkerItem],
    bfs_unit_ids: &HashSet<u32>,
    player_sub: Option<(i32, i32)>,
    pickup_threshold: i32,
) -> Vec<MarkerItem> {
    let matched_ids: HashSet<u32> = newly_matched.iter().map(|m| m.unit_id).collect();

    // BFS sees it, filter no longer matches → drop.
    persistent.retain(|uid, _| !bfs_unit_ids.contains(uid) || matched_ids.contains(uid));

    // Upsert current matches.
    for m in newly_matched {
        persistent.insert(m.unit_id, *m);
    }

    // Close + invisible = picked up.
    if let Some((px, py)) = player_sub {
        persistent.retain(|uid, cached| {
            if bfs_unit_ids.contains(uid) {
                return true;
            }
            let dist = (cached.sub_x - px).abs() + (cached.sub_y - py).abs();
            dist >= pickup_threshold
        });
    }

    let mut out: Vec<MarkerItem> = persistent.values().copied().collect();
    out.sort_by_key(|m| (m.unit_id, m.cell_x, m.cell_y));
    out
}

/// BFS the Room1 graph up to `max_depth` hops collecting every item unit,
/// returning `(p_unit, sub_x, sub_y)` triples.
pub fn bfs_item_positions(
    ctx: &D2Context,
    max_depth: u32,
) -> Result<Vec<(u32, i32, i32)>, String> {
    let mut out = Vec::new();

    let p_player = ctx
        .process
        .read_memory::<u32>(ctx.d2_client + d2client::PLAYER_UNIT)?;
    if p_player == 0 {
        return Ok(out);
    }
    let p_path = ctx
        .process
        .read_memory::<u32>(p_player as usize + paths::TO_PATHS_PTR[1])?;
    if p_path == 0 {
        return Ok(out);
    }
    let p_room1 = ctx
        .process
        .read_memory::<u32>(p_path as usize + paths::TO_PATHS_PTR[2])?;
    if p_room1 == 0 {
        return Ok(out);
    }

    let mut visited: HashSet<u32> = HashSet::new();
    visited.insert(p_room1);
    let mut frontier: Vec<u32> = vec![p_room1];

    for depth in 0..max_depth {
        let mut next_frontier: Vec<u32> = Vec::new();

        for &room in &frontier {
            let mut p_unit = ctx
                .process
                .read_memory::<u32>(room as usize + room1::UNIT_FIRST)
                .unwrap_or(0);
            // Bound in case of corrupted list.
            for _ in 0..4096 {
                if p_unit == 0 {
                    break;
                }
                let utype = ctx
                    .process
                    .read_memory::<u32>(p_unit as usize + unit::UNIT_TYPE)
                    .unwrap_or(u32::MAX);
                if utype == unit_type::ITEM {
                    let pp = ctx
                        .process
                        .read_memory::<u32>(p_unit as usize + unit::PATH)
                        .unwrap_or(0);
                    if pp != 0 {
                        let sx = ctx
                            .process
                            .read_memory::<u32>(pp as usize + item_path::SUB_X)
                            .unwrap_or(0) as i32;
                        let sy = ctx
                            .process
                            .read_memory::<u32>(pp as usize + item_path::SUB_Y)
                            .unwrap_or(0) as i32;
                        if sx > 0 && sy > 0 {
                            out.push((p_unit, sx, sy));
                        }
                    }
                }
                // pRoomNext, NOT pListNext — the latter leaves the room.
                p_unit = ctx
                    .process
                    .read_memory::<u32>(p_unit as usize + unit::ROOM_NEXT)
                    .unwrap_or(0);
            }

            if depth + 1 < max_depth {
                let pp_near = ctx
                    .process
                    .read_memory::<u32>(room as usize + room1::PP_ROOMS_NEAR)
                    .unwrap_or(0);
                let n_near = ctx
                    .process
                    .read_memory::<u32>(room as usize + room1::DW_ROOMS_NEAR)
                    .unwrap_or(0);
                if pp_near != 0 {
                    let n = n_near.min(1024);
                    for i in 0..n {
                        let near = ctx
                            .process
                            .read_memory::<u32>(pp_near as usize + 4 * i as usize)
                            .unwrap_or(0);
                        if near != 0 && visited.insert(near) {
                            next_frontier.push(near);
                        }
                    }
                }
            }
        }

        frontier = next_frontier;
        if frontier.is_empty() {
            break;
        }
    }

    Ok(out)
}

/// Player's current subtile position, or `None` when out of world.
pub fn read_player_subtile(ctx: &D2Context) -> Option<(i32, i32)> {
    let p_player = ctx
        .process
        .read_memory::<u32>(ctx.d2_client + d2client::PLAYER_UNIT)
        .ok()?;
    if p_player == 0 {
        return None;
    }
    let p_path = ctx
        .process
        .read_memory::<u32>(p_player as usize + unit::PATH)
        .ok()?;
    if p_path == 0 {
        return None;
    }
    let sx = ctx
        .process
        .read_memory::<u16>(p_path as usize + player_path::SUB_X)
        .ok()? as i32;
    let sy = ctx
        .process
        .read_memory::<u16>(p_path as usize + player_path::SUB_Y)
        .ok()? as i32;
    Some((sx, sy))
}

/// World subtile → automap cell-space. Calibrated per the RE doc; X fits
/// exactly, Y has ≤5 units of rounding residual.
pub fn sub_to_cell(sub_x: i32, sub_y: i32) -> (i32, i32) {
    let cx = (((sub_x - sub_y) as f64) * 8.0 / 5.0).round() as i32;
    let cy = (((sub_x + sub_y) as f64) * 4.0 / 5.0).round() as i32;
    (cx, cy)
}

// ---------- internal helpers ----------

fn read_layer(ctx: &D2Context) -> Result<u32, String> {
    ctx.process
        .read_memory::<u32>(ctx.d2_client + d2client::AUTOMAP_LAYER)
}

fn write_cell_fields(ctx: &D2Context, cell: u32, cell_x: i32, cell_y: i32) -> Result<(), String> {
    let mut buf = [0u8; automap_cell::SIZE];
    buf[automap_cell::F_SAVED..automap_cell::F_SAVED + 4].copy_from_slice(&1u32.to_le_bytes());
    buf[automap_cell::N_CELL_NO..automap_cell::N_CELL_NO + 2]
        .copy_from_slice(&automap_cell::CROSS_CELL_NO.to_le_bytes());
    buf[automap_cell::X_PIXEL..automap_cell::X_PIXEL + 2]
        .copy_from_slice(&(cell_x as i16 as u16).to_le_bytes());
    buf[automap_cell::Y_PIXEL..automap_cell::Y_PIXEL + 2]
        .copy_from_slice(&(cell_y as i16 as u16).to_le_bytes());
    // wWeight / pLess / pMore already zero in buf.
    ctx.process.write_buffer(cell as usize, &buf)
}

fn hash_markers(items: &[MarkerItem]) -> u64 {
    // Sort before hashing so iteration-order jitter doesn't trip the hash-gate.
    let mut v: Vec<MarkerItem> = items.to_vec();
    v.sort_by_key(|m| (m.unit_id, m.cell_x, m.cell_y));
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(uid: u32, sx: i32, sy: i32) -> MarkerItem {
        let (cx, cy) = sub_to_cell(sx, sy);
        MarkerItem {
            unit_id: uid,
            cell_x: cx,
            cell_y: cy,
            sub_x: sx,
            sub_y: sy,
        }
    }

    #[test]
    fn sub_to_cell_matches_formula() {
        assert_eq!(sub_to_cell(0, 0), (0, 0));
        assert_eq!(sub_to_cell(5, 5), (0, 8));
        assert_eq!(sub_to_cell(10, 5), (8, 12));
        assert_eq!(sub_to_cell(1, 0), (2, 1));
    }

    #[test]
    fn hash_ignores_order_but_reflects_set_and_coords() {
        let a = [mk(1, 10, 20), mk(2, 30, 40)];
        let same_set_diff_order = [mk(2, 30, 40), mk(1, 10, 20)];
        assert_eq!(hash_markers(&a), hash_markers(&same_set_diff_order));

        let diff_coords = [mk(1, 11, 20), mk(2, 30, 40)];
        assert_ne!(hash_markers(&a), hash_markers(&diff_coords));

        let diff_set = [mk(1, 10, 20), mk(2, 30, 40), mk(3, 50, 50)];
        assert_ne!(hash_markers(&a), hash_markers(&diff_set));
    }

    #[test]
    fn reconcile_upserts_new_matches() {
        let mut persistent = HashMap::new();
        let matched = [mk(1, 50, 50), mk(2, 60, 60)];
        let bfs: HashSet<u32> = [1u32, 2].iter().copied().collect();
        let out = reconcile_persistent(&mut persistent, &matched, &bfs, Some((55, 55)), 32);
        assert_eq!(out.len(), 2);
        assert_eq!(persistent.len(), 2);
    }

    #[test]
    fn reconcile_keeps_far_cached_when_bfs_misses() {
        // Player at origin, item far away, BFS doesn't see → walked away.
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 200, 200));
        let out = reconcile_persistent(&mut persistent, &[], &HashSet::new(), Some((0, 0)), 32);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn reconcile_evicts_close_cached_when_bfs_misses() {
        // Player next to item, BFS doesn't see → picked up.
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 55, 55));
        let out = reconcile_persistent(&mut persistent, &[], &HashSet::new(), Some((50, 50)), 32);
        assert!(out.is_empty());
    }

    #[test]
    fn reconcile_evicts_when_bfs_sees_but_filter_no_longer_matches() {
        // Rule changed mid-session: item still visible but shouldn't be marked.
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 55, 55));
        let bfs: HashSet<u32> = [42u32].iter().copied().collect();
        let out = reconcile_persistent(&mut persistent, &[], &bfs, Some((50, 50)), 32);
        assert!(out.is_empty());
    }

    #[test]
    fn reconcile_updates_position_on_reupsert() {
        // unit_id reused for a new drop at a different spot.
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 10, 10));
        let matched = [mk(42, 200, 200)];
        let bfs: HashSet<u32> = [42u32].iter().copied().collect();
        let out = reconcile_persistent(&mut persistent, &matched, &bfs, Some((100, 100)), 32);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].sub_x, out[0].sub_y), (200, 200));
    }

    #[test]
    fn reconcile_keeps_everything_when_player_pos_unknown() {
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 50, 50));
        let out = reconcile_persistent(&mut persistent, &[], &HashSet::new(), None, 32);
        assert_eq!(out.len(), 1);
    }
}
