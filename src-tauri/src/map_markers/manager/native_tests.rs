use super::super::test_support::native::{calls, Fixture};
use super::*;
use crate::process::marker_test_io::Operation;

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
