use super::fallback::*;
use super::stat_acquisition_fixtures::fixture;
use super::{StatReadResult, StatReaderError};
use crate::stat_telemetry::StatConsumer;

#[test]
fn stat_acquisition_single_policy_distinguishes_optional_level_and_sockets() {
    for consumer in StatConsumer::ALL {
        for value in [
            StatReadResult::Missing,
            StatReadResult::Found(-1),
            StatReadResult::Found(0),
            StatReadResult::Found(1),
            StatReadResult::Found(6),
            StatReadResult::Found(7),
            StatReadResult::Found(i32::MAX),
        ] {
            let fixture = fixture(&[]);
            let injector = fixture.injector();
            let context = StatReadContext::new(&fixture.ctx, &injector, consumer);
            let expected = match (consumer, value) {
                (
                    StatConsumer::Stats | StatConsumer::Breakpoints | StatConsumer::Damage,
                    StatReadResult::Missing,
                ) => 0,
                (
                    StatConsumer::Stats | StatConsumer::Breakpoints | StatConsumer::Damage,
                    StatReadResult::Found(raw),
                ) => raw,
                (StatConsumer::Level, StatReadResult::Found(raw)) if raw > 0 => raw,
                (StatConsumer::Sockets, StatReadResult::Found(raw)) if (1..=6).contains(&raw) => {
                    raw
                }
                (StatConsumer::Level | StatConsumer::Sockets, _) => -777,
            };

            let result = context.single_with(
                93,
                || Ok(value),
                || Ok(u32::from_ne_bytes((-777i32).to_ne_bytes())),
            );

            assert_eq!(result, Ok(expected));
            let snapshot = injector.telemetry.snapshot().consumers[consumer.index()];
            let rejected = u64::from(expected == -777);
            let found = u64::from(matches!(value, StatReadResult::Found(_)));
            assert_eq!(snapshot.single, [1, 1, 0, rejected, found, 1 - found]);
            assert_eq!(snapshot.fallback, [rejected, rejected, 0]);
        }
    }
}

#[test]
fn stat_acquisition_legacy_signed_bits_roundtrip_after_reader_error() {
    for raw in [i32::MIN, -257, -1, 0, 1, i32::MAX] {
        let fixture = fixture(&[]);
        let injector = fixture.injector();
        let context = StatReadContext::new(&fixture.ctx, &injector, StatConsumer::Damage);

        let result = context.single_with(
            57,
            || Err(StatReaderError::NullStatList),
            || Ok(u32::from_ne_bytes(raw.to_ne_bytes())),
        );

        assert_eq!(result, Ok(raw));
        let snapshot = injector.telemetry.snapshot().consumers[2];
        assert_eq!(snapshot.single, [1, 0, 1, 0, 0, 0]);
        assert_eq!(snapshot.fallback, [1, 1, 0]);
    }
}
