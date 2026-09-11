use super::*;

impl DpsMeter {
    /// Test helper — direct insert with explicit kill flag, no scaling.
    #[cfg(test)]
    fn ingest_test(&mut self, ts_ms: u32, damage: u32, is_kill: bool) {
        self.events.push_back(Event {
            ts_ms,
            damage,
            is_kill,
        });
        self.session_total += damage as u64;
        if is_kill {
            self.session_kills += 1;
        }
    }
}

#[test]
fn empty_meter_zero_dps() {
    let mut m = DpsMeter::new();
    let s = m.snapshot(1000);
    assert_eq!(s.dps, 0.0);
    assert_eq!(s.kpm, 0.0);
    assert_eq!(s.peak, 0.0);
    assert_eq!(s.total, 0);
    assert_eq!(s.kills, 0);
    assert!(!s.in_session);
}

#[test]
fn single_event_in_window() {
    let mut m = DpsMeter::new();
    m.ingest_test(1000, 500, false);
    let s = m.snapshot(2000);
    assert_eq!(s.dps, 100.0);
    assert_eq!(s.total, 500);
    assert_eq!(s.kills, 0);
    assert!(s.in_session);
}

#[test]
fn events_outside_window_dropped_but_total_persists() {
    let mut m = DpsMeter::new();
    m.ingest_test(1000, 1000, false);
    m.ingest_test(2000, 500, true);
    let s = m.snapshot(8000);
    assert_eq!(s.dps, 0.0);
    assert_eq!(s.kpm, 0.0);
    assert_eq!(s.total, 1500);
    assert_eq!(s.kills, 1);
}

#[test]
fn peak_tracks_max() {
    let mut m = DpsMeter::new();
    m.ingest_test(1000, 5000, false);
    let s1 = m.snapshot(1100);
    assert!(s1.peak >= s1.dps);
    m.ingest_test(7000, 100, false);
    let s2 = m.snapshot(7100);
    assert!(s2.peak >= s1.peak, "peak must be monotone non-decreasing");
}

#[test]
fn reset_clears_session_state() {
    let mut m = DpsMeter::new();
    m.ingest_test(1000, 500, true);
    m.snapshot(1100);
    m.reset();
    let s = m.snapshot(1200);
    assert_eq!(s.dps, 0.0);
    assert_eq!(s.peak, 0.0);
    assert_eq!(s.total, 0);
    assert_eq!(s.kills, 0);
    assert!(!s.in_session);
}

#[test]
fn ingest_falls_back_to_template_only_when_mlvl_zero() {
    let mut m = DpsMeter::new();
    m.ingest(1000, 32768, 600, 0);
    assert_eq!(m.session_total, 600);
    m.ingest(2000, 16384, 600, 0);
    assert_eq!(m.session_total, 900);
}

#[test]
fn ingest_scales_linearly_with_mlvl() {
    let mut m = DpsMeter::new();
    m.ingest(1000, 32768, 54, 110);
    assert_eq!(m.session_total, 5940);
    m.ingest(2000, 16384, 54, 110);
    assert_eq!(m.session_total, 5940 + 2970);
}

#[test]
fn ingest_decodes_kill_flag() {
    let mut m = DpsMeter::new();
    m.ingest(1000, 32768 | KILL_FLAG, 600, 110);
    assert_eq!(m.session_total, 66000);
    assert_eq!(m.session_kills, 1);
}

#[test]
fn kpm_rolling_window() {
    let mut m = DpsMeter::new();
    m.ingest_test(1000, 100, true);
    m.ingest_test(2000, 100, true);
    m.ingest_test(3000, 100, true);
    let s = m.snapshot(4000);
    assert_eq!(s.kpm, 36.0);
    assert_eq!(s.kills, 3);
}
