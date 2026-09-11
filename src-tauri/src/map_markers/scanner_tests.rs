use super::*;

use crate::rules::Visibility;
use crate::scanner_state::CachedFilterDecision;

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use crate::map_markers::test_support::{calls, Fixture};
    use crate::offsets::{automap_layer, room1};
    use crate::process::marker_test_io::Operation;
    use crate::rules::{FilterConfig, Rule};
    use std::sync::RwLock;

    fn scanner(fixture: &Fixture, count: u32) -> MarkerScanner {
        fixture.items(count);
        let state = Arc::new(SharedScannerState::new(
            fixture.context(),
            fixture.injector(),
            HashMap::new(),
        ));
        state.filter_generation.store(1, Ordering::SeqCst);
        *state.recent_filter_decisions.write().unwrap() = (1..=count)
            .map(|unit_id| {
                (
                    unit_id,
                    CachedFilterDecision {
                        generation: 1,
                        visibility: Visibility::Show,
                        place_on_map: true,
                    },
                )
            })
            .collect();
        let scanner = MarkerScanner::new(state);
        map_enabled(&scanner, true);
        scanner
    }
    fn map_enabled(scanner: &MarkerScanner, enabled: bool) {
        let mut config = FilterConfig::default();
        let mut rule = Rule::default();
        rule.map = enabled;
        config.rules.push(rule);
        *scanner.state.filter_config.write().unwrap() = Some(Arc::new(RwLock::new(config)));
    }

    #[test]
    fn map_off_on_cycles_reuse_the_high_water_pool() {
        let fixture = Fixture::new();
        let mut scanner = scanner(&fixture, 101);
        scanner.tick();
        assert_eq!(scanner.state.recent_bfs_items.read().unwrap().len(), 101);
        assert_eq!(fixture.cells().len(), 100);
        for _ in 0..10 {
            map_enabled(&scanner, false);
            fixture.io.fail(
                fixture.ctx.d2_client + 0x1000 + automap_layer::P_OBJECTS,
                Operation::Read,
            );
            scanner.tick();
            assert_eq!(fixture.cells().len(), 100);
            scanner.tick();
            assert!(fixture.cells().is_empty());
            map_enabled(&scanner, true);
            scanner.tick();
            assert_eq!(fixture.cells().len(), 100);
            assert_eq!(calls(&scanner.state.injector.lock().unwrap()), 100);
        }
    }

    #[test]
    fn failed_clear_does_not_resurrect_removed_items() {
        for (offset, operation) in [
            (d2client::AUTOMAP_LAYER, Operation::Read),
            (0x1000 + automap_layer::P_OBJECTS, Operation::Read),
            (0x1000 + automap_layer::P_OBJECTS, Operation::Written),
        ] {
            let fixture = Fixture::new();
            let mut scanner = scanner(&fixture, 1);
            scanner.tick();
            assert_eq!(fixture.cells().len(), 1);
            map_enabled(&scanner, false);
            fixture.io.fail(fixture.ctx.d2_client + offset, operation);
            scanner.tick();
            assert!(!scanner.markers_cleared);
            fixture.seed(0x4000 + room1::UNIT_FIRST, 0);
            match operation {
                Operation::Read => {}
                Operation::Written => {
                    fixture.seed(d2client::PLAYER_UNIT, 0);
                    scanner.tick();
                    fixture.seed(d2client::PLAYER_UNIT, fixture.address(0x2000));
                }
                Operation::Write => unreachable!(),
            }
            map_enabled(&scanner, true);
            scanner.tick();
            assert!(fixture.cells().is_empty());
            assert_eq!(calls(&scanner.state.injector.lock().unwrap()), 1);
        }
    }

    #[test]
    fn loading_and_session_reset_never_write_old_cells() {
        for session_reset in [false, true] {
            let fixture = Fixture::new();
            let mut scanner = scanner(&fixture, 1);
            scanner.tick();
            assert_eq!(fixture.cells().len(), 1);
            let writes = fixture.io.writes();
            if session_reset {
                scanner.clear();
            } else {
                fixture.seed(d2client::PLAYER_UNIT, 0);
                scanner.tick();
            }
            assert_eq!(fixture.io.writes(), writes + usize::from(!session_reset));
            assert!(scanner.state.recent_bfs_items.read().unwrap().is_empty());
            fixture.seed(0x20000, 0x55555555);
            fixture.seed(0x1000 + automap_layer::P_OBJECTS, 0);
            fixture.seed(d2client::PLAYER_UNIT, fixture.address(0x2000));
            scanner.tick();
            assert_eq!(fixture.word(0x20000), 0x55555555);
            assert_eq!(fixture.cells().len(), 1);
            assert_eq!(calls(&scanner.state.injector.lock().unwrap()), 2);
        }
    }
}

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
