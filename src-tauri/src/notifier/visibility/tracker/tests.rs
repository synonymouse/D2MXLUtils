use super::*;

fn ids(values: &[u32]) -> HashSet<u32> {
    values.iter().copied().collect()
}

#[test]
fn clears_after_threshold_even_if_scan_seen_state_would_drop_id() {
    let mut tracker = HookBitTracker::new(2);
    tracker.mark_written(42);

    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
    assert_eq!(tracker.tracked_len(), 1);

    assert_eq!(tracker.plan_clears(&ids(&[])), vec![42]);
    assert_eq!(tracker.tracked_len(), 1);

    tracker.confirm_cleared(&[42]);
    assert_eq!(tracker.tracked_len(), 0);
    assert_eq!(tracker.missed_len(), 0);
}

#[test]
fn retries_until_clear_is_confirmed() {
    let mut tracker = HookBitTracker::new(2);
    tracker.mark_written(7);

    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
    assert_eq!(tracker.plan_clears(&ids(&[])), vec![7]);
    assert_eq!(tracker.plan_clears(&ids(&[])), vec![7]);

    tracker.confirm_cleared(&[7]);
    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
}

#[test]
fn reappearing_item_resets_missed_count() {
    let mut tracker = HookBitTracker::new(2);
    tracker.mark_written(99);

    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
    assert_eq!(tracker.missed_len(), 1);

    assert_eq!(tracker.plan_clears(&ids(&[99])), Vec::<u32>::new());
    assert_eq!(tracker.missed_len(), 0);

    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
    assert_eq!(tracker.plan_clears(&ids(&[])), vec![99]);
}

#[test]
fn current_item_with_same_mask_index_blocks_clear() {
    let old_unit_id = 9;
    let colliding_live_unit_id = old_unit_id + 0x1_0000;
    let mut tracker = HookBitTracker::new(2);
    tracker.mark_written(old_unit_id);

    assert_eq!(
        tracker.plan_clears(&ids(&[colliding_live_unit_id])),
        Vec::<u32>::new()
    );
    assert_eq!(
        tracker.plan_clears(&ids(&[colliding_live_unit_id])),
        Vec::<u32>::new()
    );
    assert_eq!(tracker.missed_len(), 0);

    assert_eq!(tracker.plan_clears(&ids(&[])), Vec::<u32>::new());
    assert_eq!(tracker.plan_clears(&ids(&[])), vec![old_unit_id]);
}

#[test]
fn departed_mask_collision_requires_current_item_reset() {
    let old_unit_id = 9;
    let colliding_live_unit_id = old_unit_id + 0x1_0000;
    let current = ids(&[colliding_live_unit_id]);
    let mut tracker = HookBitTracker::new(2);
    tracker.mark_written(old_unit_id);

    assert_eq!(
        tracker.departed_mask_collisions(colliding_live_unit_id, &current),
        vec![old_unit_id]
    );
    assert_eq!(
        tracker.departed_mask_collisions(old_unit_id, &current),
        Vec::<u32>::new()
    );
    assert_eq!(
        tracker.departed_mask_collisions(10, &current),
        Vec::<u32>::new()
    );
}

#[test]
fn pending_visibility_ops_retry_only_failed_ops_until_success() {
    let mut pending = PendingVisibilityMaskOps::new();

    pending.record_failed(
        55,
        vec![VisibilityMaskOp::SetHide, VisibilityMaskOp::ClearShow],
    );
    assert_eq!(
        pending.take(55),
        vec![VisibilityMaskOp::SetHide, VisibilityMaskOp::ClearShow]
    );

    pending.record_failed(55, vec![VisibilityMaskOp::ClearShow]);
    assert_eq!(pending.take(55), vec![VisibilityMaskOp::ClearShow]);

    pending.record_failed(55, Vec::new());
    assert_eq!(pending.take(55), Vec::<VisibilityMaskOp>::new());
}

#[test]
fn cleanup_failure_throttle_suppresses_and_resets() {
    let mut throttle = HookCleanupFailureLogThrottle::new(2);

    assert_eq!(throttle.record_failure(), Some(0));
    assert_eq!(throttle.record_failure(), None);
    assert_eq!(throttle.record_failure(), None);
    assert_eq!(throttle.record_failure(), Some(2));
    assert_eq!(throttle.record_failure(), None);

    throttle.reset();

    assert_eq!(throttle.record_failure(), Some(0));
}
