use super::*;
use crate::rules::Visibility;

#[test]
fn visibility_mask_ops_clear_stale_opposing_bits() {
    for (visibility, expected) in [
        (
            Visibility::Show,
            &[VisibilityMaskOp::SetShow, VisibilityMaskOp::ClearHide][..],
        ),
        (
            Visibility::Hide,
            &[VisibilityMaskOp::SetHide, VisibilityMaskOp::ClearShow][..],
        ),
        (
            Visibility::Default,
            &[VisibilityMaskOp::ClearShow, VisibilityMaskOp::ClearHide][..],
        ),
    ] {
        assert_eq!(visibility_mask_ops(visibility), expected);
    }
}
