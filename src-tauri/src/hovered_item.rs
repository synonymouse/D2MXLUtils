//! Captures the currently hovered in-game item for targeted MXL item search.
//! The module installs a small D2Sigma tooltip hook, validates the captured
//! `UnitAny*`, and resolves it to a display name only when the snapshot is fresh.

#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::Mutex;

#[cfg(target_os = "windows")]
use windows::core::s;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::FlushInstructionCache;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, VirtualProtectEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemInformation::GetTickCount;

pub(crate) const HOVERED_ITEM_FRESH_MS: u32 = 750;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) struct HoveredItemHook {
    state: Mutex<Option<HookState>>,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[allow(dead_code)]
struct HookState {
    /// Borrowed handle (Windows) / bare pid (Linux) — same shape as
    /// `dps_hook::ProcessRef`, reused here rather than redefining an
    /// identical per-OS split.
    process: crate::dps_hook::ProcessRef,
    d2sigma_base: usize,
    region: usize,
    region_size: usize,
    fields_addr: usize,
    saved_bytes: [u8; crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PATCH_SIZE],
}

#[cfg(target_os = "windows")]
unsafe impl Send for HookState {}

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HoveredItemSnapshot {
    p_unit: u32,
    last_seen_ms: u32,
    sequence: u32,
    stack_args: [u32; TOOLTIP_STACK_ARG_COUNT],
    saved_regs: [u32; TOOLTIP_SAVED_REG_COUNT],
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrologueState {
    Original,
    ExistingHook { trampoline_addr: usize },
    Mismatch([u8; crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PATCH_SIZE]),
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_SHARED_FIELDS_OFFSET: usize = 0xC0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_STACK_ARG_COUNT: usize = 6;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_SAVED_REG_COUNT: usize = 6;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_STACK_ARG_FIELDS_OFFSET: usize = 12;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_SAVED_REG_FIELDS_OFFSET: usize =
    TOOLTIP_STACK_ARG_FIELDS_OFFSET + (TOOLTIP_STACK_ARG_COUNT - 1) * 4;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const TOOLTIP_SHARED_FIELDS_SIZE: usize =
    TOOLTIP_SAVED_REG_FIELDS_OFFSET + TOOLTIP_SAVED_REG_COUNT * 4;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const HOVERED_ITEM_SNAPSHOT_READ_ATTEMPTS: usize = 4;

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, Clone)]
struct TooltipHookBlob {
    bytes: Vec<u8>,
    fields_offset: usize,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl TooltipHookBlob {
    fn fields_addr(&self, blob_base: usize) -> usize {
        blob_base + self.fields_offset
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn build_tooltip_hook_blob(
    blob_base: usize,
    resume_addr: usize,
    get_tick_count_addr: usize,
) -> TooltipHookBlob {
    let fields_addr = blob_base + TOOLTIP_SHARED_FIELDS_OFFSET;
    let last_p_unit = fields_addr as u32;
    let last_seen_ms = (fields_addr + 4) as u32;
    let sequence = (fields_addr + 8) as u32;

    let mut bytes = Vec::with_capacity(TOOLTIP_SHARED_FIELDS_OFFSET + 16);
    bytes.extend_from_slice(&[0x9C, 0x60]);
    bytes.extend_from_slice(&[0xFF, 0x05]);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    emit_store_stack_dword(
        &mut bytes,
        crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD as u8,
        last_p_unit,
    );
    for arg_index in 1..TOOLTIP_STACK_ARG_COUNT {
        emit_store_stack_dword(
            &mut bytes,
            (crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD + arg_index * 4) as u8,
            (fields_addr + TOOLTIP_STACK_ARG_FIELDS_OFFSET + (arg_index - 1) * 4) as u32,
        );
    }
    for (reg_index, stack_offset) in [0x1C, 0x18, 0x14, 0x10, 0x04, 0x00].iter().enumerate() {
        emit_store_stack_dword(
            &mut bytes,
            *stack_offset,
            (fields_addr + TOOLTIP_SAVED_REG_FIELDS_OFFSET + reg_index * 4) as u32,
        );
    }
    bytes.push(0xB8);
    bytes.extend_from_slice(&(get_tick_count_addr as u32).to_le_bytes());
    bytes.extend_from_slice(&[0xFF, 0xD0]);
    bytes.push(0xA3);
    bytes.extend_from_slice(&last_seen_ms.to_le_bytes());
    bytes.extend_from_slice(&[0xFF, 0x05]);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&[0x61, 0x9D]);
    bytes.extend_from_slice(&crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE);
    bytes.push(0xE9);
    let rel_at = bytes.len();
    let from = blob_base + rel_at + 4;
    let rel = (resume_addr as i64 - from as i64) as i32;
    bytes.extend_from_slice(&rel.to_le_bytes());

    while bytes.len() < TOOLTIP_SHARED_FIELDS_OFFSET {
        bytes.push(0x90);
    }
    bytes.extend_from_slice(&vec![0u8; TOOLTIP_SHARED_FIELDS_SIZE]);

    TooltipHookBlob {
        bytes,
        fields_offset: TOOLTIP_SHARED_FIELDS_OFFSET,
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl HoveredItemHook {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(None),
        }
    }

    pub(crate) fn install(&self, ctx: &crate::process::D2Context) -> Result<(), String> {
        if ctx.d2_sigma == 0 {
            return Err("D2Sigma.dll not found".to_string());
        }

        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hovered-item hook mutex poisoned: {}", e))?;
        if state_lock.is_some() {
            return Err("hovered-item hook already installed".to_string());
        }

        let get_tick_count_addr = get_tick_count_addr(&ctx.process)?;
        let target = ctx.d2_sigma + crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK;
        let resume = ctx.d2_sigma + crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_RESUME;

        let mut saved = [0u8; crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PATCH_SIZE];
        ctx.process
            .read_buffer_into(target, &mut saved)
            .map_err(|e| format!("read tooltip hook prologue: {}", e))?;

        match classify_tooltip_prologue(target, saved) {
            PrologueState::Original => {}
            PrologueState::ExistingHook { trampoline_addr } => {
                let blob = build_tooltip_hook_blob(trampoline_addr, resume, get_tick_count_addr);
                let mut remote_prefix = vec![0u8; blob.fields_offset];
                ctx.process
                    .read_buffer_into(trampoline_addr, &mut remote_prefix)
                    .map_err(|e| {
                        format!(
                            "read existing hovered-item trampoline at 0x{:08X}: {}",
                            trampoline_addr, e
                        )
                    })?;
                if remote_prefix != blob.bytes[..blob.fields_offset] {
                    return Err(format!(
                        "existing hovered-item hook at 0x{:08X} does not match this build; restart Diablo II once to clear the stale hook",
                        trampoline_addr
                    ));
                }

                #[cfg(target_os = "windows")]
                let process_ref = ctx.process.handle;
                #[cfg(target_os = "linux")]
                let process_ref = ctx.process.pid;
                *state_lock = Some(HookState {
                    process: process_ref,
                    d2sigma_base: ctx.d2_sigma,
                    region: trampoline_addr,
                    region_size: REGION_SIZE,
                    fields_addr: blob.fields_addr(trampoline_addr),
                    saved_bytes: crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE,
                });
                crate::logger::info(&format!(
                    "Hovered-item hook reattached: trampoline @ 0x{:08X}, target @ 0x{:08X}",
                    trampoline_addr, target
                ));
                return Ok(());
            }
            PrologueState::Mismatch(actual) => {
                return Err(format!(
                    "tooltip hook prologue mismatch: expected {:02X?}, got {:02X?} — D2Sigma.dll may have been updated; RVA needs reverification",
                    crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE,
                    actual
                ));
            }
        }

        #[cfg(target_os = "windows")]
        let region = {
            let region_ptr = unsafe {
                VirtualAllocEx(
                    ctx.process.handle,
                    None,
                    REGION_SIZE,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if region_ptr.is_null() {
                return Err("VirtualAllocEx for hovered-item hook region failed".to_string());
            }
            region_ptr as usize
        };
        #[cfg(target_os = "windows")]
        let free_region = |region: usize| unsafe {
            let _ = VirtualFreeEx(ctx.process.handle, region as *mut c_void, 0, MEM_RELEASE);
        };

        // Linux: one `mmap_remote` call, no separate free-on-error path
        // (mirrors DPS hook/LootFilterHook — leaked on error/uninstall,
        // same tradeoff `injection.rs`'s `RemoteAlloc` already makes).
        #[cfg(target_os = "linux")]
        let region = {
            let mmap_stub_addr = ctx.d2_client
                + crate::offsets::d2client::INJECT_BASE
                + crate::offsets::d2client::inject::LINUX_MMAP_STUB;
            ctx.process.mmap_remote(mmap_stub_addr, REGION_SIZE)?
        };
        #[cfg(target_os = "linux")]
        let free_region = |_region: usize| {};

        let blob = build_tooltip_hook_blob(region, resume, get_tick_count_addr);
        if blob.bytes.len() > REGION_SIZE {
            free_region(region);
            return Err(format!(
                "hovered-item hook blob size {} > REGION_SIZE {}",
                blob.bytes.len(),
                REGION_SIZE
            ));
        }

        if let Err(e) = ctx.process.write_buffer(region, &blob.bytes) {
            free_region(region);
            return Err(format!("write hovered-item trampoline blob: {}", e));
        }
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = FlushInstructionCache(
                ctx.process.handle,
                Some(region as *const c_void),
                blob.bytes.len(),
            );
        }

        let patch = build_e9_patch(target, region);
        #[cfg(target_os = "windows")]
        let patch_result = write_remote_with_page_protection(ctx.process.handle, target, &patch);
        #[cfg(target_os = "linux")]
        let patch_result = ctx.process.write_buffer(target, &patch);
        if let Err(e) = patch_result {
            free_region(region);
            return Err(format!("write E9 patch at D2Sigma tooltip hook: {}", e));
        }
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = FlushInstructionCache(
                ctx.process.handle,
                Some(target as *const c_void),
                patch.len(),
            );
        }

        #[cfg(target_os = "windows")]
        let process_ref = ctx.process.handle;
        #[cfg(target_os = "linux")]
        let process_ref = ctx.process.pid;
        *state_lock = Some(HookState {
            process: process_ref,
            d2sigma_base: ctx.d2_sigma,
            region,
            region_size: REGION_SIZE,
            fields_addr: blob.fields_addr(region),
            saved_bytes: saved,
        });

        crate::logger::info(&format!(
            "Hovered-item hook installed: trampoline @ 0x{:08X}, fields @ 0x{:08X}, target @ 0x{:08X}",
            region,
            blob.fields_addr(region),
            target
        ));

        Ok(())
    }

    pub(crate) fn uninstall(&self) -> Result<(), String> {
        let mut state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hovered-item hook mutex poisoned: {}", e))?;
        let state = match state_lock.take() {
            Some(state) => state,
            None => return Ok(()),
        };

        let target = state.d2sigma_base + crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK;
        #[cfg(target_os = "windows")]
        let restore_result =
            write_remote_with_page_protection(state.process, target, &state.saved_bytes);
        #[cfg(target_os = "linux")]
        let restore_result =
            crate::dps_hook::write_remote(state.process, target, &state.saved_bytes);
        if let Err(e) = restore_result {
            crate::logger::error(&format!(
                "Hovered-item hook uninstall: failed to restore tooltip prologue: {} — leaking region 0x{:08X} to avoid use-after-free",
                e, state.region
            ));
            return Err(format!("restore hovered-item hook prologue: {}", e));
        }
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = FlushInstructionCache(
                state.process,
                Some(target as *const c_void),
                state.saved_bytes.len(),
            );
        }

        // Drain any thread mid-trampoline before considering the region dead.
        std::thread::sleep(std::time::Duration::from_millis(50));

        #[cfg(target_os = "windows")]
        unsafe {
            if let Err(e) =
                VirtualFreeEx(state.process, state.region as *mut c_void, 0, MEM_RELEASE)
            {
                crate::logger::error(&format!(
                    "Hovered-item hook uninstall: VirtualFreeEx failed: {}",
                    e
                ));
            }
        }
        // Linux: no remote-munmap primitive built yet — same leak-on-exit
        // tradeoff as `dps_hook`/`loot_filter_hook`.

        crate::logger::info("Hovered-item hook uninstalled");
        Ok(())
    }

    fn snapshot(&self) -> Result<Option<HoveredItemSnapshot>, String> {
        let state_lock = self
            .state
            .lock()
            .map_err(|e| format!("hovered-item hook mutex poisoned: {}", e))?;
        let state = match state_lock.as_ref() {
            Some(state) => state,
            None => return Ok(None),
        };

        let mut reads = Vec::with_capacity(HOVERED_ITEM_SNAPSHOT_READ_ATTEMPTS);
        for _ in 0..HOVERED_ITEM_SNAPSHOT_READ_ATTEMPTS {
            reads.push(read_snapshot_fields(state.process, state.fields_addr)?);
            if let Some(snapshot) = stable_snapshot_from_read_sequence(&reads) {
                return Ok(Some(snapshot));
            }
        }

        Ok(None)
    }
}

#[cfg(target_os = "windows")]
fn write_remote_with_page_protection(
    process: HANDLE,
    addr: usize,
    data: &[u8],
) -> Result<(), String> {
    if data.is_empty() {
        return Ok(());
    }

    let mut old_protect = PAGE_PROTECTION_FLAGS(0);
    unsafe {
        VirtualProtectEx(
            process,
            addr as *const c_void,
            data.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old_protect,
        )
        .map_err(|e| format!("VirtualProtectEx RWX at 0x{:08X}: {}", addr, e))?;
    }

    let write_result = crate::dps_hook::write_remote(process, addr, data);
    let mut restored_old = PAGE_PROTECTION_FLAGS(0);
    let restore_result = unsafe {
        VirtualProtectEx(
            process,
            addr as *const c_void,
            data.len(),
            old_protect,
            &mut restored_old,
        )
    };

    match (write_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(write_err), Ok(())) => Err(write_err),
        (Ok(()), Err(restore_err)) => Err(format!(
            "restore page protection at 0x{:08X}: {}",
            addr, restore_err
        )),
        (Err(write_err), Err(restore_err)) => Err(format!(
            "{}; additionally failed to restore page protection at 0x{:08X}: {}",
            write_err, addr, restore_err
        )),
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn emit_store_stack_dword(bytes: &mut Vec<u8>, stack_offset: u8, dest_addr: u32) {
    bytes.extend_from_slice(&[0x8B, 0x44, 0x24, stack_offset, 0xA3]);
    bytes.extend_from_slice(&dest_addr.to_le_bytes());
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn read_snapshot_fields(
    process: crate::dps_hook::ProcessRef,
    fields_addr: usize,
) -> Result<HoveredItemSnapshot, String> {
    let mut buf = [0u8; TOOLTIP_SHARED_FIELDS_SIZE];
    crate::dps_hook::read_remote(process, fields_addr, &mut buf)
        .map_err(|e| format!("read hovered-item hook snapshot: {}", e))?;
    let mut stack_args = [0u32; TOOLTIP_STACK_ARG_COUNT];
    stack_args[0] = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    for arg_index in 1..TOOLTIP_STACK_ARG_COUNT {
        let offset = TOOLTIP_STACK_ARG_FIELDS_OFFSET + (arg_index - 1) * 4;
        stack_args[arg_index] = u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
    }
    let mut saved_regs = [0u32; TOOLTIP_SAVED_REG_COUNT];
    for reg_index in 0..TOOLTIP_SAVED_REG_COUNT {
        let offset = TOOLTIP_SAVED_REG_FIELDS_OFFSET + reg_index * 4;
        saved_regs[reg_index] = u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap());
    }

    Ok(HoveredItemSnapshot {
        p_unit: stack_args[0],
        last_seen_ms: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        sequence: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
        stack_args,
        saved_regs,
    })
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn stable_snapshot_from_reads(
    first: HoveredItemSnapshot,
    second: HoveredItemSnapshot,
) -> Option<HoveredItemSnapshot> {
    (first == second && first.sequence % 2 == 0).then_some(first)
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn stable_snapshot_from_read_sequence(
    reads: &[HoveredItemSnapshot],
) -> Option<HoveredItemSnapshot> {
    reads
        .windows(2)
        .find_map(|pair| stable_snapshot_from_reads(pair[0], pair[1]))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn hovered_item_candidates(snapshot: HoveredItemSnapshot) -> Vec<(&'static str, u32)> {
    let mut candidates = Vec::new();
    for (label, value) in [
        ("arg0", snapshot.stack_args[0]),
        ("arg1", snapshot.stack_args[1]),
        ("arg2", snapshot.stack_args[2]),
        ("arg3", snapshot.stack_args[3]),
        ("arg4", snapshot.stack_args[4]),
        ("arg5", snapshot.stack_args[5]),
        ("eax", snapshot.saved_regs[0]),
        ("ecx", snapshot.saved_regs[1]),
        ("edx", snapshot.saved_regs[2]),
        ("ebx", snapshot.saved_regs[3]),
        ("esi", snapshot.saved_regs[4]),
        ("edi", snapshot.saved_regs[5]),
    ] {
        if value != 0 && !candidates.iter().any(|(_, existing)| *existing == value) {
            candidates.push((label, value));
        }
    }
    candidates
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl Default for HoveredItemHook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl Drop for HoveredItemHook {
    fn drop(&mut self) {
        let _ = self.uninstall();
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const REGION_SIZE: usize = 0x1000;

/// `kernel32.dll` is mapped at the same VA in every process on Windows
/// (incl. WoW64), so reading its export from our own module table gives
/// the right address for the remote process — `process` is unused there.
#[cfg(target_os = "windows")]
fn get_tick_count_addr(_process: &crate::process::ProcessHandle) -> Result<usize, String> {
    let kernel32 = unsafe { GetModuleHandleA(s!("kernel32.dll")) }
        .map_err(|e| format!("GetModuleHandleA(kernel32): {}", e))?;
    let get_tick_count = unsafe { GetProcAddress(kernel32, s!("GetTickCount")) }
        .ok_or_else(|| "GetProcAddress(GetTickCount) returned null".to_string())?;
    Ok(get_tick_count as usize)
}

/// No local `GetProcAddress` shortcut for a *remote* process — resolve
/// `GetTickCount` against Wine's own `kernel32.dll` the same way
/// `dps_hook`'s Linux `install` does.
#[cfg(target_os = "linux")]
fn get_tick_count_addr(process: &crate::process::ProcessHandle) -> Result<usize, String> {
    let kernel32_base = process.get_module_base("kernel32.dll")?;
    process.resolve_export(kernel32_base, "GetTickCount")
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn build_e9_patch(target_addr: usize, trampoline_addr: usize) -> [u8; 5] {
    let rel = (trampoline_addr as i64 - (target_addr + 5) as i64) as i32;
    let mut patch = [0u8; 5];
    patch[0] = 0xE9;
    patch[1..5].copy_from_slice(&rel.to_le_bytes());
    patch
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn classify_tooltip_prologue(target_addr: usize, prologue: [u8; 5]) -> PrologueState {
    if prologue == crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE {
        return PrologueState::Original;
    }

    if prologue[0] != 0xE9 {
        return PrologueState::Mismatch(prologue);
    }

    let rel = i32::from_le_bytes(prologue[1..5].try_into().unwrap());
    let target = target_addr as i64 + 5 + rel as i64;
    if !(1..=u32::MAX as i64).contains(&target) {
        return PrologueState::Mismatch(prologue);
    }

    PrologueState::ExistingHook {
        trampoline_addr: target as usize,
    }
}

pub(crate) fn display_name_from_raw_item_name(raw: &str) -> Option<String> {
    crate::notifier::strip_color_codes(raw)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn is_fresh(now_ms: u32, last_seen_ms: u32, max_age_ms: u32) -> bool {
    now_ms.wrapping_sub(last_seen_ms) <= max_age_ms
}

fn valid_hovered_item_unit(unit_type: u32, class_id: u32, mode: u32, p_unit_data: u32) -> bool {
    unit_type == crate::offsets::unit_type::ITEM
        && class_id < 0x1_0000
        && mode != 3
        && p_unit_data != 0
}

fn valid_hovered_item_location(
    owner_inventory: u32,
    player_inventory: u32,
    game_location: u8,
    body_location: u8,
) -> bool {
    owner_inventory != 0
        && owner_inventory == player_inventory
        && (matches!(game_location, 3 | 6 | 7) || (1..=12).contains(&body_location))
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn read_hovered_item_name(
    shared: &crate::scanner_state::SharedScannerState,
) -> Result<Option<String>, String> {
    let snapshot = match shared.hovered_item_hook.snapshot()? {
        Some(snapshot) => snapshot,
        None => return Ok(None),
    };

    // Same time base the trampoline embeds (`GetTickCount`, remote on
    // Windows / Wine's own on Linux) — see `dps_meter::now_ms`'s doc
    // comment for why this needs no ptrace round-trip on Linux.
    let now_ms = crate::dps_meter::now_ms();
    if !is_fresh(now_ms, snapshot.last_seen_ms, HOVERED_ITEM_FRESH_MS) {
        return Ok(None);
    }

    let candidates = hovered_item_candidates(snapshot);
    if candidates.is_empty() {
        return Ok(None);
    }

    let Some(player_unit) = shared
        .ctx
        .process
        .read_memory::<u32>(shared.ctx.d2_client + crate::offsets::d2client::PLAYER_UNIT)
        .ok()
        .filter(|p| *p != 0)
    else {
        return Ok(None);
    };
    let Some(player_inventory) = shared
        .ctx
        .process
        .read_memory::<u32>(player_unit as usize + crate::offsets::unit::INVENTORY)
        .ok()
        .filter(|p| *p != 0)
    else {
        return Ok(None);
    };

    for (_, candidate) in candidates {
        let p_unit = candidate as usize;
        let read_u32 = |offset: usize| shared.ctx.process.read_memory::<u32>(p_unit + offset).ok();
        let Some(unit_type) = read_u32(crate::offsets::unit::UNIT_TYPE) else {
            continue;
        };
        let Some(class_id) = read_u32(crate::offsets::unit::CLASS) else {
            continue;
        };
        let Some(mode) = read_u32(crate::offsets::unit::MODE) else {
            continue;
        };
        let Some(p_unit_data) = read_u32(crate::offsets::unit::UNIT_DATA) else {
            continue;
        };
        if !valid_hovered_item_unit(unit_type, class_id, mode, p_unit_data) {
            continue;
        }

        let item_data = p_unit_data as usize;
        let read_item_u32 = |offset: usize| {
            shared
                .ctx
                .process
                .read_memory::<u32>(item_data + offset)
                .ok()
        };
        let read_item_u8 = |offset: usize| {
            shared
                .ctx
                .process
                .read_memory::<u8>(item_data + offset)
                .ok()
        };
        let Some(owner_inventory) = read_item_u32(crate::offsets::item_data::OWNER_INVENTORY)
        else {
            continue;
        };
        let Some(game_location) = read_item_u8(crate::offsets::item_data::GAME_LOCATION) else {
            continue;
        };
        let Some(body_location) = read_item_u8(crate::offsets::item_data::BODY_LOCATION) else {
            continue;
        };
        if !valid_hovered_item_location(
            owner_inventory,
            player_inventory,
            game_location,
            body_location,
        ) {
            continue;
        }

        let injector = shared
            .injector
            .lock()
            .map_err(|e| format!("D2 injector mutex poisoned: {}", e))?;
        let raw = injector
            .get_item_name(&shared.ctx.process, candidate)
            .map_err(|e| {
                format!(
                    "get_item_name failed for pUnit=0x{:08X} pUnitData=0x{:08X}: {}",
                    candidate, p_unit_data, e
                )
            })?;
        return Ok(display_name_from_raw_item_name(&raw));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_non_empty_line() {
        let raw = "Crystal Sword\n\nAzurewrath";
        assert_eq!(
            display_name_from_raw_item_name(raw),
            Some("Azurewrath".to_string())
        );
    }

    #[test]
    fn strips_diablo_color_codes() {
        let raw = "\u{00ff}c4Sacred Set\n\u{00ff}c2Witchhunter's Ire";
        assert_eq!(
            display_name_from_raw_item_name(raw),
            Some("Witchhunter's Ire".to_string())
        );
    }

    #[test]
    fn blank_name_returns_none() {
        assert_eq!(display_name_from_raw_item_name("\n  \n"), None);
    }

    #[test]
    fn freshness_accepts_recent_ticks() {
        assert!(is_fresh(1_750, 1_000, 750));
    }

    #[test]
    fn freshness_rejects_stale_ticks() {
        assert!(!is_fresh(1_751, 1_000, 750));
    }

    #[test]
    fn freshness_handles_get_tick_count_wrap() {
        assert!(is_fresh(20, u32::MAX - 10, 40));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stable_snapshot_accepts_matching_even_sequence() {
        let snapshot = HoveredItemSnapshot {
            p_unit: 0x1234_5678,
            last_seen_ms: 42,
            sequence: 2,
            stack_args: [0x1234_5678, 0x2000_0000, 0x3000_0000, 0x4000_0000, 0, 0],
            saved_regs: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
        };

        assert_eq!(
            stable_snapshot_from_reads(snapshot, snapshot),
            Some(snapshot)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stable_snapshot_rejects_changed_or_odd_sequence() {
        let first = HoveredItemSnapshot {
            p_unit: 0x1234_5678,
            last_seen_ms: 42,
            sequence: 2,
            stack_args: [0x1234_5678, 0, 0, 0, 0, 0],
            saved_regs: [0, 0, 0, 0, 0, 0],
        };
        let changed = HoveredItemSnapshot {
            sequence: 4,
            ..first
        };
        let odd = HoveredItemSnapshot {
            sequence: 3,
            ..first
        };

        assert_eq!(stable_snapshot_from_reads(first, changed), None);
        assert_eq!(stable_snapshot_from_reads(odd, odd), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn stable_snapshot_retries_until_matching_even_sequence() {
        let first = HoveredItemSnapshot {
            p_unit: 0x1234_5678,
            last_seen_ms: 42,
            sequence: 2,
            stack_args: [0x1234_5678, 0, 0, 0, 0, 0],
            saved_regs: [0, 0, 0, 0, 0, 0],
        };
        let stable = HoveredItemSnapshot {
            p_unit: 0x8765_4321,
            last_seen_ms: 43,
            sequence: 4,
            stack_args: [0x8765_4321, 0, 0, 0, 0, 0],
            saved_regs: [0, 0, 0, 0, 0, 0],
        };

        assert_eq!(
            stable_snapshot_from_read_sequence(&[first, stable, stable]),
            Some(stable)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn protected_write_restores_remote_page_protection() {
        use windows::Win32::System::Memory::{PAGE_NOACCESS, PAGE_READWRITE};
        use windows::Win32::System::Threading::GetCurrentProcess;

        let process = unsafe { GetCurrentProcess() };
        let ptr = unsafe {
            VirtualAllocEx(
                process,
                None,
                0x1000,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        assert!(!ptr.is_null());

        let addr = ptr as usize;
        crate::dps_hook::write_remote(process, addr, &[0x11, 0x22, 0x33, 0x44, 0x55]).unwrap();
        let mut old_protect = PAGE_PROTECTION_FLAGS(0);
        unsafe {
            VirtualProtectEx(process, ptr, 0x1000, PAGE_NOACCESS, &mut old_protect).unwrap();
        }

        write_remote_with_page_protection(process, addr, &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]).unwrap();

        let mut restored_protect = PAGE_PROTECTION_FLAGS(0);
        unsafe {
            VirtualProtectEx(process, ptr, 0x1000, PAGE_READWRITE, &mut restored_protect).unwrap();
        }
        assert_eq!(restored_protect, PAGE_NOACCESS);

        let mut bytes = [0u8; 5];
        crate::dps_hook::read_remote(process, addr, &mut bytes).unwrap();
        assert_eq!(bytes, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);

        unsafe {
            let _ = VirtualFreeEx(process, ptr, 0, MEM_RELEASE);
        }
    }

    #[test]
    fn hovered_item_location_accepts_only_player_inventory_scopes() {
        let player_inventory = 0x2000;
        assert!(valid_hovered_item_location(
            player_inventory,
            player_inventory,
            3,
            0
        ));
        assert!(valid_hovered_item_location(
            player_inventory,
            player_inventory,
            6,
            0
        ));
        assert!(valid_hovered_item_location(
            player_inventory,
            player_inventory,
            7,
            0
        ));
        assert!(valid_hovered_item_location(
            player_inventory,
            player_inventory,
            0,
            4
        ));

        assert!(!valid_hovered_item_location(0x3000, player_inventory, 3, 0));
        assert!(!valid_hovered_item_location(
            player_inventory,
            player_inventory,
            0,
            0
        ));
        assert!(!valid_hovered_item_location(
            player_inventory,
            player_inventory,
            8,
            0
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_reads_item_arg_after_saved_registers() {
        let blob = build_tooltip_hook_blob(0x1000_0000, 0x2000_0000, 0x3000_0000);
        let body = &blob.bytes[..blob.fields_offset];
        let expected = [
            0x8B,
            0x44,
            0x24,
            crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD as u8,
        ];

        assert!(body.windows(expected.len()).any(|w| w == expected));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_writes_shared_fields() {
        let base = 0x1000_0000;
        let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
        let fields = blob.fields_addr(base);
        let body = &blob.bytes[..blob.fields_offset];

        assert!(body.windows(5).any(|w| {
            w[0] == 0xA3 && u32::from_le_bytes(w[1..5].try_into().unwrap()) as usize == fields
        }));
        assert!(body.windows(6).any(|w| {
            w[0] == 0xFF
                && w[1] == 0x05
                && u32::from_le_bytes(w[2..6].try_into().unwrap()) as usize == fields + 8
        }));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_writes_diagnostic_stack_args() {
        let base = 0x1000_0000;
        let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
        let fields = blob.fields_addr(base);
        let body = &blob.bytes[..blob.fields_offset];
        let expected_load_arg1 = [
            0x8B,
            0x44,
            0x24,
            (crate::offsets::d2sigma::TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD + 4) as u8,
        ];
        let expected_store_arg1 = (fields + 12).to_le_bytes();

        let load_pos = body
            .windows(expected_load_arg1.len())
            .position(|w| w == expected_load_arg1)
            .expect("expected load of second stack arg");
        let store =
            &body[load_pos + expected_load_arg1.len()..load_pos + expected_load_arg1.len() + 5];

        assert_eq!(store[0], 0xA3);
        assert_eq!(&store[1..5], &expected_store_arg1[..4]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_writes_saved_register_diagnostics() {
        let base = 0x1000_0000;
        let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
        let fields = blob.fields_addr(base);
        let body = &blob.bytes[..blob.fields_offset];
        let expected_load_ecx = [0x8B, 0x44, 0x24, 0x18];
        let expected_store_ecx = (fields + 36).to_le_bytes();

        let load_pos = body
            .windows(expected_load_ecx.len())
            .position(|w| w == expected_load_ecx)
            .expect("expected load of saved ECX");
        let store =
            &body[load_pos + expected_load_ecx.len()..load_pos + expected_load_ecx.len() + 5];

        assert_eq!(store[0], 0xA3);
        assert_eq!(&store[1..5], &expected_store_ecx[..4]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn hovered_item_candidates_try_second_stack_arg_after_first_arg() {
        let snapshot = HoveredItemSnapshot {
            p_unit: 0x0000_01BE,
            last_seen_ms: 7,
            sequence: 8,
            stack_args: [0x0000_01BE, 0x28FA_CB00, 0x28E0_A200, 0, 0, 0],
            saved_regs: [0, 0, 0, 0, 0x0000_01BE, 0],
        };
        let candidates = hovered_item_candidates(snapshot);

        assert_eq!(candidates[0], ("arg0", 0x0000_01BE));
        assert_eq!(candidates[1], ("arg1", 0x28FA_CB00));
        assert!(candidates.contains(&("arg2", 0x28E0_A200)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn hovered_item_candidates_drop_zero_and_duplicate_values() {
        let snapshot = HoveredItemSnapshot {
            p_unit: 0x28E0_0200,
            last_seen_ms: 7,
            sequence: 8,
            stack_args: [0x28E0_0200, 0, 0x28E0_0200, 0, 0, 0],
            saved_regs: [0, 0x28E0_0200, 0x28E0_A200, 0, 0, 0],
        };
        let candidates = hovered_item_candidates(snapshot);

        assert_eq!(
            candidates,
            vec![("arg0", 0x28E0_0200), ("edx", 0x28E0_A200)]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_marks_sequence_odd_before_writing_snapshot() {
        let base = 0x1000_0000;
        let blob = build_tooltip_hook_blob(base, 0x2000_0000, 0x3000_0000);
        let fields = blob.fields_addr(base);
        let body = &blob.bytes[..blob.fields_offset];
        let first_seq_inc = body
            .windows(6)
            .position(|w| {
                w[0] == 0xFF
                    && w[1] == 0x05
                    && u32::from_le_bytes(w[2..6].try_into().unwrap()) as usize == fields + 8
            })
            .expect("expected sequence increment");
        let p_unit_write = body
            .windows(5)
            .position(|w| {
                w[0] == 0xA3 && u32::from_le_bytes(w[1..5].try_into().unwrap()) as usize == fields
            })
            .expect("expected pUnit write");

        assert!(first_seq_inc < p_unit_write);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_trampoline_replays_prologue_and_jumps_to_resume() {
        let base = 0x1000_0000;
        let resume = 0x2000_0000;
        let blob = build_tooltip_hook_blob(base, resume, 0x3000_0000);
        let body = &blob.bytes[..blob.fields_offset];
        let prologue = crate::offsets::d2sigma::TOOLTIP_ITEM_HOOK_PROLOGUE;
        let pos = body
            .windows(prologue.len())
            .position(|w| w == prologue)
            .expect("expected replayed tooltip hook prologue");
        let jmp = &body[pos + prologue.len()..pos + prologue.len() + 5];
        let rel = i32::from_le_bytes(jmp[1..5].try_into().unwrap());
        let from = base + pos + prologue.len() + 5;

        assert_eq!(jmp[0], 0xE9);
        assert_eq!((from as i64 + rel as i64) as usize, resume);
    }

    #[test]
    fn hovered_item_unit_shape_accepts_valid_inventory_item() {
        assert!(valid_hovered_item_unit(
            crate::offsets::unit_type::ITEM,
            0x1234,
            4,
            0x2500_0000,
        ));
    }

    #[test]
    fn hovered_item_unit_shape_rejects_ground_or_invalid_units() {
        assert!(!valid_hovered_item_unit(1, 0x1234, 4, 0x2500_0000));
        assert!(!valid_hovered_item_unit(
            crate::offsets::unit_type::ITEM,
            0x1_0000,
            4,
            0x2500_0000,
        ));
        assert!(!valid_hovered_item_unit(
            crate::offsets::unit_type::ITEM,
            0x1234,
            3,
            0x2500_0000,
        ));
        assert!(!valid_hovered_item_unit(
            crate::offsets::unit_type::ITEM,
            0x1234,
            4,
            0,
        ));
    }
}
