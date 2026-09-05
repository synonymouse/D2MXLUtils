//! Ring-buffer reader. The buffer lives in D2's address space; we drain
//! it via `ReadProcessMemory` once per scanner tick.
//!
//! Header (16 B): u32 head, u32 tail (unused — we keep our own cursor),
//! u32 capacity, u32 _pad. Slot (16 B): u32 ts_ms, u32 unit_id,
//! u32 delta_raw (bit 31 = is_kill, bits 0-30 = old - new),
//! u16 max_hp, u16 monster_level.

#![cfg(any(target_os = "windows", target_os = "linux"))]

use crate::dps_hook::{read_remote, HookEvent, ProcessRef};

pub const HEADER_SIZE: usize = 0x10;
pub const SLOT_SIZE: usize = 0x10;

#[allow(dead_code)]
pub struct RingReader {
    pub process: ProcessRef,
    pub ring_addr: usize,
    pub capacity: u32,
    pub tail: u32,
}

impl RingReader {
    /// Drain events written since the previous `drain()` call.
    ///
    /// `head` is u32-monotonic on the writer side; we track our own
    /// `tail` symmetrically and use wrapping arithmetic. If the writer
    /// raced ahead by more than `capacity`, older slots were overwritten
    /// — we skip ahead to surface the most recent ring's worth.
    pub fn drain(&mut self) -> Vec<HookEvent> {
        let mut events = Vec::new();

        let mut header = [0u8; HEADER_SIZE];
        if read_remote(self.process, self.ring_addr, &mut header).is_err() {
            return events;
        }
        let head = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let cap = u32::from_le_bytes(header[8..12].try_into().unwrap());

        // Mismatch with our installed capacity = the region was clobbered.
        if cap == 0 || (cap & (cap - 1)) != 0 || cap > 0x10000 || cap != self.capacity {
            return events;
        }

        let pending = head.wrapping_sub(self.tail);
        if pending == 0 {
            return events;
        }
        let to_read = pending.min(cap);
        if pending > cap {
            self.tail = head.wrapping_sub(cap);
        }

        let mask = cap - 1;
        for _ in 0..to_read {
            let slot_idx = self.tail & mask;
            let slot_addr = self.ring_addr + HEADER_SIZE + (slot_idx as usize) * SLOT_SIZE;

            let mut slot = [0u8; SLOT_SIZE];
            if read_remote(self.process, slot_addr, &mut slot).is_err() {
                break;
            }
            let ts_ms = u32::from_le_bytes(slot[0..4].try_into().unwrap());
            let unit_id = u32::from_le_bytes(slot[4..8].try_into().unwrap());
            let delta_raw = u32::from_le_bytes(slot[8..12].try_into().unwrap());
            let max_hp = u16::from_le_bytes(slot[12..14].try_into().unwrap());
            let monster_level = u16::from_le_bytes(slot[14..16].try_into().unwrap());

            events.push(HookEvent {
                ts_ms,
                unit_id,
                delta_raw,
                max_hp,
                monster_level,
            });
            self.tail = self.tail.wrapping_add(1);
        }

        events
    }
}
