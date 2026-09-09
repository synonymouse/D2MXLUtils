use super::tests::native::{calls, Fixture};
use super::*;
use crate::process::marker_test_io::Operation;

#[test]
fn first_snapshot_when_parent_replaced_preserves_quarantine() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.items(2);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x7000));
    let writes = fixture.io.writes();

    assert!(fixture.tick(&mut manager, &injector).is_err());

    let mut events = Vec::new();
    manager.emit_quarantine_with(|line| events.push(line.to_owned()));
    assert_eq!(events.len(), 1);
    let event: serde_json::Value =
        serde_json::from_str(events[0].strip_prefix("map_marker quarantine: ").unwrap()).unwrap();
    assert_eq!(event["failure"]["reason"], "parent_mismatch");
    assert_eq!(
        event["failure"]["expected_parent"],
        fixture.address(0x20000)
    );
    assert_eq!(event["failure"]["actual_parent"], fixture.address(0x7000));
    assert_eq!(event["placed_count"], 2);
    assert_eq!(fixture.io.writes(), writes);
    for _ in 0..3 {
        assert!(fixture.tick(&mut manager, &injector).is_err());
        manager.emit_quarantine_with(|line| events.push(line.to_owned()));
    }
    assert_eq!(events.len(), 1);
    assert_eq!(fixture.io.writes(), writes);
    assert_eq!(calls(&injector), 2);
}

#[test]
fn diagnostics_when_preparation_or_link_write_fails_keep_original_stage() {
    for (offset, stage) in [
        (0x20000, Stage::PrepareCell),
        (0x20000 + automap_cell::P_LESS, Stage::LinkCell),
        (0x1000 + automap_layer::P_OBJECTS, Stage::Publish),
    ] {
        for operation in [Operation::Write, Operation::Written] {
            let fixture = Fixture::new();
            let injector = fixture.injector();
            let mut manager = MapMarkerManager::new();
            fixture.items(2);
            fixture.io.fail(fixture.ctx.d2_client + offset, operation);

            assert!(fixture.tick(&mut manager, &injector).is_err());

            let snapshot = manager.diagnostics.first.clone().unwrap();
            assert_eq!(snapshot.failure.stage, stage);
            let writes = fixture.io.writes();
            assert!(fixture.tick(&mut manager, &injector).is_err());
            assert_eq!(manager.diagnostics.first.as_ref(), Some(&snapshot));
            assert_eq!(fixture.io.writes(), writes);
            assert_eq!(calls(&injector), 2);
        }
    }
}

#[test]
fn new_diagnostic_when_session_reset_clears_first_snapshot() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.items(1);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, 0);
    assert!(fixture.tick(&mut manager, &injector).is_err());
    let mut events = Vec::new();
    manager.emit_quarantine_with(|line| events.push(line.to_owned()));

    manager.reset_session();

    assert!(manager.diagnostics.first.is_none());
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x7000));
    assert!(fixture.tick(&mut manager, &injector).is_err());
    manager.emit_quarantine_with(|line| events.push(line.to_owned()));
    assert_eq!(events.len(), 2);
    assert_ne!(events[0], events[1]);
}

#[test]
fn read_error_when_detaching_remains_retryable_without_diagnostic() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.items(1);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.io.fail(
        fixture.ctx.d2_client + 0x20000 + automap_cell::P_MORE,
        Operation::Read,
    );

    assert!(manager.clear(&fixture.ctx).is_err());

    assert!(manager.diagnostics.first.is_none());
    assert!(manager.cells_trusted);
    manager.clear(&fixture.ctx).unwrap();
}

#[test]
fn diagnostic_layer_read_when_unavailable_does_not_change_failure() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.items(1);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, 0);
    fixture.io.fail(
        fixture.ctx.d2_client + d2client::AUTOMAP_LAYER,
        Operation::Read,
    );

    assert!(manager.detach_chain(&fixture.ctx).is_err());

    let snapshot = manager.diagnostics.first.as_ref().unwrap();
    assert_eq!(snapshot.observed_layer, None);
    assert_eq!(
        snapshot.failure.reason,
        Reason::ParentMismatch {
            expected_parent: fixture.address(0x20000),
            actual_parent: 0
        }
    );
    assert!(!manager.cells_trusted);
}
