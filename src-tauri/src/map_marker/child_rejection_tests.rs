use super::child_detach_tests::{remains_quarantined, seeded};
use super::diagnostics::TopologyIssue;
use super::*;

#[test]
fn quarantine_without_writes_when_owned_links_are_not_a_tail_only_child() {
    for (offset, value) in [
        (0x20000 + automap_cell::P_LESS, 0x8080),
        (0x20000 + automap_cell::P_MORE, 0x8080),
        (0x20040 + automap_cell::P_MORE, 0x8080),
    ] {
        let (fixture, injector, mut manager) = seeded(2);
        fixture.seed(offset, fixture.address(value));
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_writes_when_current_or_recorded_layer_differs() {
    for mode in 0..4 {
        let (fixture, injector, mut manager) = seeded(1);
        match mode {
            0 => fixture.seed(d2client::AUTOMAP_LAYER, fixture.address(0x1800)),
            1 => manager.last_layer = fixture.address(0x1800),
            2 => manager.diagnostics.published_layer = None,
            3 => manager.diagnostics.published_layer = Some(fixture.address(0x1800)),
            _ => unreachable!(),
        }
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_writes_when_head_is_unreachable_or_parent_is_ambiguous() {
    for mode in 0..4 {
        let (fixture, injector, mut manager) = seeded(1);
        match mode {
            0 => fixture.seed(0x1000 + automap_layer::P_OBJECTS, fixture.address(0x9000)),
            1 => fixture.seed(0x7000 + automap_cell::P_LESS, fixture.address(0x9000)),
            2 => {
                fixture.seed(0x9000, manager.placed[0]);
                manager.chain_parent_slot = fixture.address(0x9000);
            }
            3 => fixture.seed(0x7000 + automap_cell::P_MORE, manager.placed[0]),
            _ => unreachable!(),
        }
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_writes_when_child_or_root_repeats_a_node() {
    for (offset, target) in [
        (0x8000 + automap_cell::P_LESS, 0x8000),
        (0x8040 + automap_cell::P_MORE, 0x8000),
        (0x8000 + automap_cell::P_MORE, 0x8040),
        (0x8000 + automap_cell::P_MORE, 0x20000),
        (0x8000 + automap_cell::P_MORE, 0x20040),
        (0x8000 + automap_cell::P_MORE, 0x7000),
        (0x7000 + automap_cell::P_MORE, 0x7000),
    ] {
        let (fixture, injector, mut manager) = seeded(2);
        fixture.seed(0x8000 + automap_cell::P_LESS, fixture.address(0x8040));
        fixture.seed(offset, fixture.address(target));
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        assert!(matches!(
            manager.diagnostics.first.as_ref().unwrap().failure.reason,
            Reason::ChildTopology {
                issue: TopologyIssue::RepeatedNode,
                ..
            }
        ));
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_writes_when_spare_intersects_child_or_owned_set() {
    for address in [0x8000, 0x8040, 0x20000] {
        let (fixture, injector, mut manager) = seeded(1);
        fixture.seed(0x8000 + automap_cell::P_MORE, fixture.address(0x8040));
        manager.spare_cells.push(fixture.address(address));
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_writes_when_child_address_arithmetic_overflows() {
    let (fixture, injector, mut manager) = seeded(1);
    fixture.seed(0x20000 + automap_cell::P_LESS, u32::MAX - 4);
    let writes = fixture.io.writes();

    assert!(manager.detach_chain(&fixture.ctx).is_err());

    assert_eq!(
        manager.diagnostics.first.as_ref().unwrap().failure.reason,
        Reason::ChildTopology {
            issue: TopologyIssue::AddressOverflow,
            node: u32::MAX - 4
        }
    );
    assert_eq!(fixture.io.writes(), writes);
    remains_quarantined(&fixture, &mut manager, &injector);
}

#[test]
fn bounded_root_walk_when_tree_is_at_or_over_limit() {
    for total in [4096, 4097] {
        let (fixture, injector, mut manager) = seeded(1);
        fixture.seed(0x8000 + automap_cell::P_LESS, fixture.address(0x90000));
        for index in 0..total - 3 {
            let next = if index + 1 == total - 3 {
                0
            } else {
                fixture.address(0x90000 + (index + 1) * 0x20)
            };
            fixture.seed(0x90000 + index * 0x20 + automap_cell::P_LESS, next);
        }
        let writes = fixture.io.writes();

        let result = manager.detach_chain(&fixture.ctx);

        assert_eq!(result.is_ok(), total == 4096);
        if total == 4097 {
            assert_eq!(fixture.io.writes(), writes);
            assert!(matches!(
                manager.diagnostics.first.as_ref().unwrap().failure.reason,
                Reason::ChildTopology {
                    issue: TopologyIssue::BoundExceeded,
                    ..
                }
            ));
            remains_quarantined(&fixture, &mut manager, &injector);
        }
    }
}

#[test]
fn quarantine_when_candidate_metadata_is_empty_duplicate_zero_or_unpublished() {
    for mode in 0..6 {
        let (fixture, injector, mut manager) = seeded(1);
        let reason = Reason::TailChild {
            index: 0,
            cell: manager.placed[0],
            expected_less: 0,
            actual_less: fixture.address(0x8000),
            expected_more: 0,
            actual_more: 0,
        };
        match mode {
            0 => manager.placed.clear(),
            1 => manager.placed.push(manager.placed[0]),
            2 => manager.placed[0] = 0,
            3 => manager.chain_parent_slot = 0,
            4 => fixture.seed(d2client::AUTOMAP_LAYER, 0),
            5 => manager.diagnostics.published_layer = Some(0),
            _ => unreachable!(),
        }
        let writes = fixture.io.writes();

        assert!(manager
            .detach_child(&fixture.ctx, (fixture.address(0x8000), reason))
            .is_err());

        assert_eq!(fixture.io.writes(), writes);
        assert!(!manager.cells_trusted);
        assert!(manager.diagnostics.first.is_some());
        if mode != 4 {
            remains_quarantined(&fixture, &mut manager, &injector);
        }
    }
}
