//! Inline hook on `D2Common::STATLIST_SetUnitStat` (Ord10887). Captures
//! every monster HP write in both SP and MP.
//!
//! See `docs/dps-meter-reverse-engineering.md` for the call chain
//! and `docs/superpowers/specs/2026-05-07-dps-meter-design.md` for the
//! design rationale.

#![cfg(any(target_os = "windows", target_os = "linux"))]

mod ring;
mod trampoline;

use std::sync::Mutex;

#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows::core::s;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, ReadProcessMemory, WriteProcessMemory,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};

#[cfg(target_os = "linux")]
use crate::process::ProcessHandle;

use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{d2client, d2common};

/// Opaque per-OS process reference threaded through `HookState`/
/// `RingReader`/`read_remote`/`write_remote` — a Win32 `HANDLE` on
/// Windows, a bare pid on Linux (matching `process::ProcessHandle`'s own
/// shape there). Letting `RingReader`/the shared prologue-classification
/// logic stay OS-agnostic beyond this one type.
#[cfg(target_os = "windows")]
pub(crate) type ProcessRef = HANDLE;
#[cfg(target_os = "linux")]
pub(crate) type ProcessRef = u32;

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

#[cfg(target_os = "windows")]
impl DpsHook {
    /// Install the inline hook at `D2Common.dll + STATLIST_SET_UNIT_STAT`.
    /// Allocates a region in the D2 process for trampoline + ring buffer,
    /// then atomically patches `Ord10887`'s first 5 bytes with
    /// `E9 rel32 → trampoline`.
    pub fn install(
        &self,
        process: HANDLE,
        d2common_base: usize,
        d2client_base: usize,
    ) -> Result<(), String> {
        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hook state mutex poisoned: {}", e))?;
        if state_lock.is_some() {
            return Err("DPS hook already installed".into());
        }

        // KERNEL32 is mapped at the same VA in every process on Windows
        // (incl. WoW64), so reading its export from our own module table
        // gives the right address for the remote process.
        let kernel32 = unsafe { GetModuleHandleA(s!("kernel32.dll")) }
            .map_err(|e| format!("GetModuleHandleA(kernel32): {}", e))?;
        let get_tick_count = unsafe { GetProcAddress(kernel32, s!("GetTickCount")) }
            .ok_or_else(|| "GetProcAddress(GetTickCount) returned null".to_string())?;
        let get_tick_count_addr = get_tick_count as usize;

        let ord10887_addr = d2common_base + d2common::STATLIST_SET_UNIT_STAT;
        let ord10887_resume = ord10887_addr + 5;
        let difficulty_addr = d2client_base + d2client::DIFFICULTY;

        // Read before allocating anything: if the previous app instance died
        // after patching D2, the target will already be an E9 into our old
        // trampoline and we should reattach rather than report an RVA mismatch.
        let mut saved = [0u8; 5];
        if let Err(e) = read_remote(process, ord10887_addr, &mut saved) {
            return Err(format!("read current Ord10887 prologue: {}", e));
        }

        match classify_prologue(ord10887_addr, saved) {
            PrologueState::Original => {}
            PrologueState::ExistingHook { trampoline_addr } => {
                let blob = trampoline::build(&trampoline::BuildParams {
                    ord10887_resume,
                    blob_base: trampoline_addr,
                    ring_capacity: RING_CAPACITY,
                    difficulty_addr,
                    get_tick_count_addr,
                });
                let mut remote_prefix = vec![0u8; blob.ring_offset];
                read_remote(process, trampoline_addr, &mut remote_prefix).map_err(|e| {
                    format!(
                        "read existing DPS trampoline at 0x{:08X}: {}",
                        trampoline_addr, e
                    )
                })?;
                if remote_prefix.as_slice() != &blob.bytes[..blob.ring_offset] {
                    return Err(format!(
                        "existing DPS hook at 0x{:08X} does not match this build; \
                         restart Diablo II once to clear the stale hook",
                        trampoline_addr
                    ));
                }

                let ring_addr = trampoline_addr + blob.ring_offset;
                let read_tail = read_ring_head(process, ring_addr)?;
                *state_lock = Some(HookState {
                    process,
                    d2common_base,
                    region: trampoline_addr,
                    region_size: REGION_SIZE,
                    ring_addr,
                    ring_capacity: RING_CAPACITY,
                    saved_bytes: EXPECTED_PROLOGUE,
                    read_tail,
                });

                log_info(&format!(
                    "DPS hook reattached: trampoline @ 0x{:08X}, ring @ 0x{:08X}, target @ 0x{:08X}",
                    trampoline_addr, ring_addr, ord10887_addr
                ));
                return Ok(());
            }
            PrologueState::Mismatch(actual) => {
                return Err(format!(
                    "Ord10887 prologue mismatch: expected {:02X?}, got {:02X?} \
                     — D2Common.dll may have been updated; RVA needs reverification",
                    EXPECTED_PROLOGUE, actual
                ));
            }
        }

        let region_ptr = unsafe {
            VirtualAllocEx(
                process,
                None,
                REGION_SIZE,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        if region_ptr.is_null() {
            return Err("VirtualAllocEx for DPS hook region failed".into());
        }
        let region = region_ptr as usize;

        let blob = trampoline::build(&trampoline::BuildParams {
            ord10887_resume,
            blob_base: region,
            ring_capacity: RING_CAPACITY,
            difficulty_addr,
            get_tick_count_addr,
        });
        if blob.bytes.len() > REGION_SIZE {
            let _ = unsafe { VirtualFreeEx(process, region_ptr, 0, MEM_RELEASE) };
            return Err(format!(
                "trampoline blob size {} > REGION_SIZE {}",
                blob.bytes.len(),
                REGION_SIZE
            ));
        }

        if let Err(e) = write_remote(process, region, &blob.bytes) {
            let _ = unsafe { VirtualFreeEx(process, region_ptr, 0, MEM_RELEASE) };
            return Err(format!("write trampoline blob: {}", e));
        }
        unsafe {
            let _ = FlushInstructionCache(process, Some(region as *const c_void), blob.bytes.len());
        }

        // 5 bytes is well under a cache line, so the patch write is
        // single-instruction-safe even with concurrent execution.
        let trampoline_abs = region + blob.trampoline_offset;
        let patch_eip_after = ord10887_addr + 5;
        let rel: i32 = (trampoline_abs as i64 - patch_eip_after as i64) as i32;
        let mut patch = [0u8; 5];
        patch[0] = 0xE9;
        patch[1..5].copy_from_slice(&rel.to_le_bytes());

        if let Err(e) = write_remote(process, ord10887_addr, &patch) {
            let _ = unsafe { VirtualFreeEx(process, region_ptr, 0, MEM_RELEASE) };
            return Err(format!("write E9 patch at Ord10887: {}", e));
        }
        unsafe {
            let _ = FlushInstructionCache(process, Some(ord10887_addr as *const c_void), 5);
        }

        log_info(&format!(
            "DPS hook installed: trampoline @ 0x{:08X}, ring @ 0x{:08X}, target @ 0x{:08X}",
            trampoline_abs,
            region + blob.ring_offset,
            ord10887_addr
        ));

        *state_lock = Some(HookState {
            process,
            d2common_base,
            region,
            region_size: REGION_SIZE,
            ring_addr: region + blob.ring_offset,
            ring_capacity: RING_CAPACITY,
            saved_bytes: saved,
            read_tail: 0,
        });

        Ok(())
    }

    /// Restore Ord10887's original prologue and free the trampoline region.
    /// Idempotent: returns Ok if not currently installed.
    pub fn uninstall(&self) -> Result<(), String> {
        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hook state mutex poisoned: {}", e))?;
        let state = match state_lock.take() {
            Some(s) => s,
            None => return Ok(()),
        };

        let ord10887_addr = state.d2common_base + d2common::STATLIST_SET_UNIT_STAT;

        if let Err(e) = write_remote(state.process, ord10887_addr, &state.saved_bytes) {
            log_error(&format!(
                "DPS hook uninstall: failed to restore Ord10887 prologue: {} \
                 — leaking region 0x{:08X} to avoid use-after-free",
                e, state.region
            ));
            // Don't free the region — the trampoline may still be reachable.
            return Err(format!("restore Ord10887: {}", e));
        }
        unsafe {
            let _ = FlushInstructionCache(state.process, Some(ord10887_addr as *const c_void), 5);
        }

        // Drain any thread mid-trampoline before freeing the region.
        // 50 ms >> trampoline runtime (~200 cycles).
        std::thread::sleep(std::time::Duration::from_millis(50));

        let region_ptr = state.region as *mut c_void;
        unsafe {
            if let Err(e) = VirtualFreeEx(state.process, region_ptr, 0, MEM_RELEASE) {
                log_error(&format!("DPS hook uninstall: VirtualFreeEx failed: {}", e));
            }
        }

        log_info("DPS hook uninstalled");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl DpsHook {
    /// Install the inline hook at `D2Common.dll + STATLIST_SET_UNIT_STAT`.
    /// Same trampoline bytecode and prologue-patch approach as Windows
    /// (see `trampoline::build`/`classify_prologue`) — only the region
    /// allocation (`ProcessHandle::mmap_remote`, since there's no
    /// `VirtualAllocEx` equivalent reachable without code already running
    /// in the target) and `GetTickCount` resolution (`resolve_export`
    /// against Wine's own `kernel32.dll`, since there's no local
    /// `GetProcAddress` shortcut for a *remote* process's module) differ.
    /// No `FlushInstructionCache` equivalent is needed: x86 keeps the
    /// icache coherent with data writes in hardware.
    pub fn install(
        &self,
        process: u32,
        d2common_base: usize,
        d2client_base: usize,
    ) -> Result<(), String> {
        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hook state mutex poisoned: {}", e))?;
        if state_lock.is_some() {
            return Err("DPS hook already installed".into());
        }

        let handle = ProcessHandle { pid: process };
        let kernel32_base = handle.get_module_base("kernel32.dll")?;
        let get_tick_count_addr = handle.resolve_export(kernel32_base, "GetTickCount")?;

        let ord10887_addr = d2common_base + d2common::STATLIST_SET_UNIT_STAT;
        let ord10887_resume = ord10887_addr + 5;
        let difficulty_addr = d2client_base + d2client::DIFFICULTY;

        let mut saved = [0u8; 5];
        read_remote(process, ord10887_addr, &mut saved)
            .map_err(|e| format!("read current Ord10887 prologue: {}", e))?;

        match classify_prologue(ord10887_addr, saved) {
            PrologueState::Original => {}
            PrologueState::ExistingHook { trampoline_addr } => {
                let blob = trampoline::build(&trampoline::BuildParams {
                    ord10887_resume,
                    blob_base: trampoline_addr,
                    ring_capacity: RING_CAPACITY,
                    difficulty_addr,
                    get_tick_count_addr,
                });
                let mut remote_prefix = vec![0u8; blob.ring_offset];
                read_remote(process, trampoline_addr, &mut remote_prefix).map_err(|e| {
                    format!(
                        "read existing DPS trampoline at 0x{:08X}: {}",
                        trampoline_addr, e
                    )
                })?;
                if remote_prefix.as_slice() != &blob.bytes[..blob.ring_offset] {
                    return Err(format!(
                        "existing DPS hook at 0x{:08X} does not match this build; \
                         restart Diablo II once to clear the stale hook",
                        trampoline_addr
                    ));
                }

                let ring_addr = trampoline_addr + blob.ring_offset;
                let read_tail = read_ring_head(process, ring_addr)?;
                *state_lock = Some(HookState {
                    process,
                    d2common_base,
                    region: trampoline_addr,
                    region_size: REGION_SIZE,
                    ring_addr,
                    ring_capacity: RING_CAPACITY,
                    saved_bytes: EXPECTED_PROLOGUE,
                    read_tail,
                });

                log_info(&format!(
                    "DPS hook reattached: trampoline @ 0x{:08X}, ring @ 0x{:08X}, target @ 0x{:08X}",
                    trampoline_addr, ring_addr, ord10887_addr
                ));
                return Ok(());
            }
            PrologueState::Mismatch(actual) => {
                return Err(format!(
                    "Ord10887 prologue mismatch: expected {:02X?}, got {:02X?} \
                     — D2Common.dll may have been updated; RVA needs reverification",
                    EXPECTED_PROLOGUE, actual
                ));
            }
        }

        let mmap_stub_addr =
            d2client_base + d2client::INJECT_BASE + d2client::inject::LINUX_MMAP_STUB;
        let region = handle.mmap_remote(mmap_stub_addr, REGION_SIZE)?;

        let blob = trampoline::build(&trampoline::BuildParams {
            ord10887_resume,
            blob_base: region,
            ring_capacity: RING_CAPACITY,
            difficulty_addr,
            get_tick_count_addr,
        });
        if blob.bytes.len() > REGION_SIZE {
            return Err(format!(
                "trampoline blob size {} > REGION_SIZE {}",
                blob.bytes.len(),
                REGION_SIZE
            ));
        }

        write_remote(process, region, &blob.bytes)
            .map_err(|e| format!("write trampoline blob: {}", e))?;

        let trampoline_abs = region + blob.trampoline_offset;
        let patch_eip_after = ord10887_addr + 5;
        let rel: i32 = (trampoline_abs as i64 - patch_eip_after as i64) as i32;
        let mut patch = [0u8; 5];
        patch[0] = 0xE9;
        patch[1..5].copy_from_slice(&rel.to_le_bytes());

        write_remote(process, ord10887_addr, &patch)
            .map_err(|e| format!("write E9 patch at Ord10887: {}", e))?;

        log_info(&format!(
            "DPS hook installed: trampoline @ 0x{:08X}, ring @ 0x{:08X}, target @ 0x{:08X}",
            trampoline_abs,
            region + blob.ring_offset,
            ord10887_addr
        ));

        *state_lock = Some(HookState {
            process,
            d2common_base,
            region,
            region_size: REGION_SIZE,
            ring_addr: region + blob.ring_offset,
            ring_capacity: RING_CAPACITY,
            saved_bytes: saved,
            read_tail: 0,
        });

        Ok(())
    }

    /// Restore Ord10887's original prologue. Idempotent: returns Ok if not
    /// currently installed. Unlike Windows, the allocated region is never
    /// freed (no remote-`munmap` primitive built yet) — same leak-on-exit
    /// tradeoff `injection.rs`'s `RemoteAlloc` already makes deliberately.
    pub fn uninstall(&self) -> Result<(), String> {
        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hook state mutex poisoned: {}", e))?;
        let state = match state_lock.take() {
            Some(s) => s,
            None => return Ok(()),
        };

        let ord10887_addr = state.d2common_base + d2common::STATLIST_SET_UNIT_STAT;

        if let Err(e) = write_remote(state.process, ord10887_addr, &state.saved_bytes) {
            log_error(&format!(
                "DPS hook uninstall: failed to restore Ord10887 prologue: {} \
                 — leaking region 0x{:08X}",
                e, state.region
            ));
            return Err(format!("restore Ord10887: {}", e));
        }

        // Drain any thread mid-trampoline before considering the region dead.
        // 50 ms >> trampoline runtime (~200 cycles).
        std::thread::sleep(std::time::Duration::from_millis(50));

        log_info("DPS hook uninstalled");
        Ok(())
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

#[cfg(target_os = "windows")]
pub(crate) fn write_remote(handle: HANDLE, addr: usize, data: &[u8]) -> Result<(), String> {
    let mut written = 0usize;
    unsafe {
        WriteProcessMemory(
            handle,
            addr as *const c_void,
            data.as_ptr() as *const c_void,
            data.len(),
            Some(&mut written),
        )
        .map_err(|e| format!("WriteProcessMemory failed: {}", e))?;
    }
    if written != data.len() {
        return Err(format!(
            "WriteProcessMemory: wrote {} of {} bytes",
            written,
            data.len()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn read_remote(handle: HANDLE, addr: usize, buf: &mut [u8]) -> Result<(), String> {
    let mut read = 0usize;
    unsafe {
        ReadProcessMemory(
            handle,
            addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            buf.len(),
            Some(&mut read),
        )
        .map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
    }
    if read != buf.len() {
        return Err(format!(
            "ReadProcessMemory: read {} of {} bytes",
            read,
            buf.len()
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn write_remote(pid: u32, addr: usize, data: &[u8]) -> Result<(), String> {
    ProcessHandle { pid }.write_buffer(addr, data)
}

#[cfg(target_os = "linux")]
pub(crate) fn read_remote(pid: u32, addr: usize, buf: &mut [u8]) -> Result<(), String> {
    ProcessHandle { pid }.read_buffer_into(addr, buf)
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
mod tests {
    use super::*;

    #[test]
    fn e9_prologue_is_existing_hook_target_not_mismatch() {
        let ord10887_addr = 0x6FD8_A740;
        let trampoline_addr = 0x275D_0000;
        let rel = (trampoline_addr as i64 - (ord10887_addr + 5) as i64) as i32;
        let mut prologue = [0u8; 5];
        prologue[0] = 0xE9;
        prologue[1..5].copy_from_slice(&rel.to_le_bytes());

        assert_eq!(
            classify_prologue(ord10887_addr, prologue),
            PrologueState::ExistingHook { trampoline_addr }
        );
    }
}
