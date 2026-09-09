use super::*;
use crate::unit_stats_reader::stat_acquisition_fixtures::{fixture, PLAYER};

#[test]
fn stat_acquisition_damage_preserves_error_on_legacy_failure() {
    let fixture = fixture(&[]);
    fixture.seed(0x2000 + crate::offsets::stat_list::UNIT_TO_STATS_LIST, 1);
    let injector = fixture.injector();

    let result = read_unit_damage_stats(&fixture.ctx, &injector, PLAYER);

    assert!(result.is_err());
    assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 1);
    assert_eq!(
        injector.telemetry.snapshot().consumers[2].single,
        [1, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        injector.telemetry.snapshot().consumers[2].fallback,
        [1, 0, 1]
    );
}

#[test]
fn stat_acquisition_damage_preserves_weapon_formulas_with_direct_values() {
    let fixture = fixture(&[
        (0, 100),
        (2, 40),
        (21, 10),
        (22, 8),
        (25, 50),
        (48, -7),
        (57, -257),
        (58, 511),
        (159, 5),
        (160, 7),
    ]);
    let injector = fixture.injector();

    let result = read_unit_damage_stats(&fixture.ctx, &injector, PLAYER)
        .unwrap()
        .unwrap();

    assert_eq!((result.phys_min_1h, result.phys_max_1h), (26, 29));
    assert_eq!((result.phys_min_2h, result.phys_max_2h), (13, 18));
    assert_eq!(
        (result.str_damage_bonus_pct, result.dex_damage_bonus_pct),
        (100, 20)
    );
    assert_eq!(
        (result.poison_min_per_sec, result.poison_max_per_sec),
        (-26, 49)
    );
    assert_eq!((result.fire_min, result.fire_max), (-7, 0));
    assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 0);
}

#[test]
fn stat_acquisition_damage_two_handed_merc_preserves_floor_and_range_repair() {
    let fixture = fixture(&[(0, -1), (2, 3), (23, 10), (24, 4), (25, -10)]);
    fixture
        .ctx
        .process
        .write_buffer(
            fixture.ctx.d2_client + 0xe000 + items_txt::RECORD_SIZE + items_txt::IS_2H,
            &[1],
        )
        .unwrap();
    let injector = fixture.injector();

    let result = read_unit_damage_stats(
        &fixture.ctx,
        &injector,
        crate::unit_stats_reader::stat_acquisition_fixtures::MERC,
    )
    .unwrap()
    .unwrap();

    assert_eq!((result.phys_min_1h, result.phys_max_1h), (0, 0));
    assert_eq!((result.phys_min_2h, result.phys_max_2h), (8, 9));
    assert_eq!(
        (result.str_damage_bonus_pct, result.dex_damage_bonus_pct),
        (-1, 1)
    );
    assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 0);
}
