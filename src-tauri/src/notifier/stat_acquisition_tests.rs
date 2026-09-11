use super::*;
use crate::map_markers::test_support::Fixture;
use crate::offsets::item_flags;
use crate::stat_telemetry::{InjectorCall, StatConsumer, TelemetrySnapshot};

fn scanner_fixture(socketed: bool) -> (Fixture, DropScanner) {
    let fixture = Fixture::new();
    fixture.seed(0x4000 + paths::TO_PATHS_PTR[3], fixture.address(0x7000));
    fixture.seed(0x4000 + paths::TO_PATHS_COUNT[3], 1);
    fixture.seed(0x7000, fixture.address(0x6000));
    fixture.seed(0x6000 + paths::PATH_TO_UNIT, fixture.address(0x5000));
    fixture.seed(0x5000 + unit::UNIT_TYPE, unit_type::ITEM);
    fixture.seed(0x5000 + unit::UNIT_ID, 47);
    fixture.seed(0x5000 + unit::UNIT_DATA, fixture.address(0xa000));
    fixture.seed(
        0xa000 + item_data::FLAGS,
        if socketed { item_flags::SOCKETED } else { 0 },
    );
    let state = Arc::new(SharedScannerState::new(
        fixture.context(),
        fixture.injector(),
        Default::default(),
    ));
    let mut scanner = DropScanner::new(state, Arc::new(RwLock::new(Default::default()))).unwrap();
    scanner.class_cache = Some(Vec::new());
    scanner.unique_cache = Some(Vec::new());
    scanner.set_cache = Some(Vec::new());
    scanner.char_level = 83;
    (fixture, scanner)
}

fn seed_stat(fixture: &Fixture, unit_offset: usize, value: Option<i32>) {
    let descriptor = unit_offset + 0x100;
    let records = unit_offset + 0x200;
    let id = match unit_offset {
        0x2000 => stat_list::STAT_LEVEL,
        0x5000 => stat_list::STAT_SOCKETS,
        _ => panic!("unexpected fixture unit"),
    };
    fixture.seed(
        unit_offset + stat_list::UNIT_TO_STATS_LIST,
        fixture.address(descriptor),
    );
    fixture.seed(descriptor + stat_list::SL_PSTAT, fixture.address(records));
    fixture.seed(
        descriptor + stat_list::SL_STAT_COUNT,
        u32::from(value.is_some()),
    );
    if let Some(value) = value {
        fixture.seed(records, u32::from(id) << 16);
        fixture.seed(records + 4, u32::from_ne_bytes(value.to_ne_bytes()));
    }
}

fn metrics(scanner: &DropScanner) -> TelemetrySnapshot {
    scanner.state.injector.lock().unwrap().telemetry.snapshot()
}

#[test]
fn stat_acquisition_baseline_unsocketed_item_skips_socket_query() {
    let (fixture, mut scanner) = scanner_fixture(false);
    seed_stat(&fixture, 0x5000, Some(6));

    let events = scanner.tick_items();

    assert_eq!(events[0].sockets, 0);
    assert!(!events[0].runtime_stats_loaded);
    let snapshot = metrics(&scanner);
    assert_eq!(
        snapshot.injector_attempts[InjectorCall::GetUnitStat.index()],
        1
    );
    assert_eq!(
        snapshot.consumers[StatConsumer::Sockets.index()].single,
        [0; 6]
    );
}

#[test]
fn stat_acquisition_event_uses_direct_level_and_socket_boundaries_without_injection() {
    for (level, sockets) in [(1, 1), (147, 6), (i32::MAX, 6)] {
        let (fixture, mut scanner) = scanner_fixture(true);
        seed_stat(&fixture, 0x2000, Some(level));
        seed_stat(&fixture, 0x5000, Some(sockets));

        let events = scanner.tick_items();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].clvl, u32::try_from(level).unwrap());
        assert_eq!(events[0].sockets, u8::try_from(sockets).unwrap());
        assert!(!events[0].runtime_stats_loaded);
        let snapshot = metrics(&scanner);
        assert_eq!(snapshot.injector_attempts, [0; 5]);
        for consumer in [StatConsumer::Level, StatConsumer::Sockets] {
            assert_eq!(
                snapshot.consumers[consumer.index()].single,
                [1, 1, 0, 0, 1, 0]
            );
            assert_eq!(snapshot.consumers[consumer.index()].fallback, [0; 3]);
        }
    }
}

