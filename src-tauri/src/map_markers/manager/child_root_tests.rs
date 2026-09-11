use super::test_support::native::{remains_quarantined, seeded};
use super::*;
use crate::process::marker_test_io::Operation;

#[test]
fn root_witness_rejects_swap_before_splice() {
    let (fixture, injector, mut manager) = seeded(1);
    let ctx = fixture.context();
    let replacement = fixture.address(0x9000);
    fixture.io.on_nth(
        (
            fixture.ctx.d2_client + d2client::AUTOMAP_LAYER,
            Operation::Read,
            2,
        ),
        move || {
            ctx.process
                .write_buffer(
                    ctx.d2_client + 0x1000 + automap_layer::P_OBJECTS,
                    &replacement.to_le_bytes(),
                )
                .unwrap();
        },
    );
    let writes = fixture.io.writes();

    let result = manager.detach_chain(&fixture.ctx);

    assert!(
        result.is_err(),
        "unreachable cached parent must not be spliced"
    );
    assert_eq!(fixture.io.writes(), writes + 1);
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x20000)
    );
    assert_eq!(
        manager.diagnostics.first.as_ref().unwrap().failure.stage,
        Stage::ChildRecheck
    );
    assert_eq!(manager.placed, [fixture.address(0x20000)]);
    assert!(manager.spare_cells.is_empty());
    remains_quarantined(&fixture, &mut manager, &injector);
}

fn assert_postwrite_root_swap(target: Option<usize>, issue: &str) {
    let (fixture, injector, mut manager) = seeded(1);
    let ctx = fixture.context();
    let replacement = target.map(|offset| fixture.address(offset)).unwrap_or(0);
    fixture.io.on_nth(
        (
            fixture.ctx.d2_client + 0x7000 + automap_cell::P_LESS,
            Operation::Written,
            1,
        ),
        move || {
            ctx.process
                .write_buffer(
                    ctx.d2_client + 0x1000 + automap_layer::P_OBJECTS,
                    &replacement.to_le_bytes(),
                )
                .unwrap();
        },
    );
    let writes = fixture.io.writes();

    let result = manager.detach_chain(&fixture.ctx);

    assert!(
        result.is_err(),
        "child must remain reachable through the cached parent"
    );
    assert_eq!(fixture.io.writes(), writes + 2);
    assert_eq!(
        fixture.word(0x7000 + automap_cell::P_LESS),
        fixture.address(0x8000)
    );
    let failure = manager.diagnostics.first.as_ref().unwrap().failure;
    assert_eq!(failure.stage, Stage::ChildPostverify);
    assert_eq!(serde_json::json!(failure.reason)["issue"], issue);
    assert_eq!(manager.placed, [fixture.address(0x20000)]);
    assert!(manager.spare_cells.is_empty());
    remains_quarantined(&fixture, &mut manager, &injector);
}

#[test]
fn root_witness_rejects_unrelated_tree_after_splice() {
    assert_postwrite_root_swap(Some(0x9000), "child_unreachable");
}

#[test]
fn root_witness_rejects_direct_child_root_after_splice() {
    assert_postwrite_root_swap(Some(0x8000), "child_parent_mismatch");
}

#[test]
fn root_witness_rejects_null_root_after_splice() {
    assert_postwrite_root_swap(None, "child_unreachable");
}

#[test]
fn late_root_walk_boundary_mutation_is_rejected_before_splice() {
    for offset in [d2client::AUTOMAP_LAYER, 0x7000 + automap_cell::P_LESS] {
        let (fixture, injector, mut manager) = seeded(1);
        let ctx = fixture.context();
        let replacement = fixture.address(0x9000);
        fixture.io.on_nth(
            (
                fixture.ctx.d2_client + 0x8000 + automap_cell::P_MORE,
                Operation::Read,
                2,
            ),
            move || {
                ctx.process
                    .write_buffer(ctx.d2_client + offset, &replacement.to_le_bytes())
                    .unwrap();
            },
        );
        let writes = fixture.io.writes();

        let result = manager.detach_chain(&fixture.ctx);

        assert!(result.is_err());
        assert_eq!(
            manager.diagnostics.first.as_ref().unwrap().failure.stage,
            Stage::ChildRecheck
        );
        assert_eq!(fixture.io.writes(), writes + 1);
        assert_eq!(
            fixture.word(0x7000 + automap_cell::P_LESS),
            if offset == d2client::AUTOMAP_LAYER {
                fixture.address(0x20000)
            } else {
                replacement
            }
        );
        assert_eq!(manager.placed, [fixture.address(0x20000)]);
        assert!(manager.spare_cells.is_empty());
        remains_quarantined(&fixture, &mut manager, &injector);
    }
}
