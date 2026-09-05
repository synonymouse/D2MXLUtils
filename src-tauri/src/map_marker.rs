//! Automap markers for loot-filter matches.
//!
//! Allocates `AutomapCell`s via `D2Injector::new_automap_cell` and attaches
//! our chain as a leaf of the layer's `pObjects` BST — walk `pLess` from
//! the root until a NULL slot, attach there, exactly like the engine's own
//! icon insertion. Earlier versions instead swapped `pObjects` itself to
//! point at a freshly-prepended chain; that's the single most contended
//! slot in the structure (the engine's own quest/shrine-icon placement
//! also reads/writes it), and repeatedly rewriting it every rebuild is the
//! prime suspect for reports of quest/shrine markers occasionally going
//! missing. Leaf insertion never touches the root, so engine-owned icons
//! are never at risk of being swapped out from under it.
//!
//! A per-area `persistent` cache keeps markers sticky when the player walks
//! past an item and its room unloads. Entries are evicted either when BFS
//! misses them AND the player is within `PICKUP_THRESHOLD_SUBTILES` (assumed
//! pickup), after `MARKER_TTL` without a BFS sighting (walked away and
//! never came back), or on a real area/act change (a player-subtile jump
//! past `AREA_CHANGE_JUMP_SUBTILES` — see its doc comment for why the
//! layer pointer alone can't be used to detect this). Cross-game
//! transitions are handled separately by the explicit `clear_markers`
//! signal from `main.rs`.
//!
//! Never mutate `pFloors` or `pWalls` — that corrupts revealed terrain.
//! See `docs/map-marker-reverse-engineering.md` for offset calibration.

#![cfg(any(target_os = "windows", target_os = "linux"))]

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use crate::injection::D2Injector;
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

/// Drop persistent entries unseen by BFS for this long. Caps the cache
/// in long sessions; cross-game wipe is handled by `clear_markers`.
pub const MARKER_TTL: Duration = Duration::from_secs(20 * 60);

/// Player-subtile jump above which a tick is treated as a true area/act
/// change (waypoint, portal, Act transition) rather than ordinary
/// movement — walking covers 2-3 subtiles per tick, even Teleport caps
/// around 16. `layer` (the automap layer pointer `tick` already tracks)
/// can't be used for this: it also flips on ordinary Room2 crossings
/// within the SAME area, which is exactly why the persistent cache isn't
/// wiped there. Without a real area-change signal, `persistent` was only
/// ever cleared by a brand-new game session (`clear_markers`, set once on
/// `ingame && !was_ingame`) — confirmed live: a marker placed in one
/// act's town kept reappearing (at stale, now-meaningless cell
/// coordinates) in the next act's town after a waypoint trip, since
/// nothing else cleared it in between.
pub const AREA_CHANGE_JUMP_SUBTILES: i32 = 60;

