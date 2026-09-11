use super::*;
use crate::unit_stats_reader::stat_acquisition_fixtures::{fixture, PLAYER};

#[test]
fn stat_acquisition_breakpoints_preserve_error_on_legacy_failure() {
    let fixture = fixture(&[]);
    fixture.seed(0x2000 + crate::offsets::stat_list::UNIT_TO_STATS_LIST, 1);
    let injector = fixture.injector();

    let result = read_unit_breakpoint_data(&fixture.ctx, &injector, PLAYER);

    assert!(result.is_err());
    assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 1);
    assert_eq!(
        injector.telemetry.snapshot().consumers[1].single,
        [1, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        injector.telemetry.snapshot().consumers[1].fallback,
        [1, 0, 1]
    );
}

#[test]
fn stat_acquisition_breakpoints_found_negative_and_missing_need_no_injection() {
    let fixture = fixture(&[(68, -3), (93, -17), (105, 40)]);
    let injector = fixture.injector();

    let result = read_unit_breakpoint_data(&fixture.ctx, &injector, PLAYER)
        .unwrap()
        .unwrap();

    assert_eq!(
        (result.ias, result.fcr, result.fhr, result.fbr),
        (-17, 40, 0, 0)
    );
    assert_eq!(
        (result.skill_ias, result.skill_fhr, result.merc_type),
        (-3, 0, Some(0))
    );
    assert_eq!(injector.telemetry.snapshot().injector_attempts[3], 0);
    assert_eq!(
        injector.telemetry.snapshot().consumers[1].single,
        [6, 6, 0, 0, 3, 3]
    );
}
