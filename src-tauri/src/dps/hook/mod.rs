//! Inline hook on `D2Common::STATLIST_SetUnitStat` (Ord10887). Captures
//! every monster HP write in both SP and MP.
//!
//! See `docs/dps-meter-reverse-engineering.md` for the call chain
//! and `docs/superpowers/specs/2026-05-07-dps-meter-design.md` for the
//! design rationale.

#![cfg(any(target_os = "windows", target_os = "linux"))]

#[cfg(target_os = "linux")]
mod linux;
mod ring;
mod trampoline;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Mutex;

use crate::remote_io::{read_remote, ProcessRef};

/// 1024 × 16 B = 16 KB — far more than a single scanner tick can
/// accumulate from one client.
const RING_CAPACITY: u32 = 1024;
/// 32 KB: trampoline + helper (~300 B) + ring (~16 KB), with headroom
/// for a future RING_CAPACITY bump.
const REGION_SIZE: usize = 0x8000;
const EXPECTED_PROLOGUE: [u8; 5] = [0x8B, 0x44, 0x24, 0x0C, 0x53];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrologueState {
    Original,
    ExistingHook { trampoline_addr: usize },
    Mismatch([u8; 5]),
}

#[derive(Debug, Clone, Copy)]
pub struct HookEvent {
    /// `GetTickCount` snapshot at hook time. Native u32 — wraps every
    /// ~49 days, which `DpsMeter::snapshot` handles via `wrapping_sub`.
    pub ts_ms: u32,
    pub unit_id: u32,
    pub delta_raw: u32,
    /// Template `wMaxHP[difficulty]` from MonStats.txt for this monster's
    /// class. Used together with `monster_level` to estimate runtime
    /// damage (server-side actual max HP isn't available client-side in MP).
    pub max_hp: u16,
    /// Runtime monster level (`stat 12`) read from the unit's stat list
    /// at hook time. Zero if the trampoline didn't find the stat — caller
    /// then falls back to ×1 scaling (legacy behaviour).
    pub monster_level: u16,
}

pub struct DpsHook {
    state: Mutex<Option<HookState>>,
}

#[allow(dead_code)]
struct HookState {
    /// Borrowed handle (Windows) / bare pid (Linux) — the process itself
    /// is owned/closed elsewhere (`D2Context`), not by us.
    process: ProcessRef,
    d2common_base: usize,
    region: usize,
    region_size: usize,
    ring_addr: usize,
    ring_capacity: u32,
    saved_bytes: [u8; 5],
    read_tail: u32,
}

// SAFETY: `HANDLE` is a kernel-object reference. Its Win32 R/W ops are
// thread-safe; we never close it. Mirrors `process::ProcessHandle`.
#[cfg(target_os = "windows")]
unsafe impl Send for HookState {}

impl DpsHook {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    pub fn is_installed(&self) -> bool {
        self.state.lock().map(|g| g.is_some()).unwrap_or(false)
    }
}

impl DpsHook {
    /// Drain pending events from the in-process ring buffer.
    pub fn drain(&self) -> Vec<HookEvent> {
        let mut state_lock = match self.state.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let state = match state_lock.as_mut() {
            Some(s) => s,
            None => return Vec::new(),
        };
        let mut reader = ring::RingReader {
            process: state.process,
            ring_addr: state.ring_addr,
            capacity: state.ring_capacity,
            tail: state.read_tail,
        };
        let events = reader.drain();
        state.read_tail = reader.tail;
        events
    }
}

impl Default for DpsHook {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for DpsHook {
    fn drop(&mut self) {
        let _ = self.uninstall();
    }
}

fn classify_prologue(ord10887_addr: usize, prologue: [u8; 5]) -> PrologueState {
    if prologue == EXPECTED_PROLOGUE {
        return PrologueState::Original;
    }

    if prologue[0] != 0xE9 {
        return PrologueState::Mismatch(prologue);
    }

    let rel = i32::from_le_bytes(prologue[1..5].try_into().unwrap());
    let target = ord10887_addr as i64 + 5 + rel as i64;
    if !(1..=u32::MAX as i64).contains(&target) {
        return PrologueState::Mismatch(prologue);
    }

    PrologueState::ExistingHook {
        trampoline_addr: target as usize,
    }
}

fn read_ring_head(handle: ProcessRef, ring_addr: usize) -> Result<u32, String> {
    let mut header = [0u8; ring::HEADER_SIZE];
    read_remote(handle, ring_addr, &mut header)
        .map_err(|e| format!("read existing DPS ring header: {}", e))?;
    let head = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let cap = u32::from_le_bytes(header[8..12].try_into().unwrap());
    if cap != RING_CAPACITY {
        return Err(format!(
            "existing DPS ring capacity {} != expected {}",
            cap, RING_CAPACITY
        ));
    }
    Ok(head)
}

#[cfg(test)]
mod tests;
