//! Direct memory unit stat reader.
//!
//! Reconstructs the exact stat lookup logic of `D2Common.dll!GetUnitStat`
//! (`D2Common+0x38B70` / `0x38A80` / `0x382B0`), verified by binary analysis:
//!
//! 1. Resolves `pUnit + 0x5C` (`UNIT_TO_STATS_LIST`).
//! 2. Checks `flags` at `pStats + 0x10`:
//!    - If `flags & 0x8000_0000 != 0` (`StatListEx`, used by player & mercenaries),
//!      the modified/aggregate stats array descriptor is at `+0x48` (`pFullStat`),
//!      count is at `+0x4C` (`wFullStatCount`, signed 16-bit).
//!    - Otherwise (base `StatList`, e.g. items or unmerged stats),
//!      the base array descriptor is at `+0x24` (`pStat`),
//!      count is at `+0x28` (`wStatCount`, signed 16-bit).
//! 3. Snapshot validation: reads descriptor pointer, count, and array buffer, then rechecks
//!    that `pStats`, `flags`, array pointer, and count did not change during the read.
//!    Retries once on mismatch; returns `StatReaderError::UnstableSnapshot` if second check fails.
//! 4. Searches the 8-byte `D2StatStrc` array for target key `(stat_id << 16) | layer`
//!    using strict binary search matching `0x6fd882b0`. No linear search fallback.
//! 5. If found, evaluates conditional value adjustment via `ItemStatCost.txt` metadata:
//!    checks `+0x05` op flag against global table `[d2common + 0x890B0]`, owner unit type
//!    at `pStats + 0x44` (0=player, 1=monster/merc), and if `raw < op_base`, applies
//!    `op_base << op_param`.

#![cfg(any(target_os = "windows", target_os = "linux"))]

use std::collections::HashMap;

use crate::offsets::{d2common, data_tables, item_stat_cost, stat_list};
use crate::process::ProcessHandle;

pub const MAX_PLAUSIBLE_STAT_COUNT: usize = 2048;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StatReaderError {
    InvalidUnitPointer,
    NullStatList,
    MalformedDescriptor { count: i16, array_ptr: u32 },
    MemoryReadFailed(String),
    UnstableSnapshot,
    MetadataUnavailable(String),
}

impl std::fmt::Display for StatReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUnitPointer => write!(f, "Invalid unit pointer (null or unreadable)"),
            Self::NullStatList => write!(f, "Unit has no stat list (pStats is null)"),
            Self::MalformedDescriptor { count, array_ptr } => {
                write!(
                    f,
                    "Malformed stat descriptor: count={}, array_ptr=0x{:08X}",
                    count, array_ptr
                )
            }
            Self::MemoryReadFailed(e) => write!(f, "Memory read failed: {}", e),
            Self::UnstableSnapshot => {
                write!(f, "Stat array snapshot was unstable across retry attempts")
            }
            Self::MetadataUnavailable(e) => write!(f, "ItemStatCost metadata unavailable: {}", e),
        }
    }
}

impl std::error::Error for StatReaderError {}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StatReadResult {
    Found(i32),
    Missing, // Stat is absent from array -> engine returns 0
}

impl StatReadResult {
    #[inline]
    pub fn value_or_zero(&self) -> i32 {
        match self {
            Self::Found(v) => *v,
            Self::Missing => 0,
        }
    }

    #[inline]
    pub fn to_option(&self) -> Option<i32> {
        match self {
            Self::Found(v) => Some(*v),
            Self::Missing => None,
        }
    }
}

/// Read a single unit stat directly from memory.
///
/// Returns `Ok(StatReadResult::Found(val))` if present, `Ok(StatReadResult::Missing)`
/// if absent (engine semantic value is 0), or `Err(StatReaderError)` on validation or read failure.
pub fn read_unit_stat(
    process: &ProcessHandle,
    d2_common: usize,
    p_unit: u32,
    stat_id: u32,
    layer: u16,
) -> Result<StatReadResult, StatReaderError> {
    read_unit_stat_impl(
        |addr, size| process.read_buffer(addr, size),
        d2_common,
        p_unit,
        stat_id,
        layer,
    )
}

