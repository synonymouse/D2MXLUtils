//! Pure-data DPS accumulator. The trampoline drains HP-write events
//! into `ingest`; the scanner tick polls `snapshot`.
//!
//! In MP the server normalises HP to 0..=32768 over the wire, so actual
//! max HP isn't available client-side. We approximate with a linear
//! scale on runtime monster level (`stat 12`):
//!   `damage = (delta_raw / 32768) * MonStats.wMaxHP[diff] * mLvl`
//! See `docs/dps-meter-scaling-investigation.md` §11.
//!
//! Bit 31 of `delta_raw` carries the kill flag: trampoline ORs
//! `0x8000_0000` when the new HP value is 0.

use std::collections::VecDeque;

pub const WINDOW_SECONDS: f32 = 5.0;
const WINDOW_MS: u32 = 5_000;
const KILL_FLAG: u32 = 0x8000_0000;
const DELTA_MASK: u32 = 0x7FFF_FFFF;

/// Must share the trampoline's time base (`GetTickCount`); otherwise
/// `snapshot(now) - event.ts_ms` is meaningless.
#[cfg(target_os = "windows")]
pub fn now_ms() -> u32 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    unsafe { GetTickCount() }
}

/// Wine's `GetTickCount` (like real Windows') is system-wide, not
/// per-process — implemented on top of the host's monotonic clock, the
/// same one every process (including this one) can read directly. So,
/// same as the Windows arm above avoiding a call into the remote process
/// entirely, we can too: `CLOCK_MONOTONIC` read locally lines up with
/// what the trampoline's remote `GetTickCount()` call embeds, without
/// needing a ptrace round-trip on every tick.
#[cfg(target_os = "linux")]
pub fn now_ms() -> u32 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ((ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000)) as u32
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn now_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy)]
struct Event {
    ts_ms: u32,
    damage: u32,
    is_kill: bool,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DpsSnapshot {
    pub dps: f32,
    pub kpm: f32,
    pub peak: f32,
    pub total: u64,
    pub kills: u32,
    pub in_session: bool,
}

pub struct DpsMeter {
    events: VecDeque<Event>,
    session_total: u64,
    session_peak: f32,
    session_kills: u32,
}

impl DpsMeter {
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            session_total: 0,
            session_peak: 0.0,
            session_kills: 0,
        }
    }

    pub fn reset(&mut self) {
        self.events.clear();
        self.session_total = 0;
        self.session_peak = 0.0;
        self.session_kills = 0;
    }

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

    /// `monster_level == 0` means the trampoline didn't find `stat 12`
    /// in the unit's stat list; fall back to ×1 so behaviour degrades
    /// gracefully instead of zeroing every event.
    pub fn ingest(
        &mut self,
        ts_ms: u32,
        delta_raw_with_flag: u32,
        max_hp: u16,
        monster_level: u16,
    ) {
        let is_kill = (delta_raw_with_flag & KILL_FLAG) != 0;
        let delta_raw = delta_raw_with_flag & DELTA_MASK;
        let scale = if monster_level == 0 {
            1u64
        } else {
            monster_level as u64
        };
        let damage = ((delta_raw as u64)
            .saturating_mul(max_hp as u64)
            .saturating_mul(scale)
            / 32768) as u32;
        if damage == 0 && !is_kill {
            return;
        }
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

    pub fn snapshot(&mut self, now_ms: u32) -> DpsSnapshot {
        // wrapping_sub keeps the age check correct across GetTickCount's
        // ~49-day wraparound.
        while let Some(e) = self.events.front() {
            if now_ms.wrapping_sub(e.ts_ms) > WINDOW_MS {
                self.events.pop_front();
            } else {
                break;
            }
        }
        let window_dmg: u64 = self.events.iter().map(|e| e.damage as u64).sum();
        let window_kills: u32 = self.events.iter().filter(|e| e.is_kill).count() as u32;
        let dps = window_dmg as f32 / WINDOW_SECONDS;
        let kpm = (window_kills as f32) * (60.0 / WINDOW_SECONDS);
        if dps > self.session_peak {
            self.session_peak = dps;
        }
        DpsSnapshot {
            dps,
            kpm,
            peak: self.session_peak,
            total: self.session_total,
            kills: self.session_kills,
            in_session: !self.events.is_empty() || self.session_total > 0 || self.session_kills > 0,
        }
    }
}

impl Default for DpsMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
