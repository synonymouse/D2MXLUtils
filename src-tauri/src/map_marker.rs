//! Automap markers for loot-filter matches.
//!
//! Writes `AutomapCell`s directly into a pre-allocated buffer in `Game.exe`
//! (`D2Injector::cell_buffer`) and attaches our chain as a leaf of the layer's
//! `pObjects` BST — walk `pLess` from the root until a NULL slot, attach there,
//! exactly like the engine's own icon insertion. Earlier versions instead
//! allocated cells dynamically via `NewAutomapCell` via `CreateRemoteThread`,
//! which suffered from remote thread injection errors (`os error 5`), thread
//! contention with the game engine, and Use-After-Free crashes when the engine
//! freed its automap chunks on layer unload. Using a dedicated `RemoteAlloc`
//! buffer avoids all remote thread calls, memory leaks, and layer-unload crashes.
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
    /// Cells placed in the layer's BST, in chain order. `placed[0]` is the head;
    /// the last cell's `pLess` is 0 (or an engine child); empty when not attached.
    placed: Vec<u32>,
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
    /// same low-unit_id batch camping there forever. Kept in lockstep with
    /// `persistent` (orphans are GC'd at the end of `reconcile_persistent`).
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
            last_hash: 0,
            persistent: HashMap::new(),
            last_seen: HashMap::new(),
            first_seen: HashMap::new(),
            last_player_sub: None,
        }
    }

    /// Detach our chain and wipe all state (cache + bookkeeping). Safe to
    /// call repeatedly.
    pub fn clear(&mut self, ctx: &D2Context) -> Result<(), String> {
        let layer = read_layer(ctx).unwrap_or(0);
        if layer != 0 && (self.chain_parent_slot != 0 || !self.placed.is_empty()) {
            self.detach_chain(ctx)?;
        } else if layer == 0 {
            self.chain_parent_slot = 0;
            self.placed.clear();
        }
        self.last_hash = 0;
        self.persistent.clear();
        self.last_seen.clear();
        self.first_seen.clear();
        self.last_layer = 0;
        self.last_player_sub = None;
        Ok(())
    }

    /// The marker scanner can observe loading before tick() runs. Forget
    /// addresses without touching memory the game may already have freed.
    /// If context is provided and a layer is still present, attempts to detach cleanly;
    /// if detachment fails, ownership state is preserved to block buffer reuse.
    pub fn invalidate_cells(&mut self, ctx: Option<&D2Context>) {
        if let Some(ctx) = ctx {
            let layer = read_layer(ctx).unwrap_or(0);
            if layer != 0 && (self.chain_parent_slot != 0 || !self.placed.is_empty()) {
                if let Err(_) = self.detach_chain(ctx) {
                    // Detachment failed: preserve ownership state so buffer reuse is blocked
                    return;
                }
            }
        }
        self.last_layer = 0;
        self.chain_parent_slot = 0;
        self.placed.clear();
        self.last_hash = 0;
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
                self.first_seen.clear();
                self.detach_chain(ctx)?;
                self.last_hash = 0;
            }
            self.last_player_sub = Some((px, py));
        }

        // Layer switch (Room2 crossing or layer reallocation): detach from
        // the old layer if still valid. Because our cells live in a dedicated
        // pre-allocated buffer owned by D2Injector, they are never freed by
        // the engine's layer tear-down (Fog_Free at D2Client+0x5F300).
        if layer != self.last_layer {
            if self.last_layer != 0 && (self.chain_parent_slot != 0 || !self.placed.is_empty()) {
                self.detach_chain(ctx)?;
            }
            self.last_layer = layer;
            self.last_hash = 0;
        }

        // Tamper check: if the slot we attached under (root or some
        // existing leaf's pLess) no longer points at our head, check if our head
        // is still reachable in the tree. If still in tree, update chain_parent_slot;
        // if confirmed gone, clear ownership; if read failed, preserve ownership.
        if self.chain_parent_slot != 0 {
            if let Some(&head) = self.placed.first() {
                let current = ctx
                    .process
                    .read_memory::<u32>(self.chain_parent_slot as usize)
                    .unwrap_or(0);
                if current != head {
                    let objects_slot = layer + automap_layer::P_OBJECTS as u32;
                    match find_node_slot(ctx, objects_slot, head) {
                        Ok(Some(actual_slot)) => {
                            self.chain_parent_slot = actual_slot;
                        }
                        Ok(None) => {
                            self.chain_parent_slot = 0;
                            self.placed.clear();
                            self.last_hash = 0;
                        }
                        Err(_) => {
                            // Read error: preserve ownership so we don't assume detached
                        }
                    }
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

    /// Detach our chain from the layer's BST.
    ///
    /// If the engine attached a child under our tail cell's `pLess` (e.g. a
    /// native quest/shrine icon), splice that child directly into
    /// the parent slot so engine-owned icons are never orphaned or lost.
    /// Propagates all read/write failures and preserves ownership state
    /// if detachment cannot be completed and confirmed.
    fn detach_chain(&mut self, ctx: &D2Context) -> Result<(), String> {
        if self.chain_parent_slot == 0 && self.placed.is_empty() {
            return Ok(());
        }

        let layer = read_layer(ctx)?;
        if layer == 0 {
            // Layer is gone (unmapped or null). The engine already destroyed the tree.
            self.chain_parent_slot = 0;
            self.placed.clear();
            return Ok(());
        }

        let head = match self.placed.first().copied() {
            Some(h) => h,
            None => {
                self.chain_parent_slot = 0;
                return Ok(());
            }
        };

        let objects_slot = layer + automap_layer::P_OBJECTS as u32;

        // 1. Locate the slot that points to our head.
        let parent_slot = if self.chain_parent_slot != 0 {
            let current = ctx
                .process
                .read_memory::<u32>(self.chain_parent_slot as usize)?;
            if current == head {
                Some(self.chain_parent_slot)
            } else {
                // Not at cached slot; search the pObjects tree.
                find_node_slot(ctx, objects_slot, head)?
            }
        } else {
            find_node_slot(ctx, objects_slot, head)?
        };

        // 2. If head was found in the tree, splice it out.
        if let Some(slot) = parent_slot {
            let tail = *self.placed.last().unwrap();
            let tail_child = ctx
                .process
                .read_memory::<u32>(tail as usize + automap_cell::P_LESS)?;

            let replacement = if tail_child != 0 && !self.placed.contains(&tail_child) {
                tail_child
            } else {
                0
            };

            ctx.process
                .write_buffer(slot as usize, &replacement.to_le_bytes())?;

            // 3. Confirm detachment: verify head is no longer reachable from objects_slot.
            if let Some(still_slot) = find_node_slot(ctx, objects_slot, head)? {
                self.chain_parent_slot = still_slot;
                return Err(format!(
                    "detach_chain: head {:#x} still reachable in pObjects at slot {:#x} after detachment",
                    head, still_slot
                ));
            }
        }

        // 4. Detachment confirmed: safe to clear bookkeeping and allow buffer reuse.
        self.chain_parent_slot = 0;
        self.placed.clear();
        Ok(())
    }

    /// Write cell records for `wanted` into our dedicated remote buffer and
    /// attach the chain as a leaf hanging off the existing `pObjects` tree.
    ///
    /// Uses the pre-allocated `cell_buffer` in `D2Injector` rather than calling
    /// `NewAutomapCell` via `CreateRemoteThread`. This avoids thread injection
    /// overhead, thread contention with the game engine's chunk allocator, and
    /// eliminates memory leaks and Use-After-Free crashes across room transitions.
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

        // Block buffer reuse until detachment is confirmed
        if self.chain_parent_slot != 0 || !self.placed.is_empty() {
            return Err(format!(
                "attach_chain: buffer reuse blocked - previous chain is still active (slot={:#x}, placed_count={})",
                self.chain_parent_slot,
                self.placed.len()
            ));
        }

        let cell_base = injector.cell_buffer.address as u32;
        if cell_base == 0 {
            return Err("D2Injector cell_buffer address is null".to_string());
        }

        let objects_slot = layer + automap_layer::P_OBJECTS as u32;

        // Block buffer reuse if cell_base is reachable anywhere in pObjects
        if let Some(slot) = find_node_slot(ctx, objects_slot, cell_base)? {
            return Err(format!(
                "attach_chain: cell buffer {:#x} is still reachable in pObjects at slot {:#x}; buffer reuse blocked",
                cell_base, slot
            ));
        }

        let attach_slot = find_leaf_slot(ctx, objects_slot)?;

        // Ensure attach_slot is not inside our own cell buffer
        let cell_buffer_end = cell_base + (MAX_MARKER_CELLS * automap_cell::SIZE) as u32;
        if attach_slot >= cell_base && attach_slot < cell_buffer_end {
            return Err(format!(
                "attach_chain: attach_slot {:#x} is inside cell_buffer [{:#x}..{:#x}]; cycle prevented",
                attach_slot, cell_base, cell_buffer_end
            ));
        }

        if read_layer(ctx)? != layer {
            return Err("Automap layer changed during marker attachment".to_string());
        }

        // Format all cells into a contiguous buffer and write in a single call.
        let bytes = build_marker_cells(cell_base, wanted);
        ctx.process.write_buffer(cell_base as usize, &bytes)?;

        // Splice head into attach_slot
        ctx.process
            .write_buffer(attach_slot as usize, &cell_base.to_le_bytes())?;

        self.chain_parent_slot = attach_slot;
        self.placed = (0..wanted.len())
            .map(|i| cell_base + (i * automap_cell::SIZE) as u32)
            .collect();
        Ok(())
    }
}

/// Serialize `items` into a contiguous byte buffer of `AutomapCell` structures,
/// chained together via `pLess`.
///
/// Cell `i` points to `cell_base + (i + 1) * automap_cell::SIZE`, and the last
/// cell has `pLess = 0`.
pub fn build_marker_cells(cell_base: u32, items: &[MarkerItem]) -> Vec<u8> {
    let mut out = Vec::with_capacity(items.len() * automap_cell::SIZE);
    for (i, item) in items.iter().enumerate() {
        let p_less = if i + 1 < items.len() {
            cell_base + ((i + 1) * automap_cell::SIZE) as u32
        } else {
            0
        };
        let mut buf = [0u8; automap_cell::SIZE];
        buf[automap_cell::F_SAVED..automap_cell::F_SAVED + 4].copy_from_slice(&1u32.to_le_bytes());
        buf[automap_cell::N_CELL_NO..automap_cell::N_CELL_NO + 2]
            .copy_from_slice(&automap_cell::CROSS_CELL_NO.to_le_bytes());
        buf[automap_cell::X_PIXEL..automap_cell::X_PIXEL + 2]
            .copy_from_slice(&(item.cell_x as i16 as u16).to_le_bytes());
        buf[automap_cell::Y_PIXEL..automap_cell::Y_PIXEL + 2]
            .copy_from_slice(&(item.cell_y as i16 as u16).to_le_bytes());
        buf[automap_cell::P_LESS..automap_cell::P_LESS + 4].copy_from_slice(&p_less.to_le_bytes());
        // wWeight and pMore are 0 in buf.
        out.extend_from_slice(&buf);
    }
    out
}

fn chain_is_intact(
    cells: &[u32],
    mut read_links: impl FnMut(u32) -> Result<(u32, u32), String>,
) -> Result<bool, String> {
    for (i, &cell) in cells.iter().enumerate() {
        let (less, more) = read_links(cell)?;
        if more != 0 {
            return Ok(false);
        }
        let expected_less = cells.get(i + 1).copied();
        if let Some(next) = expected_less {
            if less != next {
                return Ok(false);
            }
        }
        // For the tail cell, `less` may be 0 or an engine child; both are valid.
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
    first_seen.retain(|uid, _| persistent.contains_key(uid));

    // Over the marker-cell cap: keep the most recently dropped `max_markers`
    // entries, evicting the rest. Without this, a persistent set that grows
    // past the cap (nothing here evicts by count on its own) always loses
    // the same oldest-unit_id batch to `attach_chain`'s own truncation,
    // permanently blocking every later drop from ever getting a marker.
    if persistent.len() > max_markers {
        let mut by_age: Vec<(u32, Instant)> = first_seen.iter().map(|(&u, &t)| (u, t)).collect();
        by_age.sort_unstable_by_key(|&(uid, t)| (t, uid));
        for &(uid, _) in by_age.iter().take(by_age.len() - max_markers) {
            persistent.remove(&uid);
            last_seen.remove(&uid);
            first_seen.remove(&uid);
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
/// own icon insertion would have put a new leaf. Bounded with cycle detection
/// to guard against a corrupted/cyclic tree.
fn find_leaf_slot(ctx: &D2Context, root_slot: u32) -> Result<u32, String> {
    find_leaf_slot_impl(|addr| ctx.process.read_memory::<u32>(addr), root_slot)
}

fn find_leaf_slot_impl(
    mut read_u32: impl FnMut(usize) -> Result<u32, String>,
    root_slot: u32,
) -> Result<u32, String> {
    let mut slot = root_slot;
    let mut visited: HashSet<u32> = HashSet::new();
    for _ in 0..4096 {
        let node = read_u32(slot as usize)?;
        if node == 0 {
            return Ok(slot);
        }
        if !visited.insert(node) {
            return Err("find_leaf_slot: cycle detected in pObjects tree".to_string());
        }
        slot = node + automap_cell::P_LESS as u32;
    }
    Err("find_leaf_slot: pObjects tree exceeds depth bound (corrupted?)".to_string())
}

/// Search `pObjects` BST starting from `root_slot` to find the slot holding `target_node`.
/// Returns `Ok(Some(slot))` if found, `Ok(None)` if not reachable, or `Err` on read error.
fn find_node_slot(
    ctx: &D2Context,
    root_slot: u32,
    target_node: u32,
) -> Result<Option<u32>, String> {
    find_node_slot_impl(
        |addr| ctx.process.read_memory::<u32>(addr),
        root_slot,
        target_node,
    )
}

fn find_node_slot_impl(
    mut read_u32: impl FnMut(usize) -> Result<u32, String>,
    root_slot: u32,
    target_node: u32,
) -> Result<Option<u32>, String> {
    if target_node == 0 {
        return Ok(None);
    }
    let root_node = read_u32(root_slot as usize)?;
    if root_node == target_node {
        return Ok(Some(root_slot));
    }
    if root_node == 0 {
        return Ok(None);
    }

    let mut visited: HashSet<u32> = HashSet::new();
    let mut queue: Vec<u32> = vec![root_node];
    visited.insert(root_node);

    let mut steps = 0;
    while let Some(curr) = queue.pop() {
        steps += 1;
        if steps > 4096 {
            return Err(
                "find_node_slot: pObjects tree exceeds depth bound (corrupted?)".to_string(),
            );
        }

        // Check pLess
        let less_slot = curr + automap_cell::P_LESS as u32;
        let less = read_u32(less_slot as usize)?;
        if less == target_node {
            return Ok(Some(less_slot));
        }
        if less != 0 && visited.insert(less) {
            queue.push(less);
        }

        // Check pMore
        let more_slot = curr + automap_cell::P_MORE as u32;
        let more = read_u32(more_slot as usize)?;
        if more == target_node {
            return Ok(Some(more_slot));
        }
        if more != 0 && visited.insert(more) {
            queue.push(more);
        }
    }

    Ok(None)
}

fn read_layer(ctx: &D2Context) -> Result<u32, String> {
    ctx.process
        .read_memory::<u32>(ctx.d2_client + d2client::AUTOMAP_LAYER)
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

    #[test]
    fn build_marker_cells_formats_contiguous_chain() {
        let items = [mk(1, 10, 20), mk(2, 30, 40), mk(3, 50, 60)];
        let base = 0x1000u32;
        let bytes = build_marker_cells(base, &items);
        assert_eq!(bytes.len(), 3 * automap_cell::SIZE);

        // Check Cell 0
        let c0 = &bytes[0..20];
        let c0_saved = u32::from_le_bytes(c0[0..4].try_into().unwrap());
        let c0_cell_no = u16::from_le_bytes(c0[4..6].try_into().unwrap());
        let c0_x = i16::from_le_bytes(c0[6..8].try_into().unwrap());
        let c0_y = i16::from_le_bytes(c0[8..10].try_into().unwrap());
        let c0_less = u32::from_le_bytes(c0[12..16].try_into().unwrap());
        let c0_more = u32::from_le_bytes(c0[16..20].try_into().unwrap());
        assert_eq!(c0_saved, 1);
        assert_eq!(c0_cell_no, 300);
        assert_eq!(
            (c0_x as i32, c0_y as i32),
            (items[0].cell_x, items[0].cell_y)
        );
        assert_eq!(c0_less, base + 20);
        assert_eq!(c0_more, 0);

        // Check Cell 1
        let c1 = &bytes[20..40];
        let c1_less = u32::from_le_bytes(c1[12..16].try_into().unwrap());
        assert_eq!(c1_less, base + 40);

        // Check Cell 2 (tail)
        let c2 = &bytes[40..60];
        let c2_less = u32::from_le_bytes(c2[12..16].try_into().unwrap());
        let c2_more = u32::from_le_bytes(c2[16..20].try_into().unwrap());
        assert_eq!(c2_less, 0);
        assert_eq!(c2_more, 0);
    }

    #[test]
    fn chain_is_intact_validates_chain_and_allows_tail_child() {
        assert!(chain_is_intact(&[10, 20], |p| Ok((if p == 10 { 20 } else { 0 }, 0))).unwrap());
        // Tail having an engine child attached under it is valid:
        assert!(chain_is_intact(&[10, 20], |p| Ok((if p == 10 { 20 } else { 999 }, 0))).unwrap());
        // Broken intermediate link:
        assert!(!chain_is_intact(&[10, 20], |p| Ok((if p == 10 { 999 } else { 0 }, 0))).unwrap());
        // Non-zero pMore:
        assert!(!chain_is_intact(&[10, 20], |_| Ok((20, 1))).unwrap());
        assert!(chain_is_intact(&[10], |_| Err("unreadable".into())).is_err());
    }

    #[test]
    fn loading_invalidates_all_cell_addresses_without_losing_marker_cache() {
        let mut manager = MapMarkerManager::new();
        manager.last_layer = 123;
        manager.chain_parent_slot = 456;
        manager.placed.push(10);
        manager.persistent.insert(1, mk(1, 50, 50));
        manager.invalidate_cells(None);
        assert!(manager.placed.is_empty());
        assert_eq!(manager.last_layer, 0);
        assert_eq!(manager.chain_parent_slot, 0);
        assert_eq!(manager.persistent.len(), 1);
    }

    #[test]
    fn find_leaf_slot_finds_null_slot() {
        let mut mem: HashMap<usize, u32> = HashMap::new();
        mem.insert(0x100, 0x200);
        mem.insert(0x20C, 0x300);
        mem.insert(0x30C, 0);

        let res = find_leaf_slot_impl(|addr| Ok(*mem.get(&addr).unwrap_or(&0)), 0x100).unwrap();
        assert_eq!(res, 0x30C);
    }

    #[test]
    fn find_leaf_slot_detects_cycle() {
        let mut mem: HashMap<usize, u32> = HashMap::new();
        mem.insert(0x100, 0x200);
        mem.insert(0x20C, 0x300);
        mem.insert(0x30C, 0x200);

        let res = find_leaf_slot_impl(|addr| Ok(*mem.get(&addr).unwrap_or(&0)), 0x100);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("cycle detected"));
    }

    #[test]
    fn find_node_slot_finds_root_and_children() {
        let mut mem: HashMap<usize, u32> = HashMap::new();
        mem.insert(0x100, 0x200);
        mem.insert(0x20C, 0x300);
        mem.insert(0x210, 0x400);
        mem.insert(0x30C, 0);
        mem.insert(0x310, 0);
        mem.insert(0x40C, 0x500);
        mem.insert(0x410, 0);
        mem.insert(0x50C, 0);
        mem.insert(0x510, 0);

        let read = |addr| Ok(*mem.get(&addr).unwrap_or(&0));

        // Root
        assert_eq!(
            find_node_slot_impl(read, 0x100, 0x200).unwrap(),
            Some(0x100)
        );
        // Left child of 0x200
        assert_eq!(
            find_node_slot_impl(read, 0x100, 0x300).unwrap(),
            Some(0x20C)
        );
        // Right child of 0x200
        assert_eq!(
            find_node_slot_impl(read, 0x100, 0x400).unwrap(),
            Some(0x210)
        );
        // Left child of 0x400
        assert_eq!(
            find_node_slot_impl(read, 0x100, 0x500).unwrap(),
            Some(0x40C)
        );
        // Absent node
        assert_eq!(find_node_slot_impl(read, 0x100, 0x999).unwrap(), None);
    }

    #[test]
    fn find_node_slot_handles_cycle_safely() {
        let mut mem: HashMap<usize, u32> = HashMap::new();
        mem.insert(0x100, 0x200);
        mem.insert(0x20C, 0x300);
        mem.insert(0x30C, 0x200);

        let read = |addr| Ok(*mem.get(&addr).unwrap_or(&0));
        assert_eq!(find_node_slot_impl(read, 0x100, 0x888).unwrap(), None);
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

    #[cfg(target_os = "windows")]
    #[test]
    fn buffer_reuse_blocked_when_attached_or_placed() {
        use crate::injection::PublishedRemoteBuffer;
        use crate::process::ProcessHandle;
        use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
        use windows::Win32::System::Threading::GetCurrentProcess;

        let mut handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentProcess(),
                GetCurrentProcess(),
                &mut handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .unwrap();
        }
        let process = ProcessHandle {
            handle,
            pid: std::process::id(),
        };
        let ctx = D2Context {
            process,
            d2_client: 0,
            d2_common: 0,
            d2_win: 0,
            d2_lang: 0,
            d2_sigma: 0,
            d2_sigma_size: 0,
            always_show_items_ptr_rva: None,
        };

        let injector = D2Injector {
            string_buffer: crate::injection::RemoteAlloc::new(&ctx.process, 0x100).unwrap(),
            params_buffer: crate::injection::RemoteAlloc::new(&ctx.process, 0x100).unwrap(),
            cell_buffer: PublishedRemoteBuffer {
                address: 0x12340000,
                size: 0x1000,
            },
            inject_get_string: 0,
            inject_get_item_name: 0,
            inject_get_item_stat: 0,
            inject_get_unit_stat: 0,
            inject_new_automap_cell: 0,
            remote_calls_get_string: std::sync::atomic::AtomicU64::new(0),
            remote_calls_get_item_name: std::sync::atomic::AtomicU64::new(0),
            remote_calls_get_item_stat: std::sync::atomic::AtomicU64::new(0),
            remote_calls_get_unit_stat: std::sync::atomic::AtomicU64::new(0),
            remote_calls_new_automap_cell: std::sync::atomic::AtomicU64::new(0),
        };

        let mut manager = MapMarkerManager::new();
        manager.chain_parent_slot = 0x500;
        manager.placed.push(0x12340000);

        let wanted = [mk(1, 10, 20)];
        let res = manager.attach_chain(&ctx, &injector, 0x1000, &wanted);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("buffer reuse blocked"));
        // Ownership state preserved
        assert_eq!(manager.chain_parent_slot, 0x500);
        assert_eq!(manager.placed.len(), 1);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn detach_failure_preserves_ownership_state() {
        use crate::process::ProcessHandle;
        use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
        use windows::Win32::System::Threading::GetCurrentProcess;

        let mut handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentProcess(),
                GetCurrentProcess(),
                &mut handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .unwrap();
        }
        let process = ProcessHandle {
            handle,
            pid: std::process::id(),
        };

        let alloc = crate::injection::RemoteAlloc::new(&process, 0x200000).unwrap();
        let mock_layer_addr = alloc.address + 0x100;
        let p_automap_layer = alloc.address + d2client::AUTOMAP_LAYER;
        process
            .write_buffer(p_automap_layer, &(mock_layer_addr as u32).to_le_bytes())
            .unwrap();

        let ctx = D2Context {
            process,
            d2_client: alloc.address,
            d2_common: 0,
            d2_win: 0,
            d2_lang: 0,
            d2_sigma: 0,
            d2_sigma_size: 0,
            always_show_items_ptr_rva: None,
        };

        let mut manager = MapMarkerManager::new();
        // Point chain_parent_slot to an unmapped address where read_memory will fail
        manager.chain_parent_slot = 0xFFFFFFFF;
        manager.placed.push(0x12340000);

        let res = manager.detach_chain(&ctx);
        assert!(
            res.is_err(),
            "detachment should fail when reading unmapped memory"
        );
        // Crucial requirement: ownership state must NOT be cleared on failure!
        assert_eq!(manager.chain_parent_slot, 0xFFFFFFFF);
        assert_eq!(manager.placed, vec![0x12340000]);

        // clear() must also fail and preserve ownership
        let clear_res = manager.clear(&ctx);
        assert!(clear_res.is_err());
        assert_eq!(manager.chain_parent_slot, 0xFFFFFFFF);
        assert_eq!(manager.placed, vec![0x12340000]);
    }
}
