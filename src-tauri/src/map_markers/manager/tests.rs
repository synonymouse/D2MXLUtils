use super::test_support::mk;
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
#[path = "native_tests.rs"]
mod native;

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
