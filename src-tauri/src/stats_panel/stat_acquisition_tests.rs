use super::*;
use crate::unit_stats_reader::stat_acquisition_fixtures::{fixture, PLAYER};

#[test]
fn stat_acquisition_previous_values_survive_legacy_failure_without_double_scaling() {
    let fixture = fixture(&[]);
    fixture.seed(0x2000 + stat_list::UNIT_TO_STATS_LIST, 0);
    let injector = fixture.injector();
    let previous = CharacterStats {
        class: 271,
        stats: BTreeMap::from([(6, -7), (7, 123), (1, 100), (12, 2), (485, 105), (488, 50)]),
        base_stats: BTreeMap::new(),
    };

    let result =
        read_unit_character_stats(&fixture.ctx, &injector, PLAYER, Some(&previous)).unwrap();

    assert_eq!(result.stats[&6], -7);
    assert_eq!(result.stats[&7], 123);
    assert_eq!(result.stats[&904], 157);
    assert_eq!(result.stats[&907], 131);
    assert_eq!(result.stats[&905], 500);
    assert_eq!(result.stats[&906], 1500);
    assert_eq!(
        injector.telemetry.snapshot().injector_attempts[3],
        u64::try_from(STAT_IDS.len()).unwrap()
    );
}

#[test]
fn stat_acquisition_valid_bulk_uses_no_injection_for_player_and_merc() {
    for offset in [
        PLAYER,
        crate::unit_stats_reader::stat_acquisition_fixtures::MERC,
    ] {
        let fixture = fixture(&[
            (1, 100),
            (6, -257),
            (7, 511),
            (12, 2),
            (485, 105),
            (488, 50),
        ]);
        let injector = fixture.injector();
        fixture.seed(0x2000 + unit::UNIT_TYPE, u32::from(offset != PLAYER));
        let previous = CharacterStats {
            class: 271,
            stats: BTreeMap::from([(93, 987)]),
            base_stats: BTreeMap::new(),
        };

        let result =
            read_unit_character_stats(&fixture.ctx, &injector, offset, Some(&previous)).unwrap();

        assert_eq!(result.stats[&6], -2);
        assert_eq!(result.stats[&7], 1);
        assert_eq!(result.stats[&904], 157);
        assert_eq!(result.stats[&907], 131);
        assert_eq!(result.stats[&905], 500);
        assert_eq!(result.stats[&906], 1500);
        assert_eq!(result.stats[&93], 0);
        assert_eq!(result.base_stats[&1], 100);
        assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 0);
        let metrics = injector.telemetry.snapshot().consumers[0];
        assert_eq!(
            metrics.bulk,
            [1, 1, 0, 0, 6, u64::try_from(STAT_IDS.len() - 6).unwrap()]
        );
        assert_eq!(metrics.fallback, [0; 3]);
    }
}

#[test]
fn stat_acquisition_rejected_bulk_only_runs_legacy_sweep_each_poll() {
    for values in [
        vec![],
        vec![(12, 0)],
        vec![(12, -2)],
        vec![(12, 2), (1, 17)],
    ] {
        let fixture = fixture(&values);
        let injector = fixture.injector();
        let previous = CharacterStats {
            class: 271,
            stats: BTreeMap::from([(6, -9), (12, 2)]),
            base_stats: BTreeMap::new(),
        };

        for _ in 0..12 {
            let result =
                read_unit_character_stats(&fixture.ctx, &injector, PLAYER, Some(&previous))
                    .unwrap();
            assert_eq!(result.stats[&6], -9);
        }

        let snapshot = injector.telemetry.snapshot();
        let metrics = snapshot.consumers[0];
        let requests = u64::try_from(STAT_IDS.len()).unwrap() * 12;
        assert_eq!(snapshot.injector_attempts[3], requests);
        assert_eq!(metrics.fallback, [requests, 0, requests]);
        assert_eq!(metrics.single, [0; 6]);
        assert_eq!(metrics.bulk[0], 12);
        if values.len() == 2 {
            assert_eq!(&metrics.bulk[1..4], &[0, 12, 0]);
        } else {
            assert_eq!(&metrics.bulk[1..4], &[12, 0, 12]);
        }
    }
}
