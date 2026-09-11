//! Linux/Wine hook installation, reattachment and removal.

use super::{
    classify_prologue, read_ring_head, trampoline, DpsHook, HookState, PrologueState,
    EXPECTED_PROLOGUE, REGION_SIZE, RING_CAPACITY,
};
use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{d2client, d2common};
use crate::process::ProcessHandle;
use crate::remote_io::{read_remote, write_remote};

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
    /// tradeoff `injection/mod.rs`'s `RemoteAlloc` already makes deliberately.
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
