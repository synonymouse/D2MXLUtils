//! Windows hook installation, reattachment and removal.

use std::ffi::c_void;
use windows::core::s;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};

use super::{
    classify_prologue, read_ring_head, trampoline, DpsHook, HookState, PrologueState,
    EXPECTED_PROLOGUE, REGION_SIZE, RING_CAPACITY,
};
use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{d2client, d2common};
use crate::remote_io::{read_remote, write_remote};

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
