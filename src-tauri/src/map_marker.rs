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

/// Maximum number of automap marker cells to allocate and place simultaneously.
/// Prevents runaway cell allocation when there are hundreds of filtered drops,
/// avoiding memory exhaustion in Diablo II's 32-bit address space.
pub const MAX_MARKER_CELLS: usize = 100;

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
    /// Detached cells available for reuse across ticks and room transitions.
    /// Never free individual cells: the game owns their backing pool.
    spare_cells: Vec<u32>,
    cells_trusted: bool,
    last_hash: u64,
    persistent: HashMap<u32, MarkerItem>,
    /// Last-stamp per unit_id for TTL. Kept in lockstep with `persistent`
    /// (orphans are GC'd at the end of `reconcile_persistent`).
    last_seen: HashMap<u32, Instant>,
    /// First-stamp per unit_id, set once and never refreshed — unlike
    /// `last_seen` (which every BFS-confirmed sighting bumps to `now`,
    /// making it useless for telling old drops from new ones while both
    /// are still on the ground). Used only to pick which markers to keep
    /// when `persistent` exceeds `MAX_MARKER_CELLS`: evict the oldest
    /// first-seen entries so new drops always get a slot instead of the
    /// same low-unit_id batch camping there forever. Evicted IDs keep their
    /// stamp only while BFS-visible; storage is bounded by the current BFS
    /// snapshot plus the capped persistent set, not session history.
    first_seen: HashMap<u32, Instant>,
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
            spare_cells: Vec::new(),
            cells_trusted: true,
            last_hash: 0,
            persistent: HashMap::new(),
            last_seen: HashMap::new(),
            first_seen: HashMap::new(),
            last_player_sub: None,
        }
    }

    /// Logical map-off: detach and retain the live pool's high-water cells.
    pub fn clear(&mut self, ctx: &D2Context) -> Result<(), String> {
        self.persistent.clear();
        self.last_seen.clear();
        self.first_seen.clear();
        self.last_player_sub = None;
        let layer = read_layer(ctx)?;
        if layer == 0 {
            self.invalidate_cells();
        } else {
            self.require_trusted_cells()?;
            self.detach_chain(ctx)?;
        }
        self.last_hash = 0;
        self.last_layer = 0;
        Ok(())
    }

    /// The marker scanner can observe loading before tick() runs. Forget
    /// addresses without touching memory the game may already have freed.
    pub fn invalidate_cells(&mut self) {
        self.last_layer = 0;
        self.chain_parent_slot = 0;
        self.placed.clear();
        self.spare_cells.clear();
        self.last_hash = 0;
        self.cells_trusted = true;
    }

    pub fn reset_session(&mut self) {
        self.invalidate_cells();
        self.persistent.clear();
        self.last_seen.clear();
        self.first_seen.clear();
        self.last_player_sub = None;
    }

    fn require_trusted_cells(&self) -> Result<(), String> {
        if self.cells_trusted {
            Ok(())
        } else {
            Err("Automap cell ownership uncertain; waiting for lifecycle invalidation".to_string())
        }
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
            self.invalidate_cells();
            return Ok(());
        }
        self.require_trusted_cells()?;

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
                self.first_seen.clear();
                self.invalidate_cells();
            }
            self.last_player_sub = Some((px, py));
        }

        // Layer switch (Room2 crossing or layer reallocation): detach from
        // the old layer and keep existing cells in spare_cells for reuse.
        // AutomapCell allocations come from a global pool (D2Client NewAutomapCell),
        // not the individual layer, so discarding spare_cells here would permanently
        // leak memory in Game.exe every time the player crosses rooms.
        if layer != self.last_layer {
            self.detach_chain(ctx)?;
            self.chain_parent_slot = 0;
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
                    .read_memory::<u32>(self.chain_parent_slot as usize)?;
                if current != head {
                    self.cells_trusted = false;
                    return self.require_trusted_cells();
                }
            }
        }

        let wanted = reconcile_persistent(
            &mut self.persistent,
            &mut self.last_seen,
            &mut self.first_seen,
            newly_matched,
            explicitly_unmarked,
            bfs_unit_ids,
            player_sub,
            PICKUP_THRESHOLD_SUBTILES,
            MARKER_TTL,
            MAX_MARKER_CELLS,
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

    /// Recycle only a chain that is still exactly ours. If the engine added
    /// children or replaced a link, leave that tree alone and quarantine all
    /// addresses instead of disconnecting or rewriting foreign nodes.
    fn detach_chain(&mut self, ctx: &D2Context) -> Result<(), String> {
        if self.chain_parent_slot == 0 || self.placed.is_empty() {
            return Ok(());
        }
        let current = ctx
            .process
            .read_memory::<u32>(self.chain_parent_slot as usize)?;
        let intact = current == self.placed[0]
            && chain_is_intact(&self.placed, |cell| {
                let less = ctx
                    .process
                    .read_memory::<u32>(cell as usize + automap_cell::P_LESS)?;
                let more = ctx
                    .process
                    .read_memory::<u32>(cell as usize + automap_cell::P_MORE)?;
                Ok((less, more))
            })?;
        if intact {
            self.cells_trusted = false;
            ctx.process
                .write_buffer(self.chain_parent_slot as usize, &0u32.to_le_bytes())?;
            self.cells_trusted = true;
            self.spare_cells.append(&mut self.placed);
        } else {
            self.cells_trusted = false;
            return self.require_trusted_cells();
        }
        self.chain_parent_slot = 0;
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
        let wanted = if wanted.len() > MAX_MARKER_CELLS {
            &wanted[..MAX_MARKER_CELLS]
        } else {
            wanted
        };
        if wanted.is_empty() {
            return Ok(());
        }

        let objects_slot = layer + automap_layer::P_OBJECTS as u32;

        // Retain newly allocated cells even if a later allocation/write fails.
        // Normal rebuilds only grow to the high-water mark, rather than
        // allocating the entire surviving marker set on every item change.
        grow_cell_pool(&mut self.spare_cells, wanted.len(), || {
            injector.new_automap_cell(&ctx.process).inspect_err(|_| {
                self.cells_trusted = false;
            })
        })?;
        self.cells_trusted = false;
        if read_layer(ctx)? != layer {
            return Err("Automap layer changed during marker allocation".to_string());
        }
        let cells = &self.spare_cells[..wanted.len()];
        for (item, &cell) in wanted.iter().zip(cells) {
            write_cell_fields(ctx, cell, item.cell_x, item.cell_y)?;
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
        let head = cells[0];
        if read_layer(ctx)? != layer {
            return Err("Automap layer changed before marker publication".to_string());
        }
        if ctx.process.read_memory::<u32>(attach_slot as usize)? != 0 {
            return Err("Automap attach slot changed before marker publication".to_string());
        }
        // A failed write may still have published the pointer. Do not offer
        // those cells for reuse after an ambiguous publication failure.
        self.placed = self.spare_cells.drain(..wanted.len()).collect();
        self.chain_parent_slot = attach_slot;
        ctx.process
            .write_buffer(attach_slot as usize, &head.to_le_bytes())?;
        self.cells_trusted = true;
        Ok(())
    }
}

// These helpers share the production allocation/ownership decisions with
// tests, without requiring a live game process.
fn grow_cell_pool(
    cells: &mut Vec<u32>,
    needed: usize,
    mut allocate: impl FnMut() -> Result<u32, String>,
) -> Result<(), String> {
    let needed = needed.min(MAX_MARKER_CELLS);
    while cells.len() < needed {
        let cell = allocate()?;
        if cell == 0 {
            return Err("NewAutomapCell returned NULL".to_string());
        }
        cells.push(cell);
    }
    // Keep every chain ordered by address. A renderer can still hold the old
    // detached head while we rewrite cells; retaining this order prevents
    // transient back-links/cycles as old and new links overlap.
    cells.sort_unstable();
    Ok(())
}

fn chain_is_intact(
    cells: &[u32],
    mut read_links: impl FnMut(u32) -> Result<(u32, u32), String>,
) -> Result<bool, String> {
    for (i, &cell) in cells.iter().enumerate() {
        let (less, more) = read_links(cell)?;
        if less != cells.get(i + 1).copied().unwrap_or(0) || more != 0 {
            return Ok(false);
        }
    }
    Ok(true)
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
/// markers to render (deterministic for stable hashing), capped at
/// `max_markers` by evicting the oldest-dropped entries first — see
/// `first_seen`'s doc comment for why `last_seen` can't be used for this.
fn reconcile_persistent(
    persistent: &mut HashMap<u32, MarkerItem>,
    last_seen: &mut HashMap<u32, Instant>,
    first_seen: &mut HashMap<u32, Instant>,
    newly_matched: &[MarkerItem],
    explicitly_unmarked: &HashSet<u32>,
    bfs_unit_ids: &HashSet<u32>,
    player_sub: Option<(i32, i32)>,
    pickup_threshold: i32,
    ttl: Duration,
    max_markers: usize,
    now: Instant,
) -> Vec<MarkerItem> {
    persistent.retain(|uid, _| !explicitly_unmarked.contains(uid));

    for m in newly_matched {
        persistent.insert(m.unit_id, *m);
        last_seen.insert(m.unit_id, now);
        first_seen.entry(m.unit_id).or_insert(now);
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
    first_seen.retain(|uid, _| {
        persistent.contains_key(uid)
            || (bfs_unit_ids.contains(uid) && !explicitly_unmarked.contains(uid))
    });

    // Over the marker-cell cap: keep the most recently dropped `max_markers`
    // entries, evicting the rest. Without this, a persistent set that grows
    // past the cap (nothing here evicts by count on its own) always loses
    // the same oldest-unit_id batch to `attach_chain`'s own truncation,
    // permanently blocking every later drop from ever getting a marker.
    if persistent.len() > max_markers {
        let mut by_age: Vec<(u32, Instant)> = first_seen
            .iter()
            .filter(|(uid, _)| persistent.contains_key(uid))
            .map(|(&uid, &stamp)| (uid, stamp))
            .collect();
        by_age.sort_unstable_by_key(|&(uid, t)| (t, uid));
        for &(uid, _) in by_age.iter().take(by_age.len() - max_markers) {
            persistent.remove(&uid);
            last_seen.remove(&uid);
        }
    }

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
        let node = ctx.process.read_memory::<u32>(slot as usize)?;
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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn full_snapshots_keep_stable_markers_and_bounded_identity_stamps() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let start = Instant::now();
        for batch in 0..100u32 {
            let matched: Vec<_> = (batch * 101..(batch + 1) * 101)
                .map(|uid| mk(uid, 100, 100))
                .collect();
            let bfs = matched.iter().map(|item| item.unit_id).collect();
            let mut previous = None;
            for repeat in 0..3 {
                let wanted = reconcile_persistent(
                    &mut persistent,
                    &mut last_seen,
                    &mut first_seen,
                    &matched,
                    &HashSet::new(),
                    &bfs,
                    None,
                    32,
                    MARKER_TTL,
                    MAX_MARKER_CELLS,
                    start + Duration::from_secs(u64::from(batch * 3 + repeat)),
                );
                if let Some(previous) = previous {
                    assert_eq!(wanted, previous);
                }
                assert_eq!(wanted.len(), MAX_MARKER_CELLS);
                assert!(first_seen.len() <= bfs.len() + MAX_MARKER_CELLS);
                previous = Some(wanted);
            }
        }
    }

    #[cfg(target_os = "windows")]
    pub(crate) mod native {
        use super::*;
        use crate::process::{
            marker_test_io::{Operation, Scope},
            ProcessHandle,
        };
        use windows::Win32::System::{
            Memory::{
                VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
            },
            Threading::{
                GetCurrentProcessId, OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ,
                PROCESS_VM_WRITE,
            },
        };

        pub(crate) struct Fixture {
            pub ctx: D2Context,
            region: std::ptr::NonNull<std::ffi::c_void>,
            pub io: Scope,
        }
        impl Fixture {
            pub fn new() -> Self {
                // SAFETY: GetCurrentProcessId takes no pointers and has no preconditions.
                let pid = unsafe { GetCurrentProcessId() };
                // SAFETY: the PID is current and these rights are used only on fixture-owned pages.
                let handle = unsafe {
                    OpenProcess(
                        PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                        false,
                        pid,
                    )
                }
                .unwrap();
                let process = ProcessHandle { handle, pid };
                let region = (0x10000000usize..0x70000000)
                    .step_by(0x200000)
                    .find_map(|base| {
                        // SAFETY: reservation hints do not overwrite existing mappings; NULL means try another low address.
                        std::ptr::NonNull::new(unsafe {
                            VirtualAllocEx(
                                handle,
                                Some(std::ptr::with_exposed_provenance(base)),
                                0x200000,
                                MEM_COMMIT | MEM_RESERVE,
                                PAGE_READWRITE,
                            )
                        })
                    })
                    .expect("low-address fixture allocation");
                let base = region.as_ptr().expose_provenance();
                let fixture = Self {
                    ctx: D2Context {
                        process,
                        d2_client: base,
                        d2_common: 0,
                        d2_win: 0,
                        d2_lang: 0,
                        d2_sigma: 0,
                        d2_sigma_size: 0,
                        always_show_items_ptr_rva: None,
                    },
                    region,
                    io: Scope::new(base..base + 0x200000),
                };
                fixture.seed(d2client::AUTOMAP_LAYER, fixture.address(0x1000));
                fixture.seed(d2client::PLAYER_UNIT, fixture.address(0x2000));
                fixture.seed(0x2000 + paths::TO_PATHS_PTR[1], fixture.address(0x3000));
                fixture.seed(0x3000 + paths::TO_PATHS_PTR[2], fixture.address(0x4000));
                fixture
            }
            pub fn address(&self, offset: usize) -> u32 {
                u32::try_from(self.ctx.d2_client + offset).unwrap()
            }
            pub fn seed(&self, offset: usize, value: u32) {
                self.ctx
                    .process
                    .write_buffer(self.ctx.d2_client + offset, &value.to_le_bytes())
                    .unwrap();
            }
            pub fn word(&self, offset: usize) -> u32 {
                self.ctx
                    .process
                    .read_memory(self.ctx.d2_client + offset)
                    .unwrap()
            }
            pub fn items(&self, count: u32) {
                self.seed(
                    0x4000 + room1::UNIT_FIRST,
                    if count == 0 { 0 } else { self.address(0x5000) },
                );
                for index in 0..count {
                    let offset = 0x5000 + usize::try_from(index).unwrap() * 0x100;
                    self.seed(offset + unit::UNIT_TYPE, unit_type::ITEM);
                    self.seed(offset + unit::UNIT_ID, index + 1);
                    self.seed(offset + unit::PATH, self.address(offset + 0x80));
                    self.seed(offset + 0x80 + item_path::SUB_X, 100 + index);
                    self.seed(offset + 0x80 + item_path::SUB_Y, 100);
                    self.seed(
                        offset + unit::ROOM_NEXT,
                        if index + 1 == count {
                            0
                        } else {
                            self.address(offset + 0x100)
                        },
                    );
                }
            }
            pub fn injector(&self) -> D2Injector {
                D2Injector::for_marker_test(
                    (0..200).map(|index| self.address(0x20000 + index * 0x40)),
                )
            }
            pub fn context(&self) -> D2Context {
                // SAFETY: duplicate ownership via a fresh handle to this process, not a borrowed HANDLE.
                let handle = unsafe {
                    OpenProcess(
                        PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                        false,
                        self.ctx.process.pid,
                    )
                }
                .unwrap();
                D2Context {
                    process: ProcessHandle {
                        handle,
                        pid: self.ctx.process.pid,
                    },
                    d2_client: self.ctx.d2_client,
                    d2_common: 0,
                    d2_win: 0,
                    d2_lang: 0,
                    d2_sigma: 0,
                    d2_sigma_size: 0,
                    always_show_items_ptr_rva: None,
                }
            }
            pub fn tick(
                &self,
                manager: &mut MapMarkerManager,
                injector: &D2Injector,
            ) -> Result<(), String> {
                let matched: Vec<_> = bfs_item_positions(&self.ctx, 10)?
                    .into_iter()
                    .map(|(ptr, sx, sy)| {
                        mk(
                            self.ctx
                                .process
                                .read_memory::<u32>(usize::try_from(ptr).unwrap() + unit::UNIT_ID)
                                .unwrap(),
                            sx,
                            sy,
                        )
                    })
                    .collect();
                let bfs = matched.iter().map(|item| item.unit_id).collect();
                manager.tick(&self.ctx, injector, &matched, &HashSet::new(), &bfs, None)
            }
            pub fn cells(&self) -> Vec<(u32, u16)> {
                let mut node = self.word(0x1000 + automap_layer::P_OBJECTS);
                let mut cells = Vec::new();
                while node != 0 {
                    assert!(
                        cells.len() < MAX_MARKER_CELLS,
                        "chain exceeds cap or cycles"
                    );
                    let address = usize::try_from(node).unwrap();
                    cells.push((
                        node,
                        self.ctx
                            .process
                            .read_memory(address + automap_cell::X_PIXEL)
                            .unwrap(),
                    ));
                    node = self
                        .ctx
                        .process
                        .read_memory(address + automap_cell::P_LESS)
                        .unwrap();
                }
                cells
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                // SAFETY: this is the original reservation, owned once, and the process handle is still live.
                let result = unsafe {
                    VirtualFreeEx(
                        self.ctx.process.handle,
                        self.region.as_ptr(),
                        0,
                        MEM_RELEASE,
                    )
                };
                assert!(result.is_ok(), "fixture pages must be released");
            }
        }
        pub fn calls(injector: &D2Injector) -> usize {
            injector
                .marker_allocator
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .calls
        }

        #[test]
        fn unchanged_full_bfs_snapshot_performs_no_rebuild() {
            let fixture = Fixture::new();
            let injector = fixture.injector();
            let mut manager = MapMarkerManager::new();
            fixture.items(100);
            fixture.tick(&mut manager, &injector).unwrap();
            fixture.items(101);
            fixture.tick(&mut manager, &injector).unwrap();
            let cells = fixture.cells();
            let writes = fixture.io.writes();
            for _ in 0..20 {
                fixture.tick(&mut manager, &injector).unwrap();
            }
            assert_eq!(fixture.cells(), cells);
            assert_eq!(fixture.io.writes(), writes);
            assert_eq!(calls(&injector), 100);
        }

        #[test]
        fn unconfirmed_detach_never_authorizes_reuse() {
            for operation in [Operation::Read, Operation::Write, Operation::Written] {
                let fixture = Fixture::new();
                let injector = fixture.injector();
                let mut manager = MapMarkerManager::new();
                fixture.items(1);
                fixture.tick(&mut manager, &injector).unwrap();
                fixture.seed(d2client::AUTOMAP_LAYER, fixture.address(0x1800));
                fixture.io.fail(
                    fixture.ctx.d2_client + 0x1000 + automap_layer::P_OBJECTS,
                    operation,
                );
                assert!(fixture.tick(&mut manager, &injector).is_err());
                assert_eq!(manager.placed.len(), 1);
                assert!(manager.spare_cells.is_empty());
                let writes = fixture.io.writes();
                match operation {
                    Operation::Read => {
                        fixture.tick(&mut manager, &injector).unwrap();
                        assert_eq!(fixture.word(0x1000 + automap_layer::P_OBJECTS), 0);
                    }
                    Operation::Write | Operation::Written => {
                        for _ in 0..3 {
                            assert!(fixture.tick(&mut manager, &injector).is_err());
                        }
                        assert_eq!(fixture.io.writes(), writes);
                    }
                }
                assert_eq!(calls(&injector), 1);
            }
        }

        #[test]
        fn preparation_and_publication_errors_quarantine_cells() {
            for offset in [
                0x20000,
                0x20000 + automap_cell::P_LESS,
                0x1000 + automap_layer::P_OBJECTS,
            ] {
                let fixture = Fixture::new();
                let injector = fixture.injector();
                let mut manager = MapMarkerManager::new();
                fixture.items(2);
                fixture
                    .io
                    .fail(fixture.ctx.d2_client + offset, Operation::Written);
                assert!(fixture.tick(&mut manager, &injector).is_err());
                let writes = fixture.io.writes();
                for _ in 0..3 {
                    assert!(fixture.tick(&mut manager, &injector).is_err());
                }
                assert_eq!(fixture.io.writes(), writes);
                assert_eq!(calls(&injector), 2);
            }
        }

        #[test]
        fn partial_allocation_distinguishes_null_from_unknown_outcome() {
            for outcome in [Ok(0), Err("unknown allocation outcome".to_string())] {
                let fixture = Fixture::new();
                let injector = fixture.injector();
                let mut manager = MapMarkerManager::new();
                fixture.items(2);
                injector
                    .marker_allocator
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .cells[1] = outcome.clone();
                assert!(fixture.tick(&mut manager, &injector).is_err());
                assert_eq!(manager.spare_cells, vec![fixture.address(0x20000)]);
                match outcome {
                    Ok(_) => {
                        fixture.tick(&mut manager, &injector).unwrap();
                        assert_eq!(manager.placed[0], fixture.address(0x20000));
                        assert_eq!(calls(&injector), 3);
                    }
                    Err(_) => {
                        assert!(fixture.tick(&mut manager, &injector).is_err());
                        assert_eq!(calls(&injector), 2);
                    }
                }
            }
        }
    }

    #[test]
    fn rebuilds_reuse_cells_at_the_high_water_mark() {
        let mut spare = Vec::new();
        let mut placed = Vec::new();
        let mut allocations = 0;
        for needed in (0..=100).chain((0..100).rev()).cycle().take(2010) {
            spare.append(&mut placed);
            grow_cell_pool(&mut spare, needed, || {
                allocations += 1;
                Ok(allocations)
            })
            .unwrap();
            placed = spare.drain(..needed).collect();
        }
        assert_eq!(allocations, 100);
    }

    #[test]
    fn allocation_failure_preserves_unpublished_cells_for_retry() {
        let mut cells = Vec::new();
        let mut calls = 0;
        assert!(grow_cell_pool(&mut cells, 3, || {
            calls += 1;
            if calls == 2 {
                Err("allocation failed".into())
            } else {
                Ok(calls)
            }
        })
        .is_err());
        assert_eq!(cells, vec![1]);
        assert!(grow_cell_pool(&mut cells, 3, || Ok(0)).is_err());
        assert_eq!(cells, vec![1]);
        grow_cell_pool(&mut cells, 3, || {
            calls += 1;
            Ok(calls)
        })
        .unwrap();
        assert_eq!(cells, vec![1, 3, 4]);
    }

    #[test]
    fn reuse_keeps_links_forward_even_when_spares_precede_detached_cells() {
        let mut cells = vec![300, 100, 200];
        grow_cell_pool(&mut cells, 3, || panic!("must reuse existing cells")).unwrap();
        assert_eq!(cells, vec![100, 200, 300]);
    }

    #[test]
    fn recycling_rejects_foreign_children_and_unreadable_links() {
        assert!(chain_is_intact(&[10, 20], |p| Ok((if p == 10 { 20 } else { 0 }, 0))).unwrap());
        assert!(!chain_is_intact(&[10, 20], |_| Ok((20, 99))).unwrap());
        assert!(!chain_is_intact(&[10, 20], |_| Ok((99, 0))).unwrap());
        assert!(chain_is_intact(&[10], |_| Err("unreadable".into())).is_err());
    }

    #[test]
    fn loading_invalidates_all_cell_addresses_without_losing_marker_cache() {
        let mut manager = MapMarkerManager::new();
        manager.last_layer = 123;
        manager.chain_parent_slot = 456;
        manager.placed.push(10);
        manager.spare_cells.push(20);
        manager.persistent.insert(1, mk(1, 50, 50));
        manager.invalidate_cells();
        assert!(manager.placed.is_empty() && manager.spare_cells.is_empty());
        assert_eq!(manager.last_layer, 0);
        assert_eq!(manager.chain_parent_slot, 0);
        assert_eq!(manager.persistent.len(), 1);
    }

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
    fn reconcile_over_cap_evicts_oldest_and_admits_newest() {
        // Fill to exactly the cap, all seen on the same BFS pass (as if the
        // player has been standing in a room full of unpicked matched
        // items for a while — everything's `last_seen` ties at `now`).
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let bfs: HashSet<u32> = (1..=10u32).collect();
        let t0 = Instant::now();
        let initial: Vec<MarkerItem> = (1..=10u32)
            .map(|uid| mk(uid, 10 + uid as i32, 10))
            .collect();
        reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &initial,
            &HashSet::new(),
            &bfs,
            Some((0, 0)),
            32,
            Duration::from_secs(3600),
            10,
            t0,
        );
        assert_eq!(persistent.len(), 10);

        // A new item (unit_id 11) drops a tick later; every old one is
        // still on the ground and still BFS-visible, so nothing would
        // naturally age out on its own.
        let t1 = t0 + Duration::from_millis(100);
        let mut bfs_next = bfs.clone();
        bfs_next.insert(11);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[mk(11, 999, 10)],
            &HashSet::new(),
            &bfs_next,
            Some((0, 0)),
            32,
            Duration::from_secs(3600),
            10,
            t1,
        );

        // Still capped at 10, the newest drop got a slot, and the very
        // oldest one (unit_id 1) was the one evicted to make room.
        assert_eq!(out.len(), 10);
        assert!(
            out.iter().any(|m| m.unit_id == 11),
            "new drop must be admitted once the cap is enforced"
        );
        assert!(
            !out.iter().any(|m| m.unit_id == 1),
            "oldest marker must be evicted to make room for the new one"
        );
    }

    #[test]
    fn reconcile_upserts_new_matches() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let matched = [mk(1, 50, 50), mk(2, 60, 60)];
        let bfs: HashSet<u32> = [1u32, 2].iter().copied().collect();
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &matched,
            &HashSet::new(),
            &bfs,
            Some((55, 55)),
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
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
        let mut first_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 200, 200));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((0, 0)),
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
            now,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn reconcile_evicts_close_cached_when_bfs_misses() {
        // Player next to item, BFS doesn't see → picked up.
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 55, 55));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((50, 50)),
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
            now,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn reconcile_evicts_when_bfs_sees_but_filter_no_longer_matches() {
        let mut persistent = HashMap::new();
        persistent.insert(42u32, mk(42, 55, 55));
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let now = Instant::now();
        last_seen.insert(42u32, now);

        let mut bfs = HashSet::new();
        bfs.insert(42u32);
        let explicitly_unmarked = bfs.clone();

        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[],
            &explicitly_unmarked,
            &bfs,
            Some((50, 50)),
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
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
        let mut first_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 10, 10));
        last_seen.insert(42u32, now);
        let matched = [mk(42, 200, 200)];
        let bfs: HashSet<u32> = [42u32].iter().copied().collect();
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &matched,
            &HashSet::new(),
            &bfs,
            Some((100, 100)),
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
            now,
        );
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].sub_x, out[0].sub_y), (200, 200));
    }

    #[test]
    fn reconcile_keeps_everything_when_player_pos_unknown() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let now = Instant::now();
        persistent.insert(42u32, mk(42, 50, 50));
        last_seen.insert(42u32, now);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            None,
            32,
            Duration::from_secs(3600),
            MAX_MARKER_CELLS,
            now,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn reconcile_evicts_after_ttl() {
        let mut persistent = HashMap::new();
        let mut last_seen = HashMap::new();
        let mut first_seen = HashMap::new();
        let early = Instant::now();
        persistent.insert(42u32, mk(42, 200, 200));
        last_seen.insert(42u32, early);
        let out = reconcile_persistent(
            &mut persistent,
            &mut last_seen,
            &mut first_seen,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((0, 0)),
            32,
            Duration::from_secs(60),
            MAX_MARKER_CELLS,
            early + Duration::from_secs(120),
        );
        assert!(out.is_empty());
        assert!(persistent.is_empty());
        assert!(last_seen.is_empty());
    }
}