#[test]
fn stat_acquisition_retains_previous_level_when_missing_or_nonpositive_and_fallback_fails() {
    for value in [None, Some(0), Some(-1), Some(i32::MIN)] {
        let (fixture, mut scanner) = scanner_fixture(false);
        seed_stat(&fixture, 0x2000, value);

        let events = scanner.tick_items();

        assert_eq!(events[0].clvl, 83);
        let snapshot = metrics(&scanner);
        let found = u64::from(value.is_some());
        assert_eq!(
            snapshot.consumers[StatConsumer::Level.index()].single,
            [1, 1, 0, 1, found, 1 - found]
        );
        assert_eq!(
            snapshot.consumers[StatConsumer::Level.index()].fallback,
            [1, 0, 1]
        );
        assert_eq!(
            snapshot.injector_attempts[InjectorCall::GetUnitStat.index()],
            1
        );
    }
}

#[test]
fn stat_acquisition_retains_socket_default_when_missing_or_invalid_and_fallback_fails() {
    for value in [None, Some(0), Some(7), Some(-1), Some(i32::MIN)] {
        let (fixture, mut scanner) = scanner_fixture(true);
        seed_stat(&fixture, 0x2000, Some(147));
        seed_stat(&fixture, 0x5000, value);

        let events = scanner.tick_items();

        assert_eq!((events[0].clvl, events[0].sockets), (147, 0));
        assert!(!events[0].runtime_stats_loaded);
        let snapshot = metrics(&scanner);
        let found = u64::from(value.is_some());
        assert_eq!(
            snapshot.consumers[StatConsumer::Sockets.index()].single,
            [1, 1, 0, 1, found, 1 - found]
        );
        assert_eq!(
            snapshot.consumers[StatConsumer::Sockets.index()].fallback,
            [1, 0, 1]
        );
        assert_eq!(
            snapshot.injector_attempts[InjectorCall::GetUnitStat.index()],
            1
        );
    }
}

#[test]
fn stat_acquisition_failed_memory_reads_attempt_one_fallback_per_consumer() {
    let (fixture, mut scanner) = scanner_fixture(true);
    fixture.seed(0x2000 + stat_list::UNIT_TO_STATS_LIST, 1);
    fixture.seed(0x5000 + stat_list::UNIT_TO_STATS_LIST, 1);

    let events = scanner.tick_items();

    assert_eq!((events[0].clvl, events[0].sockets), (83, 0));
    let snapshot = metrics(&scanner);
    for consumer in [StatConsumer::Level, StatConsumer::Sockets] {
        assert_eq!(
            snapshot.consumers[consumer.index()].single,
            [1, 0, 1, 0, 0, 0]
        );
        assert_eq!(snapshot.consumers[consumer.index()].fallback, [1, 0, 1]);
    }
    assert_eq!(
        snapshot.injector_attempts[InjectorCall::GetUnitStat.index()],
        2
    );
}

#[test]
fn stat_acquisition_persistent_failure_keeps_existing_tick_and_seen_item_gates() {
    let (_fixture, mut scanner) = scanner_fixture(true);

    let batches: Vec<_> = (0..20).map(|_| scanner.tick_items()).collect();

    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(scanner.char_level, 83);
    let snapshot = metrics(&scanner);
    assert_eq!(
        snapshot.consumers[StatConsumer::Level.index()].fallback,
        [20, 0, 20]
    );
    assert_eq!(
        snapshot.consumers[StatConsumer::Sockets.index()].fallback,
        [1, 0, 1]
    );
    assert_eq!(
        snapshot.injector_attempts[InjectorCall::GetUnitStat.index()],
        21
    );
}
