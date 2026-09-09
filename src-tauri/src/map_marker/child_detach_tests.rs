use super::tests::native::{calls, Fixture};
use super::*;

pub(super) fn seeded(count: u32) -> (Fixture, D2Injector, MapMarkerManager) {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x7000));
    fixture.items(count);
    fixture.tick(&mut manager, &injector).unwrap();
    let tail = usize::try_from(*manager.placed.last().unwrap()).unwrap();
    fixture
        .ctx
        .process
        .write_buffer(
            tail + automap_cell::P_LESS,
            &fixture.address(0x8000).to_le_bytes(),
        )
        .unwrap();
    (fixture, injector, manager)
}

pub(super) fn remains_quarantined(
    fixture: &Fixture,
    manager: &mut MapMarkerManager,
    injector: &D2Injector,
) {
    assert!(!manager.cells_trusted);
    let diagnosis = manager.diagnostics.first.clone().unwrap();
    let placed = manager.placed.clone();
    let spares = manager.spare_cells.clone();
    let allocated = calls(injector);
    let writes = fixture.io.writes();
    for _ in 0..3 {
        assert!(fixture.tick(manager, injector).is_err());
        assert!(manager.clear(&fixture.ctx).is_err());
    }
    assert_eq!(manager.placed, placed);
    assert_eq!(manager.spare_cells, spares);
    assert_eq!(manager.diagnostics.first.as_ref(), Some(&diagnosis));
    assert_eq!(fixture.io.writes(), writes);
    assert_eq!(calls(injector), allocated);
}

#[test]
fn child_tree_is_preserved_when_one_cell_detaches_in_same_layer() {
    let fixture = Fixture::new();
    let injector = fixture.injector();
    let mut manager = MapMarkerManager::new();
    fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x7000));
    fixture.items(1);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.seed(0x20000 + automap_cell::P_LESS, fixture.address(0x8000));
    fixture.seed(0x8000 + automap_cell::P_MORE, fixture.address(0x8040));
    let before: [u8; 0x80] = fixture
        .ctx
        .process
        .read_memory(fixture.ctx.d2_client + 0x8000)
        .unwrap();
    let writes = fixture.io.writes();

    let detached = manager.detach_chain(&fixture.ctx);

    assert!(
        detached.is_ok(),
        "same-layer acyclic tail child must detach: {detached:?}"
    );
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    assert_eq!(
        fixture.word(0x20000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    assert_eq!(
        fixture
            .ctx
            .process
            .read_memory::<[u8; 0x80]>(fixture.ctx.d2_client + 0x8000)
            .unwrap(),
        before
    );
    assert_eq!(fixture.io.writes(), writes + 1);
    assert!(manager.cells_trusted);
    assert!(manager.diagnostics.first.is_none());
    assert!(manager.placed.is_empty());
    assert_eq!(manager.spare_cells, [fixture.address(0x20000)]);
    assert_eq!(calls(&injector), 1);
}

#[test]
fn child_bytes_are_unchanged_when_multicell_chain_detaches() {
    let (fixture, injector, mut manager) = seeded(3);
    fixture.seed(0x8000 + automap_cell::P_LESS, fixture.address(0x8040));
    fixture.seed(0x8000 + automap_cell::P_MORE, fixture.address(0x8080));
    fixture.seed(0x8040, 0x12345678);
    let before: [u8; 0xc0] = fixture
        .ctx
        .process
        .read_memory(fixture.ctx.d2_client + 0x8000)
        .unwrap();
    let writes = fixture.io.writes();
    let placed = manager.placed.clone();

    manager.detach_chain(&fixture.ctx).unwrap();

    assert_eq!(
        fixture
            .ctx
            .process
            .read_memory::<[u8; 0xc0]>(fixture.ctx.d2_client + 0x8000)
            .unwrap(),
        before
    );
    assert_eq!(fixture.io.writes(), writes + 1);
    assert_eq!(manager.spare_cells, placed);
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    assert!(manager.diagnostics.first.is_none());
    assert_eq!(calls(&injector), 3);
}

#[test]
fn pool_stays_bounded_when_child_detach_is_followed_by_rebuilds() {
    let (fixture, injector, mut manager) = seeded(2);
    let allocated = manager.placed.clone();

    for position in 110..140 {
        fixture.seed(0x5080 + item_path::SUB_X, position);
        fixture.tick(&mut manager, &injector).unwrap();
    }

    assert_eq!(manager.placed, allocated);
    assert_eq!(calls(&injector), 2);
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    assert_eq!(fixture.word(0x8000 + automap_cell::P_LESS), allocated[0]);
    assert!(manager.cells_trusted);
    assert!(manager.diagnostics.first.is_none());
}

#[test]
fn foreign_tree_is_restored_when_rebuilt_markers_are_picked_up() {
    let (fixture, injector, mut manager) = seeded(2);
    fixture.seed(0x8000 + automap_cell::P_MORE, fixture.address(0x8040));
    let before: [u8; 0x80] = fixture
        .ctx
        .process
        .read_memory(fixture.ctx.d2_client + 0x8000)
        .unwrap();
    fixture.seed(0x5080 + item_path::SUB_X, 110);
    fixture.tick(&mut manager, &injector).unwrap();
    fixture.items(0);

    manager
        .tick(
            &fixture.ctx,
            &injector,
            &[],
            &HashSet::new(),
            &HashSet::new(),
            Some((100, 100)),
        )
        .unwrap();

    assert_eq!(
        fixture
            .ctx
            .process
            .read_memory::<[u8; 0x80]>(fixture.ctx.d2_client + 0x8000)
            .unwrap(),
        before
    );
    assert!(manager.placed.is_empty());
    assert_eq!(manager.spare_cells.len(), 2);
    assert!(manager.persistent.is_empty());
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    assert_eq!(calls(&injector), 2);
}