/// Bulk-read multiple stats for a unit in a single memory pass over the stat array.
///
/// Populates all requested stats that exist in the unit's stat array. Any stat not
/// in the array is omitted from the returned map (callers interpret absent stats as 0).
pub fn read_unit_stats_bulk(
    process: &ProcessHandle,
    d2_common: usize,
    p_unit: u32,
    stat_ids: &[u32],
    layer: u16,
) -> Result<HashMap<u32, i32>, StatReaderError> {
    read_unit_stats_bulk_impl(
        |addr, size| process.read_buffer(addr, size),
        d2_common,
        p_unit,
        stat_ids,
        layer,
    )
}

fn read_u8_helper(
    read_mem: &mut impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    addr: usize,
) -> Result<u8, String> {
    let buf = read_mem(addr, 1)?;
    Ok(buf[0])
}

fn read_u32_helper(
    read_mem: &mut impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    addr: usize,
) -> Result<u32, String> {
    let buf = read_mem(addr, 4)?;
    Ok(u32::from_le_bytes(
        buf.try_into().map_err(|_| "read 4 bytes failed")?,
    ))
}

fn read_i32_helper(
    read_mem: &mut impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    addr: usize,
) -> Result<i32, String> {
    let buf = read_mem(addr, 4)?;
    Ok(i32::from_le_bytes(
        buf.try_into().map_err(|_| "read 4 bytes failed")?,
    ))
}

fn read_i16_helper(
    read_mem: &mut impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    addr: usize,
) -> Result<i16, String> {
    let buf = read_mem(addr, 2)?;
    Ok(i16::from_le_bytes(
        buf.try_into().map_err(|_| "read 2 bytes failed")?,
    ))
}

#[derive(Debug, Clone, Copy)]
struct StatArrayDescriptor {
    p_stats: usize,
    flags: u32,
    p_array_addr: usize,
    count_addr: usize,
    array_ptr: usize,
    count_raw: i16,
}

fn resolve_descriptor(
    read_mem: &mut impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    p_unit: u32,
) -> Result<StatArrayDescriptor, StatReaderError> {
    if p_unit == 0 {
        return Err(StatReaderError::InvalidUnitPointer);
    }

    let p_stats = match read_u32_helper(read_mem, p_unit as usize + stat_list::UNIT_TO_STATS_LIST) {
        Ok(p) if p != 0 => p as usize,
        Ok(_) => return Err(StatReaderError::NullStatList),
        Err(e) => return Err(StatReaderError::MemoryReadFailed(e)),
    };

    let flags = read_u32_helper(read_mem, p_stats + stat_list::SL_FLAGS)
        .map_err(StatReaderError::MemoryReadFailed)?;

    let (p_array_addr, count_addr) = if flags & stat_list::SL_FLAG_EX != 0 {
        (
            p_stats + stat_list::SL_FULL_PSTAT,
            p_stats + stat_list::SL_FULL_STAT_COUNT,
        )
    } else {
        (
            p_stats + stat_list::SL_PSTAT,
            p_stats + stat_list::SL_STAT_COUNT,
        )
    };

    let array_ptr = read_u32_helper(read_mem, p_array_addr)
        .map_err(StatReaderError::MemoryReadFailed)? as usize;
    let count_raw =
        read_i16_helper(read_mem, count_addr).map_err(StatReaderError::MemoryReadFailed)?;

    Ok(StatArrayDescriptor {
        p_stats,
        flags,
        p_array_addr,
        count_addr,
        array_ptr,
        count_raw,
    })
}

/// Strict binary search matching D2Common+0x382B0.
///
/// In the engine, `0x6fd882b0` performs an exact binary search on `(stat_id << 16) | layer`.
/// Returns `Some(index)` on match or `None` if not found.
pub fn binary_search_stat(buffer: &[u8], count: usize, target_key: u32) -> Option<usize> {
    let mut low = 0usize;
    let mut high = count;

    while low < high {
        let mid = low + (high - low) / 2;
        let offset = mid * stat_list::STAT_RECORD_SIZE;
        if offset + 4 > buffer.len() {
            return None;
        }
        let rec_key = u32::from_le_bytes(buffer[offset..offset + 4].try_into().ok()?);

        if rec_key < target_key {
            low = mid + 1;
        } else if rec_key > target_key {
            high = mid;
        } else {
            return Some(mid);
        }
    }

    None
}

