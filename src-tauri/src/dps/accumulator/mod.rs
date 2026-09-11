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
mod tests;