pub struct MapMarkerManager {
    last_layer: u32,
    /// Remote address of the slot whose value = our chain head — the
    /// `pLess` field of whatever existing leaf we attached under (or
    /// `layer + P_OBJECTS` itself, only when the tree was empty). Never
    /// rewritten to point elsewhere once picked; zero when not attached.
    chain_parent_slot: u32,
    /// Cells we've allocated, in chain order. `placed[0]` is the head; the
    /// last cell's `pLess` is 0 — we're a genuine leaf chain, not a splice
    /// back into the engine's tree (see `attach_chain`).
    placed: Vec<u32>,
    last_hash: u64,
    persistent: HashMap<u32, MarkerItem>,
    /// Last-stamp per unit_id for TTL. Kept in lockstep with `persistent`
    /// (orphans are GC'd at the end of `reconcile_persistent`).
    last_seen: HashMap<u32, Instant>,
    /// Last known player subtile position, tracked across ticks. Not
    /// reset by a `None` reading (e.g. mid-loading-screen) — we want to
    /// compare against the last *real* position once the player reappears,
    /// not treat "no reading yet" as a jump. See `AREA_CHANGE_JUMP_SUBTILES`.
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
            last_seen: HashMap::new(),
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
        self.last_seen.clear();
        self.last_layer = 0;
        self.last_player_sub = None;
        Ok(())
    }

    pub fn tick(
        &mut self,
        ctx: &D2Context,
        injector: &D2Injector,
        newly_matched: &[MarkerItem],
        explicitly_unmarked: &HashSet<u32>,
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

        // True area/act change: wipe the persistent cache itself, not just
        // the chain bookkeeping the layer-switch check below handles —
        // see AREA_CHANGE_JUMP_SUBTILES for why the layer pointer alone
        // can't be used for this. Compares against the last real position
        // regardless of how many `None` (out-of-world) ticks came between
        // — a loading screen doesn't itself reset `last_player_sub`.
        if let Some((px, py)) = player_sub {
            if is_area_change(self.last_player_sub, (px, py)) {
                self.persistent.clear();
                self.last_seen.clear();
            }
            self.last_player_sub = Some((px, py));
        }

        // Layer switch (Room2 crossing or layer reallocation): forget the
        // chain, keep the cache — reconcile will re-splice on the new layer.
        if layer != self.last_layer {
            self.chain_parent_slot = 0;
            self.placed.clear();
            self.last_hash = 0;
            self.last_layer = layer;
        }

        // Tamper check: if the slot we attached under (root or some
        // existing leaf's pLess) no longer points at our head, the engine
        // or MXL wrote through/past us and our chain is orphaned. Force
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
            &mut self.last_seen,
            newly_matched,
            explicitly_unmarked,
            bfs_unit_ids,
            player_sub,
            PICKUP_THRESHOLD_SUBTILES,
            MARKER_TTL,
            Instant::now(),
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

    /// Clear whatever leaf slot we attached under back to NULL. Since our
    /// chain is a genuine leaf (tail's `pLess` is 0, not spliced back into
    /// anything), this can never disconnect engine-owned nodes — unlike an
    /// earlier root-swap design, detaching us never touches the engine's
    /// own tree structure. No-op if someone else already overwrote the
    /// slot (tamper case, already handled by the caller).
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
            ctx.process
                .write_buffer(self.chain_parent_slot as usize, &0u32.to_le_bytes())?;
        }
        self.chain_parent_slot = 0;
        self.placed.clear();
        Ok(())
    }

    /// Allocate cells for `wanted` and attach them as a leaf hanging off
    /// the existing `pObjects` tree, per the engine's own verified
    /// insertion algorithm (walk `pLess` from the root until a NULL slot,
    /// attach there — "order is irrelevant, the renderer walks the entire
    /// tree"; see the map-marker RE notes). Earlier versions of this
    /// instead swapped `pObjects` itself to point at our new head — the
    /// single most contended slot in the structure, since the engine's own
    /// quest/shrine-icon insertion also reads/writes it — which is the
    /// prime suspect for those markers occasionally going missing. Leaf
    /// insertion touches only one `pLess` field deep in the tree, the same
    /// kind of write the engine's own insertion makes, so we're
    /// indistinguishable from one more native icon rather than a
    /// structural rewrite of the root every rebuild.
    fn attach_chain(
        &mut self,
        ctx: &D2Context,
        injector: &D2Injector,
        layer: u32,
        wanted: &[MarkerItem],
    ) -> Result<(), String> {
        let objects_slot = layer + automap_layer::P_OBJECTS as u32;

        let mut cells: Vec<u32> = Vec::with_capacity(wanted.len());
        for item in wanted {
            let cell = injector.new_automap_cell(&ctx.process)?;
            if cell == 0 {
                return Err("NewAutomapCell returned NULL".to_string());
            }
            write_cell_fields(ctx, cell, item.cell_x, item.cell_y)?;
            cells.push(cell);
        }

        // Chain our own cells together; the tail stays a real leaf
        // (pLess = 0, already zeroed by write_cell_fields) rather than
        // splicing back into the engine's tree.
        for i in 0..cells.len().saturating_sub(1) {
            let pless_slot = (cells[i] + automap_cell::P_LESS as u32) as usize;
            ctx.process
                .write_buffer(pless_slot, &cells[i + 1].to_le_bytes())?;
        }

        let attach_slot = find_leaf_slot(ctx, objects_slot)?;
        ctx.process
            .write_buffer(attach_slot as usize, &cells[0].to_le_bytes())?;

        self.chain_parent_slot = attach_slot;
        self.placed = cells;
        Ok(())
    }
}

/// Whether `current` is far enough from `last` (Manhattan distance in
/// subtiles) to only be explained by a waypoint/portal/Act transition —
/// see `AREA_CHANGE_JUMP_SUBTILES`. `last` being `None` (no prior reading
/// yet, e.g. right after `clear()`) is never treated as a jump. Pure so
/// it can be unit-tested without a live process.
fn is_area_change(last: Option<(i32, i32)>, current: (i32, i32)) -> bool {
    let Some((lx, ly)) = last else {
        return false;
    };
    let (px, py) = current;
    let jump = (px - lx).abs() + (py - ly).abs();
    jump >= AREA_CHANGE_JUMP_SUBTILES
}

