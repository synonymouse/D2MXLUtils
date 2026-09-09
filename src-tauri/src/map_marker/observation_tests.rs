use super::tests::native::Fixture;
use super::*;

#[test]
fn layer_mismatch_when_allocation_finishes_preserves_layer_values() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    let wanted = [MarkerItem {
        unit_id: 1,
        cell_x: 1,
        cell_y: 1,
        sub_x: 1,
        sub_y: 1,
    }];

    assert!(manager
        .attach_chain(&fixture.ctx, &injector, fixture.address(0x1800), &wanted)
        .is_err());

    let snapshot = manager.diagnostics.first.as_ref().unwrap();
    assert_eq!(snapshot.failure.stage, Stage::AllocationLayer);
    assert_eq!(
        snapshot.failure.reason,
        Reason::LayerMismatch {
            expected_layer: fixture.address(0x1800),
            actual_layer: fixture.address(0x1000)
        }
    );
    assert!(!manager.cells_trusted);
}
