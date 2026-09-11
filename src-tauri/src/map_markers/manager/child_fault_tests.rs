use super::test_support::native::{remains_quarantined, seeded};
use super::*;
use crate::process::marker_test_io::Operation;

#[test]
fn quarantine_when_splice_write_fails_before_or_after_writing() {
    for operation in [Operation::Write, Operation::Written] {
        let (fixture, injector, mut manager) = seeded(1);
        fixture.io.fail(
            fixture.ctx.d2_client + 0x7000 + automap_cell::P_LESS,
            operation,
        );

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(
            manager.diagnostics.first.as_ref().unwrap().failure.stage,
            Stage::ChildWrite
        );
        assert_eq!(
            fixture.word(0x7000 + automap_cell::P_LESS),
            match operation {
                Operation::Write => fixture.address(0x20000),
                Operation::Written => fixture.address(0x8000),
                Operation::Read => unreachable!(),
            }
        );
        assert_eq!(manager.placed.len(), 1);
        assert!(manager.spare_cells.is_empty());
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_write_when_child_preflight_read_fails() {
    for offset in [
        d2client::AUTOMAP_LAYER,
        0x1000 + automap_layer::P_OBJECTS,
        0x8000 + automap_cell::P_LESS,
        0x8000 + automap_cell::P_MORE,
    ] {
        let (fixture, injector, mut manager) = seeded(1);
        fixture
            .io
            .fail(fixture.ctx.d2_client + offset, Operation::Read);
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes);
        assert!(matches!(
            manager.diagnostics.first.as_ref().unwrap().failure.reason,
            Reason::ReadFailed { .. }
        ));
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_write_when_recheck_observes_mutation() {
    for offset in [
        d2client::AUTOMAP_LAYER,
        0x7000 + automap_cell::P_LESS,
        0x20000 + automap_cell::P_LESS,
        0x20000 + automap_cell::P_MORE,
        0x20040 + automap_cell::P_LESS,
        0x20040 + automap_cell::P_MORE,
    ] {
        let (fixture, injector, mut manager) = seeded(2);
        let ctx = fixture.context();
        let changed = fixture.address(0x9000);
        fixture.io.on_nth(
            (
                fixture.ctx.d2_client + d2client::AUTOMAP_LAYER,
                Operation::Read,
                2,
            ),
            move || {
                ctx.process
                    .write_buffer(ctx.d2_client + offset, &changed.to_le_bytes())
                    .unwrap();
            },
        );
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes + 1);
        assert_eq!(
            manager.diagnostics.first.as_ref().unwrap().failure.stage,
            Stage::ChildRecheck
        );
        assert_eq!(manager.placed.len(), 2);
        assert!(manager.spare_cells.is_empty());
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_without_write_when_recheck_read_fails() {
    let (fixture, injector, mut manager) = seeded(1);
    let address = fixture.ctx.d2_client + 0x20000 + automap_cell::P_MORE;
    fixture.io.on_nth((address, Operation::Read, 3), move || {
        crate::process::marker_test_io::fail_next(address, Operation::Read);
    });
    let writes = fixture.io.writes();

    assert!(manager.detach_chain(&fixture.ctx).is_err());

    assert_eq!(fixture.io.writes(), writes);
    assert_eq!(
        manager.diagnostics.first.as_ref().unwrap().failure.stage,
        Stage::ChildRecheck
    );
    remains_quarantined(&fixture, &mut manager, &injector);
}

#[test]
fn quarantine_when_postcheck_observes_changed_parent_layer_or_reachable_owned_cell() {
    for (offset, target) in [
        (d2client::AUTOMAP_LAYER, 0x1800),
        (0x7000 + automap_cell::P_LESS, 0x9000),
        (0x8000 + automap_cell::P_MORE, 0x20000),
        (0x8000 + automap_cell::P_MORE, 0x20040),
        (0x8000 + automap_cell::P_MORE, 0x8000),
    ] {
        let (fixture, injector, mut manager) = seeded(2);
        let ctx = fixture.context();
        let changed = fixture.address(target);
        fixture.io.on_nth(
            (
                fixture.ctx.d2_client + 0x7000 + automap_cell::P_LESS,
                Operation::Written,
                1,
            ),
            move || {
                ctx.process
                    .write_buffer(ctx.d2_client + offset, &changed.to_le_bytes())
                    .unwrap();
            },
        );
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes + 2);
        assert_eq!(
            manager.diagnostics.first.as_ref().unwrap().failure.stage,
            Stage::ChildPostverify
        );
        assert_eq!(manager.placed.len(), 2);
        assert!(manager.spare_cells.is_empty());
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_when_postcheck_cannot_read_layer_parent_root_or_child() {
    for offset in [
        d2client::AUTOMAP_LAYER,
        0x7000 + automap_cell::P_LESS,
        0x1000 + automap_layer::P_OBJECTS,
        0x8000 + automap_cell::P_LESS,
        0x8000 + automap_cell::P_MORE,
    ] {
        let (fixture, injector, mut manager) = seeded(1);
        let address = fixture.ctx.d2_client + offset;
        fixture.io.on_nth(
            (
                fixture.ctx.d2_client + 0x7000 + automap_cell::P_LESS,
                Operation::Written,
                1,
            ),
            move || {
                crate::process::marker_test_io::fail_next(address, Operation::Read);
            },
        );
        let writes = fixture.io.writes();

        assert!(manager.detach_chain(&fixture.ctx).is_err());

        assert_eq!(fixture.io.writes(), writes + 1);
        let failure = manager.diagnostics.first.as_ref().unwrap().failure;
        assert_eq!(failure.stage, Stage::ChildPostverify);
        assert_eq!(
            failure.reason,
            Reason::ReadFailed {
                slot: Some(fixture.address(offset))
            }
        );
        assert_eq!(
            fixture.word(0x7000 + automap_cell::P_LESS),
            fixture.address(0x8000)
        );
        assert_eq!(manager.placed.len(), 1);
        assert!(manager.spare_cells.is_empty());
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}

#[test]
fn quarantine_when_postcheck_root_exceeds_bound() {
    let (fixture, injector, mut manager) = seeded(1);
    for index in 0..4096 {
        let next = if index == 4095 {
            0
        } else {
            fixture.address(0x90000 + (index + 1) * 0x20)
        };
        fixture.seed(0x90000 + index * 0x20 + automap_cell::P_LESS, next);
    }
    let ctx = fixture.context();
    let subtree = fixture.address(0x90000);
    fixture.io.on_nth(
        (
            fixture.ctx.d2_client + 0x7000 + automap_cell::P_LESS,
            Operation::Written,
            1,
        ),
        move || {
            ctx.process
                .write_buffer(
                    ctx.d2_client + 0x8000 + automap_cell::P_MORE,
                    &subtree.to_le_bytes(),
                )
                .unwrap();
        },
    );
    let writes = fixture.io.writes();

    assert!(manager.detach_chain(&fixture.ctx).is_err());

    assert_eq!(fixture.io.writes(), writes + 2);
    let failure = manager.diagnostics.first.as_ref().unwrap().failure;
    assert_eq!(failure.stage, Stage::ChildPostverify);
    assert!(matches!(
        failure.reason,
        Reason::ChildTopology {
            issue: diagnostics::TopologyIssue::BoundExceeded,
            ..
        }
    ));
    remains_quarantined(&fixture, &mut manager, &injector);
}