/// Reconcile `persistent` against this tick's BFS results. Pure so it can
/// be unit-tested without a live process. Returns the sorted list of
/// markers to render (deterministic for stable hashing).
fn reconcile_persistent(
    persistent: &mut HashMap<u32, MarkerItem>,
    last_seen: &mut HashMap<u32, Instant>,
    newly_matched: &[MarkerItem],
    explicitly_unmarked: &HashSet<u32>,
    bfs_unit_ids: &HashSet<u32>,
    player_sub: Option<(i32, i32)>,
    pickup_threshold: i32,
    ttl: Duration,
    now: Instant,
) -> Vec<MarkerItem> {
    persistent.retain(|uid, _| !explicitly_unmarked.contains(uid));

    for m in newly_matched {
        persistent.insert(m.unit_id, *m);
        last_seen.insert(m.unit_id, now);
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

    persistent.retain(|uid, _| match last_seen.get(uid) {
        Some(t) => now.duration_since(*t) < ttl,
        None => false,
    });
    last_seen.retain(|uid, _| persistent.contains_key(uid));

    let mut out: Vec<MarkerItem> = persistent.values().copied().collect();
    out.sort_by_key(|m| (m.unit_id, m.cell_x, m.cell_y));
    out
}

/// BFS the Room1 graph up to `max_depth` hops collecting every item unit,
/// returning `(p_unit, sub_x, sub_y)` triples.
pub fn bfs_item_positions(ctx: &D2Context, max_depth: u32) -> Result<Vec<(u32, i32, i32)>, String> {
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

/// Walk `pLess` from `root_slot` (either `layer + P_OBJECTS` itself, or
/// some existing cell's `pLess` field) until finding a NULL child slot,
/// returning that slot's address — matches the engine's own documented
/// insertion algorithm exactly, so our cells land wherever the engine's
/// own icon insertion would have put a new leaf. Bounded to guard against
/// a corrupted/cyclic tree; matches the depth bound style used by the BFS
/// scanner elsewhere in this module.
fn find_leaf_slot(ctx: &D2Context, root_slot: u32) -> Result<u32, String> {
    let mut slot = root_slot;
    for _ in 0..4096 {
        let node = ctx.process.read_memory::<u32>(slot as usize).unwrap_or(0);
        if node == 0 {
            return Ok(slot);
        }
        slot = node + automap_cell::P_LESS as u32;
    }
    Err("find_leaf_slot: pObjects tree exceeds depth bound (corrupted?)".to_string())
}

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
    fn is_area_change_ignores_ordinary_movement() {
        assert!(!is_area_change(Some((100, 100)), (102, 101))); // walking
        assert!(!is_area_change(Some((100, 100)), (110, 105))); // teleport
    }

    #[test]
    fn is_area_change_detects_waypoint_jump() {
        assert!(is_area_change(Some((100, 100)), (200, 200)));
        assert!(is_area_change(Some((100, 100)), (100, 160))); // exactly at threshold
    }

    #[test]
    fn is_area_change_false_with_no_prior_reading() {
        assert!(!is_area_change(None, (5000, 5000)));
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
        let mut last_seen = HashMap::new();
        let matched = [mk(1, 50, 50), mk(2, 60, 60)];
        let bfs: HashSet<u32> = [1u32, 2].iter().copied().collect();
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &matched,
            &HashSet::new(),
            &bfs,
            Some((55, 55)),
            32,
            Duration::from_secs(3600),
            Instant::now(),
        );
        assert_eq!(out.len(), 2);
        assert_eq!(persistent.len(), 2);
    }

    #[test]
    fn reconcile_keeps_far_cached_when_bfs_misses() {
        // Player at origin, item far away, BFS doesn't see → walked away.
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 200, 200));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((0, 0)),
            32,
            Duration::from_secs(3600),
            now,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn reconcile_evicts_close_cached_when_bfs_misses() {
        // Player next to item, BFS doesn't see → picked up.
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 55, 55));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((50, 50)),
            32,
            Duration::from_secs(3600),
            now,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn reconcile_evicts_when_bfs_sees_but_filter_no_longer_matches() {
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 55, 55));
        let mut last_seen = HashMap::new();
        let now = Instant::now();
        last_seen.insert(42u32, now);

        let mut bfs = HashSet::new();
        bfs.insert(42u32);
        let explicitly_unmarked = bfs.clone();

        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &[],
            &explicitly_unmarked,
            &bfs,
            Some((50, 50)),
            32,
            Duration::from_secs(3600),
            now,
        );
        assert!(out.is_empty());
        assert!(persistent.is_empty());
    }

    #[test]
    fn reconcile_updates_position_on_reupsert() {
        // unit_id reused for a new drop at a different spot.
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 10, 10));
        last_seen.insert(42u32, now);
        let matched = [mk(42, 200, 200)];
        let bfs: HashSet<u32> = [42u32].iter().copied().collect();
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &matched,
            &HashSet::new(),
            &bfs,
            Some((100, 100)),
            32,
            Duration::from_secs(3600),
            now,
        );
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].sub_x, out[0].sub_y), (200, 200));
    }

    #[test]
    fn reconcile_keeps_everything_when_player_pos_unknown() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 50, 50));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            None,
            32,
            Duration::from_secs(3600),
            now,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn reconcile_evicts_after_ttl() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let early = Instant::now();
        persistent.insert(42u32, mk(42, 200, 200));
        last_seen.insert(42u32, early);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((0, 0)),
            32,
            Duration::from_secs(60),
            early + Duration::from_secs(120),
        );
        assert!(out.is_empty());
        assert!(persistent.is_empty());
        assert!(last_seen.is_empty());
    }
}