/// Disassembled from D2Common.dll 0x6fd88aae - 0x6fd88af1.
///
/// If stat has an op adjustment flag matching the global flags mask, is attached to
/// a player/merc unit via StatListEx, and raw value is less than the ItemStatCost floor
/// threshold, returns `op_base << op_param`.
pub fn check_item_stat_cost_adjustment(
    mut read_mem: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    d2_common: usize,
    p_stats: usize,
    flags: u32,
    stat_id: u32,
    raw_value: i32,
) -> Result<i32, StatReaderError> {
    if d2_common == 0 {
        return Ok(raw_value);
    }

    // 0x6fd88b92: mov ecx, dword ptr [0x6fde9e1c] (sgptDataTables)
    let sgpt = match read_u32_helper(&mut read_mem, d2_common + d2common::SGPT_DATA_TABLES) {
        Ok(p) if p != 0 => p as usize,
        _ => return Ok(raw_value),
    };

    let table_count =
        match read_u32_helper(&mut read_mem, sgpt + data_tables::ITEM_STAT_COST_TXT_COUNT) {
            Ok(c) => c,
            _ => return Ok(raw_value),
        };

    if stat_id >= table_count {
        return Ok(raw_value);
    }

    let table_base =
        match read_u32_helper(&mut read_mem, sgpt + data_tables::ITEM_STAT_COST_TXT_PTR) {
            Ok(p) if p != 0 => p as usize,
            _ => return Ok(raw_value),
        };

    let record_addr = table_base + (stat_id as usize) * item_stat_cost::RECORD_SIZE;
    let dl = match read_u8_helper(&mut read_mem, record_addr + item_stat_cost::FIELD_OP_FLAG) {
        Ok(b) => b,
        _ => return Ok(raw_value),
    };

    // If dl == 0, stat has no op adjustment
    if dl == 0 {
        return Ok(raw_value);
    }

    // 0x6fd88abc: mov ecx, dword ptr [0x6fdd90b0]
    let global_ptr =
        match read_u32_helper(&mut read_mem, d2_common + d2common::GLOBAL_STAT_FLAGS_PTR) {
            Ok(p) if p != 0 => p as usize,
            _ => return Ok(raw_value),
        };

    let global_byte = match read_u8_helper(&mut read_mem, global_ptr + 8) {
        Ok(b) => b,
        _ => return Ok(raw_value),
    };

    // 0x6fd88ac2: test byte ptr [ecx + 8], dl
    if (global_byte & dl) == 0 {
        return Ok(raw_value);
    }

    // 0x6fd88ac7: test ebp, ebp (SL_FLAG_EX)
    if (flags & stat_list::SL_FLAG_EX) == 0 {
        return Ok(raw_value);
    }

    // 0x6fd88acb: mov ecx, dword ptr [ebx + 0x44] (owner Unit*)
    let p_owner = match read_u32_helper(&mut read_mem, p_stats + 0x44) {
        Ok(p) if p != 0 => p as usize,
        _ => return Ok(raw_value),
    };

    // 0x6fd88ad2: mov edx, dword ptr [ecx] (unit type: 0=player, 1=monster/merc)
    let unit_type = match read_u32_helper(&mut read_mem, p_owner) {
        Ok(t) => t,
        _ => return Ok(raw_value),
    };

    if unit_type != 0 && unit_type != 1 {
        return Ok(raw_value);
    }

    // 0x6fd88ae1: mov edx, dword ptr [esi + 0x2c] (op_base / floor)
    let op_base = match read_i32_helper(&mut read_mem, record_addr + item_stat_cost::FIELD_OP_BASE)
    {
        Ok(val) => val,
        _ => return Ok(raw_value),
    };

    // 0x6fd88ae4: cmp eax, edx
    if raw_value < op_base {
        let op_param =
            match read_u8_helper(&mut read_mem, record_addr + item_stat_cost::FIELD_OP_PARAM) {
                Ok(p) => p,
                _ => return Ok(raw_value),
            };
        let adjusted = op_base.checked_shl(op_param as u32).unwrap_or(op_base);
        return Ok(adjusted);
    }

    Ok(raw_value)
}

