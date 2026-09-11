use std::collections::HashSet;

use super::*;

fn entry(unit_id: u32, name: &str) -> LootEntry {
    LootEntry {
        unit_id,
        timestamp_ms: 1000 + unit_id as u64,
        name: name.to_string(),
        quality: String::new(),
        color: None,
        pickup: PickupState::Pending,
        seed: 0,
    }
}

fn entry_with_seed(unit_id: u32, seed: u32, name: &str) -> LootEntry {
    let mut e = entry(unit_id, name);
    e.seed = seed;
    e
}

#[test]
fn push_adds_entry_to_back_and_index() {
    let mut h = LootHistory::new();
    h.push(entry(1, "A"));
    h.push(entry(2, "B"));
    assert_eq!(h.len(), 2);
    assert_eq!(h.snapshot()[0].name, "A");
    assert_eq!(h.snapshot()[1].name, "B");
}

#[test]
fn push_skips_duplicate_unit_id() {
    let mut h = LootHistory::new();
    h.push(entry(1, "A"));
    h.push(entry(1, "A again"));
    assert_eq!(
        h.len(),
        1,
        "duplicate unit_id should not produce a 2nd entry"
    );
    assert_eq!(h.snapshot()[0].name, "A");
}

#[test]
fn push_evicts_oldest_when_at_cap() {
    let mut h = LootHistory::new();
    for i in 0..MAX_ENTRIES as u32 {
        h.push(entry(i, "x"));
    }
    assert_eq!(h.len(), MAX_ENTRIES);
    h.push(entry(9999, "newest"));
    assert_eq!(h.len(), MAX_ENTRIES);
    let snap = h.snapshot();
    assert_eq!(snap[0].unit_id, 1, "oldest (unit_id=0) should be evicted");
    assert_eq!(snap[MAX_ENTRIES - 1].unit_id, 9999);
}

#[test]
fn pending_becomes_picked_up_when_in_our_inventory() {
    let mut h = LootHistory::new();
    h.push(entry(42, "x"));

    let mut ours: HashSet<u32> = HashSet::new();
    ours.insert(42);

    let updates = h.resolve_pending(&ours);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0], (42, 0, PickupState::PickedUp));
    assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);
}

#[test]
fn pending_stays_pending_when_not_in_inventory() {
    // Map change / area unload / item just sitting on ground:
    // no positive evidence → stay Pending forever.
    let mut h = LootHistory::new();
    h.push(entry(42, "x"));

    let ours: HashSet<u32> = HashSet::new();

    for _ in 0..1000 {
        let updates = h.resolve_pending(&ours);
        assert!(updates.is_empty(), "must not auto-transition");
    }
    assert_eq!(h.snapshot()[0].pickup, PickupState::Pending);
}

#[test]
fn terminal_states_do_not_emit_again() {
    let mut h = LootHistory::new();
    h.push(entry(42, "x"));

    let mut ours: HashSet<u32> = HashSet::new();
    ours.insert(42);

    let _ = h.resolve_pending(&ours); // PickedUp
    let updates = h.resolve_pending(&ours);
    assert!(updates.is_empty(), "terminal state must not re-emit");
}

#[test]
fn seed_merge_rekeys_existing_pending_entry() {
    // Same physical item seen again with a fresh unit_id after the
    // player teleported away and returned: merge into the existing
    // entry, do NOT add a duplicate row.
    let mut h = LootHistory::new();
    let outcome = h.push(entry_with_seed(42, 0xDEADBEEF, "TU Helm"));
    assert_eq!(outcome, PushOutcome::Inserted);
    assert_eq!(h.len(), 1);

    let outcome = h.push(entry_with_seed(99, 0xDEADBEEF, "TU Helm"));
    assert_eq!(outcome, PushOutcome::Merged);
    assert_eq!(h.len(), 1, "seed dedup must not create a 2nd row");

    let snap = h.snapshot();
    assert_eq!(snap[0].unit_id, 99, "uid must be re-keyed to new sighting");
    assert_eq!(snap[0].pickup, PickupState::Pending);
}

#[test]
fn seed_merge_emits_update_under_new_uid_after_rekey() {
    // After seed-merge, resolve_pending must report the *current* uid
    // (the rekeyed one) so the frontend can look up by stable seed.
    let mut h = LootHistory::new();
    h.push(entry_with_seed(42, 0xDEADBEEF, "TU Helm"));
    h.push(entry_with_seed(99, 0xDEADBEEF, "TU Helm")); // rekey to 99

    let mut ours: HashSet<u32> = HashSet::new();
    ours.insert(99); // picked up under new uid
    let updates = h.resolve_pending(&ours);
    assert_eq!(updates, vec![(99, 0xDEADBEEF, PickupState::PickedUp)]);
}

#[test]
fn seed_merge_does_not_disturb_terminal_entries() {
    let mut h = LootHistory::new();
    h.push(entry_with_seed(42, 0xCAFEBABE, "Rune"));

    let mut ours: HashSet<u32> = HashSet::new();
    ours.insert(42);
    let _ = h.resolve_pending(&ours);
    assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);

    let outcome = h.push(entry_with_seed(99, 0xCAFEBABE, "Rune"));
    assert_eq!(outcome, PushOutcome::Duplicate);
    assert_eq!(h.len(), 1);
    assert_eq!(h.snapshot()[0].pickup, PickupState::PickedUp);
    assert_eq!(h.snapshot()[0].unit_id, 42);
}

#[test]
fn lost_terminal_is_not_resurrected_by_seed_match() {
    // After menu-exit Lost, a coincidental seed sighting in a new
    // session must not flip the historical row back to Pending.
    let mut h = LootHistory::new();
    h.push(entry_with_seed(42, 0xABCD1234, "TU Helm"));
    let lost = h.mark_all_pending_lost();
    assert_eq!(lost, vec![(42, 0xABCD1234, PickupState::Lost)]);

    let outcome = h.push(entry_with_seed(99, 0xABCD1234, "TU Helm"));
    assert_eq!(outcome, PushOutcome::Duplicate);
    assert_eq!(h.len(), 1);
    assert_eq!(h.snapshot()[0].pickup, PickupState::Lost);
    assert_eq!(h.snapshot()[0].unit_id, 42);
}

#[test]
fn zero_seed_falls_back_to_unit_id_dedup() {
    // Items whose seed read failed (seed=0) must not collide via
    // the by_seed index — fall through to unit_id dedup.
    let mut h = LootHistory::new();
    let a = h.push(entry_with_seed(1, 0, "A"));
    let b = h.push(entry_with_seed(2, 0, "B"));
    assert_eq!(a, PushOutcome::Inserted);
    assert_eq!(b, PushOutcome::Inserted);
    assert_eq!(h.len(), 2);
}
