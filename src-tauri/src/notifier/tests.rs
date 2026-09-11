use super::*;
use std::collections::{HashMap, HashSet};

use crate::scanner_state::{BfsItemCandidate, CachedFilterDecision};

#[test]
fn bfs_candidate_enrichment_skips_current_scan_and_current_cached_decision() {
    let candidate = BfsItemCandidate {
        unit_id: 42,
        p_unit: 0x1000,
        sub_x: 10,
        sub_y: 20,
    };
    let mut current_item_ids = HashSet::new();
    let mut decisions = HashMap::new();

    assert!(should_enrich_bfs_candidate(
        &candidate,
        &current_item_ids,
        &decisions,
        7
    ));

    current_item_ids.insert(42);
    assert!(!should_enrich_bfs_candidate(
        &candidate,
        &current_item_ids,
        &decisions,
        7
    ));

    current_item_ids.clear();
    decisions.insert(
        42,
        CachedFilterDecision {
            generation: 7,
            visibility: Visibility::Show,
            place_on_map: true,
        },
    );
    assert!(!should_enrich_bfs_candidate(
        &candidate,
        &current_item_ids,
        &decisions,
        7
    ));

    decisions.get_mut(&42).unwrap().generation = 6;
    assert!(should_enrich_bfs_candidate(
        &candidate,
        &current_item_ids,
        &decisions,
        7
    ));
}

#[test]
fn item_scan_path_count_is_capped() {
    assert_eq!(capped_item_scan_path_count(0), (0, false));
    assert_eq!(capped_item_scan_path_count(12), (12, false));
    assert_eq!(
        capped_item_scan_path_count(MAX_ITEM_SCAN_PATHS + 1),
        (MAX_ITEM_SCAN_PATHS, true)
    );
}

#[test]
fn item_scan_unit_walk_stops_at_marker_bfs_cap() {
    assert!(item_scan_unit_index_in_bounds(0));
    assert!(item_scan_unit_index_in_bounds(
        MAX_ITEM_SCAN_UNITS_PER_PATH - 1
    ));
    assert!(!item_scan_unit_index_in_bounds(
        MAX_ITEM_SCAN_UNITS_PER_PATH
    ));
}