/// Pure implementation of single stat lookup, parameterized on memory reader.
pub fn read_unit_stat_impl(
    mut read_mem: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    d2_common: usize,
    p_unit: u32,
    stat_id: u32,
    layer: u16,
) -> Result<StatReadResult, StatReaderError> {
    let target_key = (stat_id << 16) | (layer as u32);

    for attempt in 0..2 {
        let desc = resolve_descriptor(&mut read_mem, p_unit)?;

        if desc.count_raw <= 0 {
            return Ok(StatReadResult::Missing);
        }
        if (desc.count_raw as usize) > MAX_PLAUSIBLE_STAT_COUNT || desc.array_ptr == 0 {
            return Err(StatReaderError::MalformedDescriptor {
                count: desc.count_raw,
                array_ptr: desc.array_ptr as u32,
            });
        }
        let count = desc.count_raw as usize;
        let byte_len = count * stat_list::STAT_RECORD_SIZE;

        let buffer = match read_mem(desc.array_ptr, byte_len) {
            Ok(b) if b.len() == byte_len => b,
            Err(e) => {
                if attempt == 0 {
                    continue;
                }
                return Err(StatReaderError::MemoryReadFailed(e));
            }
            Ok(_) => {
                if attempt == 0 {
                    continue;
                }
                return Err(StatReaderError::MemoryReadFailed("short read".to_string()));
            }
        };

        // Snapshot validation: recheck pStats, flags, array_ptr, count
        let check_p_stats = read_u32_helper(
            &mut read_mem,
            p_unit as usize + stat_list::UNIT_TO_STATS_LIST,
        )
        .unwrap_or(0) as usize;
        let check_flags =
            read_u32_helper(&mut read_mem, desc.p_stats + stat_list::SL_FLAGS).unwrap_or(u32::MAX);
        let check_ptr = read_u32_helper(&mut read_mem, desc.p_array_addr).unwrap_or(0) as usize;
        let check_count = read_i16_helper(&mut read_mem, desc.count_addr).unwrap_or(i16::MIN);

        if check_p_stats != desc.p_stats
            || check_flags != desc.flags
            || check_ptr != desc.array_ptr
            || check_count != desc.count_raw
        {
            if attempt == 0 {
                continue;
            }
            return Err(StatReaderError::UnstableSnapshot);
        }

        // Strict binary search matching D2Common+0x382B0
        if let Some(idx) = binary_search_stat(&buffer, count, target_key) {
            let val_offset = idx * stat_list::STAT_RECORD_SIZE + stat_list::STAT_VALUE;
            let raw_val = i32::from_le_bytes(
                buffer[val_offset..val_offset + 4]
                    .try_into()
                    .map_err(|_| StatReaderError::MemoryReadFailed("slice failed".to_string()))?,
            );

            let adjusted_val = check_item_stat_cost_adjustment(
                &mut read_mem,
                d2_common,
                desc.p_stats,
                desc.flags,
                stat_id,
                raw_val,
            )?;
            return Ok(StatReadResult::Found(adjusted_val));
        }

        return Ok(StatReadResult::Missing);
    }

    Err(StatReaderError::UnstableSnapshot)
}

/// Pure implementation of bulk stat lookup, parameterized on memory reader.
pub fn read_unit_stats_bulk_impl(
    mut read_mem: impl FnMut(usize, usize) -> Result<Vec<u8>, String>,
    d2_common: usize,
    p_unit: u32,
    stat_ids: &[u32],
    layer: u16,
) -> Result<HashMap<u32, i32>, StatReaderError> {
    if stat_ids.is_empty() {
        return Ok(HashMap::new());
    }

    for attempt in 0..2 {
        let desc = resolve_descriptor(&mut read_mem, p_unit)?;

        if desc.count_raw <= 0 {
            return Ok(HashMap::new());
        }
        if (desc.count_raw as usize) > MAX_PLAUSIBLE_STAT_COUNT || desc.array_ptr == 0 {
            return Err(StatReaderError::MalformedDescriptor {
                count: desc.count_raw,
                array_ptr: desc.array_ptr as u32,
            });
        }
        let count = desc.count_raw as usize;
        let byte_len = count * stat_list::STAT_RECORD_SIZE;

        let buffer = match read_mem(desc.array_ptr, byte_len) {
            Ok(b) if b.len() == byte_len => b,
            Err(e) => {
                if attempt == 0 {
                    continue;
                }
                return Err(StatReaderError::MemoryReadFailed(e));
            }
            Ok(_) => {
                if attempt == 0 {
                    continue;
                }
                return Err(StatReaderError::MemoryReadFailed("short read".to_string()));
            }
        };

        // Snapshot validation
        let check_p_stats = read_u32_helper(
            &mut read_mem,
            p_unit as usize + stat_list::UNIT_TO_STATS_LIST,
        )
        .unwrap_or(0) as usize;
        let check_flags =
            read_u32_helper(&mut read_mem, desc.p_stats + stat_list::SL_FLAGS).unwrap_or(u32::MAX);
        let check_ptr = read_u32_helper(&mut read_mem, desc.p_array_addr).unwrap_or(0) as usize;
        let check_count = read_i16_helper(&mut read_mem, desc.count_addr).unwrap_or(i16::MIN);

        if check_p_stats != desc.p_stats
            || check_flags != desc.flags
            || check_ptr != desc.array_ptr
            || check_count != desc.count_raw
        {
            if attempt == 0 {
                continue;
            }
            return Err(StatReaderError::UnstableSnapshot);
        }

        let mut results = HashMap::with_capacity(stat_ids.len());
        for &id in stat_ids {
            let target_key = (id << 16) | (layer as u32);
            if let Some(idx) = binary_search_stat(&buffer, count, target_key) {
                let val_offset = idx * stat_list::STAT_RECORD_SIZE + stat_list::STAT_VALUE;
                let raw_val =
                    i32::from_le_bytes(buffer[val_offset..val_offset + 4].try_into().map_err(
                        |_| StatReaderError::MemoryReadFailed("slice failed".to_string()),
                    )?);

                let adjusted_val = check_item_stat_cost_adjustment(
                    &mut read_mem,
                    d2_common,
                    desc.p_stats,
                    desc.flags,
                    id,
                    raw_val,
                )?;
                results.insert(id, adjusted_val);
            }
        }

        return Ok(results);
    }

    Err(StatReaderError::UnstableSnapshot)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParityOutcome {
    ExactMatch { value: i32 },
    ConcurrentEngineUpdate { d1: i32, remote: i32, d2: i32 },
    ReproducibleMismatch { direct: i32, remote: i32 },
}

/// Opt-in, rate-limited parity verification between direct memory read and GetUnitStat.
///
/// - For stable stats: requires exact match (`direct == remote`). No blanket tolerance is permitted.
/// - For dynamic stats: executes sandwiched read (`D1 -> remote -> D2`). If memory was stable
///   (`D1 == D2`) but remote differs, flags as reproducible mismatch. If memory changed, verifies
///   in-flight update consistency.
pub fn verify_stat_parity(
    ctx: &crate::process::D2Context,
    injector: &crate::injection::D2Injector,
    p_unit: u32,
    stat_id: u32,
    layer: u16,
    is_stable: bool,
) -> Result<ParityOutcome, StatReaderError> {
    let d1 = read_unit_stat(&ctx.process, ctx.d2_common, p_unit, stat_id, layer)?.value_or_zero();

    let remote = injector
        .get_unit_stat(&ctx.process, p_unit, stat_id)
        .map(|v| v as i32)
        .map_err(|e| StatReaderError::MemoryReadFailed(format!("remote call failed: {}", e)))?;

    if d1 == remote {
        return Ok(ParityOutcome::ExactMatch { value: d1 });
    }

    if is_stable {
        return Ok(ParityOutcome::ReproducibleMismatch { direct: d1, remote });
    }

    // Dynamic stat sandwich check
    let d2 = read_unit_stat(&ctx.process, ctx.d2_common, p_unit, stat_id, layer)?.value_or_zero();

    if d1 == d2 {
        // Direct memory was static across the remote call, yet remote returned a different value
        return Ok(ParityOutcome::ReproducibleMismatch { direct: d1, remote });
    }

    let min_val = d1.min(d2);
    let max_val = d1.max(d2);
    if remote >= min_val && remote <= max_val {
        Ok(ParityOutcome::ConcurrentEngineUpdate { d1, remote, d2 })
    } else {
        Ok(ParityOutcome::ReproducibleMismatch { direct: d2, remote })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockMemory {
        data: HashMap<usize, Vec<u8>>,
        read_counts: HashMap<usize, usize>,
        read_sequences: HashMap<usize, Vec<Vec<u8>>>,
    }

    impl MockMemory {
        fn new() -> Self {
            Self {
                data: HashMap::new(),
                read_counts: HashMap::new(),
                read_sequences: HashMap::new(),
            }
        }

        fn write_u32(&mut self, addr: usize, val: u32) {
            self.data.insert(addr, val.to_le_bytes().to_vec());
        }

        fn write_i32(&mut self, addr: usize, val: i32) {
            self.data.insert(addr, val.to_le_bytes().to_vec());
        }

        fn write_i16(&mut self, addr: usize, val: i16) {
            self.data.insert(addr, val.to_le_bytes().to_vec());
        }

        fn write_u8(&mut self, addr: usize, val: u8) {
            self.data.insert(addr, vec![val]);
        }

        fn write_bytes(&mut self, addr: usize, bytes: &[u8]) {
            self.data.insert(addr, bytes.to_vec());
        }

        fn set_read_sequence(&mut self, addr: usize, seq: Vec<Vec<u8>>) {
            self.read_sequences.insert(addr, seq);
        }

        fn reader(&mut self) -> impl FnMut(usize, usize) -> Result<Vec<u8>, String> + '_ {
            move |addr, size| {
                let count = self.read_counts.entry(addr).or_insert(0);
                *count += 1;
                let read_idx = *count - 1;

                if let Some(seq) = self.read_sequences.get(&addr) {
                    if let Some(val) = seq.get(read_idx) {
                        if val.len() >= size {
                            return Ok(val[..size].to_vec());
                        }
                    }
                }

                if let Some(chunk) = self.data.get(&addr) {
                    if chunk.len() >= size {
                        return Ok(chunk[..size].to_vec());
                    }
                }
                Err(format!("address 0x{:08X} not found or short", addr))
            }
        }
    }

    fn encode_stat_record(layer: u16, stat_id: u16, value: i32) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&layer.to_le_bytes());
        buf[2..4].copy_from_slice(&stat_id.to_le_bytes());
        buf[4..8].copy_from_slice(&value.to_le_bytes());
        buf
    }

    #[test]
    fn strict_binary_search_finds_sorted_entries() {
        let mut records = Vec::new();
        records.extend_from_slice(&encode_stat_record(0, 5, 50));
        records.extend_from_slice(&encode_stat_record(0, 10, 100));
        records.extend_from_slice(&encode_stat_record(0, 20, 200));
        records.extend_from_slice(&encode_stat_record(0, 30, 300));

        let key10 = (10u32 << 16) | 0;
        assert_eq!(binary_search_stat(&records, 4, key10), Some(1));

        let key30 = (30u32 << 16) | 0;
        assert_eq!(binary_search_stat(&records, 4, key30), Some(3));

        let key15 = (15u32 << 16) | 0;
        assert_eq!(binary_search_stat(&records, 4, key15), None);
    }

    #[test]
    fn no_linear_search_fallback_on_unsorted_array() {
        // If data is unsorted, binary search fails matching native D2Common
        let mut records = Vec::new();
        records.extend_from_slice(&encode_stat_record(0, 30, 300));
        records.extend_from_slice(&encode_stat_record(0, 10, 100)); // out of order

        let key10 = (10u32 << 16) | 0;
        // Native binary search checks mid (idx 1 = 10, match, but if placed before mid it would fail)
        // With target key 10 and high key 30 first:
        // mid = 1 -> key is 10, matches.
        // Let's create an unsorted layout where binary search strictly misses:
        let mut records2 = Vec::new();
        records2.extend_from_slice(&encode_stat_record(0, 50, 500));
        records2.extend_from_slice(&encode_stat_record(0, 60, 600));
        records2.extend_from_slice(&encode_stat_record(0, 10, 100)); // unsorted at end

        // mid 1 is 60. 10 < 60 -> high becomes 1. mid 0 is 50. 10 < 50 -> high becomes 0. Loop ends -> None!
        assert_eq!(binary_search_stat(&records2, 3, key10), None);
    }

    #[test]
    fn stat_list_ex_reads_pfullstat_descriptor() {
        let mut mem = MockMemory::new();
        let p_unit = 0x1000;
        let p_stats = 0x2000;
        let p_full_array = 0x3000;

        mem.write_u32(p_unit + stat_list::UNIT_TO_STATS_LIST, p_stats as u32);
        mem.write_u32(p_stats + stat_list::SL_FLAGS, stat_list::SL_FLAG_EX);
        mem.write_u32(p_stats + stat_list::SL_FULL_PSTAT, p_full_array as u32);
        mem.write_i16(p_stats + stat_list::SL_FULL_STAT_COUNT, 3);

        let mut array_bytes = Vec::new();
        array_bytes.extend_from_slice(&encode_stat_record(0, 6, 500));
        array_bytes.extend_from_slice(&encode_stat_record(0, 12, 99));
        array_bytes.extend_from_slice(&encode_stat_record(0, 93, 40));
        mem.write_bytes(p_full_array, &array_bytes);

        let res_level = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 12, 0).unwrap();
        assert_eq!(res_level, StatReadResult::Found(99));

        let res_ias = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 93, 0).unwrap();
        assert_eq!(res_ias, StatReadResult::Found(40));

        let res_missing = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 105, 0).unwrap();
        assert_eq!(res_missing, StatReadResult::Missing);
    }

    #[test]
    fn malformed_descriptor_returns_error() {
        let mut mem = MockMemory::new();
        let p_unit = 0x1000;
        let p_stats = 0x2000;

        mem.write_u32(p_unit + stat_list::UNIT_TO_STATS_LIST, p_stats as u32);
        mem.write_u32(p_stats + stat_list::SL_FLAGS, 0);
        mem.write_u32(p_stats + stat_list::SL_PSTAT, 0); // null array ptr
        mem.write_i16(p_stats + stat_list::SL_STAT_COUNT, 5);

        let res = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 12, 0);
        assert!(matches!(
            res,
            Err(StatReaderError::MalformedDescriptor { .. })
        ));
    }

    #[test]
    fn unstable_snapshot_detected_and_retried_until_exhaustion() {
        let mut mem = MockMemory::new();
        let p_unit = 0x1000;
        let p_stats = 0x2000;
        let p_array = 0x3000;

        mem.write_u32(p_unit + stat_list::UNIT_TO_STATS_LIST, p_stats as u32);
        mem.write_u32(p_stats + stat_list::SL_FLAGS, 0);
        mem.write_u32(p_stats + stat_list::SL_PSTAT, p_array as u32);

        // Prepare 3 stat records in array
        let mut array_bytes = Vec::new();
        array_bytes.extend_from_slice(&encode_stat_record(0, 10, 100));
        array_bytes.extend_from_slice(&encode_stat_record(0, 12, 50));
        array_bytes.extend_from_slice(&encode_stat_record(0, 14, 200));
        mem.write_bytes(p_array, &array_bytes);

        // Sequence of SL_STAT_COUNT reads:
        // Attempt 0: resolve = 1, recheck = 2 (mismatch -> retry)
        // Attempt 1: resolve = 2, recheck = 3 (mismatch -> retry exhausted -> UnstableSnapshot)
        mem.set_read_sequence(
            p_stats + stat_list::SL_STAT_COUNT,
            vec![
                1i16.to_le_bytes().to_vec(),
                2i16.to_le_bytes().to_vec(),
                2i16.to_le_bytes().to_vec(),
                3i16.to_le_bytes().to_vec(),
            ],
        );

        let res = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 12, 0);
        assert_eq!(res, Err(StatReaderError::UnstableSnapshot));
    }

    #[test]
    fn unstable_snapshot_recovers_on_second_attempt() {
        let mut mem = MockMemory::new();
        let p_unit = 0x1000;
        let p_stats = 0x2000;
        let p_array = 0x3000;

        mem.write_u32(p_unit + stat_list::UNIT_TO_STATS_LIST, p_stats as u32);
        mem.write_u32(p_stats + stat_list::SL_FLAGS, 0);
        mem.write_u32(p_stats + stat_list::SL_PSTAT, p_array as u32);

        let mut array_bytes = Vec::new();
        array_bytes.extend_from_slice(&encode_stat_record(0, 10, 100));
        array_bytes.extend_from_slice(&encode_stat_record(0, 12, 50));
        mem.write_bytes(p_array, &array_bytes);

        // Sequence of SL_STAT_COUNT reads:
        // Attempt 0: resolve = 1, recheck = 2 (mismatch -> retry)
        // Attempt 1: resolve = 2, recheck = 2 (stable -> succeeds!)
        mem.set_read_sequence(
            p_stats + stat_list::SL_STAT_COUNT,
            vec![
                1i16.to_le_bytes().to_vec(),
                2i16.to_le_bytes().to_vec(),
                2i16.to_le_bytes().to_vec(),
                2i16.to_le_bytes().to_vec(),
            ],
        );

        let res = read_unit_stat_impl(mem.reader(), 0, p_unit as u32, 12, 0).unwrap();
        assert_eq!(res, StatReadResult::Found(50));
    }

    #[test]
    fn bulk_reads_match_individual_stats() {
        let mut mem = MockMemory::new();
        let p_unit = 0x1000;
        let p_stats = 0x2000;
        let p_array = 0x3000;

        mem.write_u32(p_unit + stat_list::UNIT_TO_STATS_LIST, p_stats as u32);
        mem.write_u32(p_stats + stat_list::SL_FLAGS, 0);
        mem.write_u32(p_stats + stat_list::SL_PSTAT, p_array as u32);
        mem.write_i16(p_stats + stat_list::SL_STAT_COUNT, 3);

        let mut array_bytes = Vec::new();
        array_bytes.extend_from_slice(&encode_stat_record(0, 12, 99)); // Level
        array_bytes.extend_from_slice(&encode_stat_record(0, 93, 40)); // IAS
        array_bytes.extend_from_slice(&encode_stat_record(0, 105, 20)); // FCR
        mem.write_bytes(p_array, &array_bytes);

        let requested = vec![12, 93, 105, 999]; // 999 is absent
        let bulk =
            read_unit_stats_bulk_impl(mem.reader(), 0, p_unit as u32, &requested, 0).unwrap();

        assert_eq!(bulk.get(&12), Some(&99));
        assert_eq!(bulk.get(&93), Some(&40));
        assert_eq!(bulk.get(&105), Some(&20));
        assert_eq!(bulk.get(&999), None); // Absent stat omitted from map
    }

    #[test]
    fn item_stat_cost_adjustment_applied_when_below_threshold() {
        let mut mem = MockMemory::new();
        let d2_common = 0x6fd50000;
        let sgpt = 0x10000;
        let table_base = 0x20000;
        let global_flags = 0x30000;
        let p_stats = 0x4000;
        let p_owner = 0x5000;

        // sgptDataTables
        mem.write_u32(d2_common + d2common::SGPT_DATA_TABLES, sgpt as u32);
        mem.write_u32(sgpt + data_tables::ITEM_STAT_COST_TXT_COUNT, 100);
        mem.write_u32(
            sgpt + data_tables::ITEM_STAT_COST_TXT_PTR,
            table_base as u32,
        );

        // global_flags pointer at d2_common + 0x890B0
        mem.write_u32(
            d2_common + d2common::GLOBAL_STAT_FLAGS_PTR,
            global_flags as u32,
        );
        mem.write_u8(global_flags + 8, 0x01); // global mask has bit 1 set

        // Stat 10 ItemStatCost record:
        let rec = table_base + 10 * item_stat_cost::RECORD_SIZE;
        mem.write_u8(rec + item_stat_cost::FIELD_OP_FLAG, 0x01); // op flag = 1
        mem.write_u8(rec + item_stat_cost::FIELD_OP_PARAM, 2); // shift = 2
        mem.write_i32(rec + item_stat_cost::FIELD_OP_BASE, 100); // threshold = 100

        // Unit owner at p_stats + 0x44
        mem.write_u32(p_stats + 0x44, p_owner as u32);
        mem.write_u32(p_owner, 0); // unit_type = 0 (player)

        // Raw value is 50 (< 100 threshold). Expected = 100 << 2 = 400.
        let adjusted = check_item_stat_cost_adjustment(
            mem.reader(),
            d2_common,
            p_stats,
            stat_list::SL_FLAG_EX,
            10,
            50,
        )
        .unwrap();

        assert_eq!(adjusted, 400);

        // Raw value is 150 (>= 100 threshold). Expected = 150 (unchanged).
        let unchanged = check_item_stat_cost_adjustment(
            mem.reader(),
            d2_common,
            p_stats,
            stat_list::SL_FLAG_EX,
            10,
            150,
        )
        .unwrap();

        assert_eq!(unchanged, 150);
    }
}
