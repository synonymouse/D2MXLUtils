//! Loot Filter Hook Module
//! Injects code into D2Sigma.dll to control item visibility based on iEarLevel field

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, VirtualProtectEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};

#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::logger::{error as log_error, info as log_info};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::process::{D2Context, ProcessHandle};
use crate::rules::Visibility;

/// Number of bytes to patch at the hook point
/// Must match exactly the size of copied instructions in generate_trampoline_code()
/// sub esp,8 (3) + push ebx (1) + push ebp (1) + mov ebx,ecx (2) + push esi (1) + push edi (1) = 9
const PATCH_SIZE: usize = 9;

/// Signature of the loot filter function (LootFilter_ShouldShowItem):
/// 83 EC 08    sub esp, 08
/// 53          push ebx
/// 55          push ebp
/// 8B D9       mov ebx, ecx
/// 56          push esi
/// 57          push edi
const FUNCTION_SIGNATURE: [u8; 9] = [0x83, 0xEC, 0x08, 0x53, 0x55, 0x8B, 0xD9, 0x56, 0x57];

// Reattach metadata — written into our 256-byte trampoline buffer on fresh
// inject, parsed by try_reattach on next launch if the JMP patch survived
// a dirty shutdown. Layout at trampoline+METADATA_OFFSET:
//   magic, version, g_call_counter, g_show_all_loot,
//   g_last_unit_id, g_show_mask, g_hide_mask, g_inspected_mask,
//   g_force_show_all  (9 × u32 LE)
const MAGIC: u32 = 0xD2FE11E7;
const METADATA_VERSION: u32 = 5;
const METADATA_OFFSET: usize = 216;
const METADATA_SIZE: usize = 36;
// Trampoline offset of the replayed original 9 bytes; verified by debug_assert
// in generate_trampoline_code.
const DO_ORIGINAL_OFFSET: usize = 68;
// Trampoline starts with `inc dword [counter]` = `FF 05 ...`.
const TRAMPOLINE_FIRST_BYTE: u8 = 0xFF;

pub(crate) const MASK_BYTES: usize = 8192;
const MASK_INDEX_BITS: u32 = 0xFFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibilityMaskOp {
    SetShow,
    SetHide,
    ClearShow,
    ClearHide,
}

const SHOW_VISIBILITY_MASK_OPS: [VisibilityMaskOp; 2] =
    [VisibilityMaskOp::SetShow, VisibilityMaskOp::ClearHide];
const HIDE_VISIBILITY_MASK_OPS: [VisibilityMaskOp; 2] =
    [VisibilityMaskOp::SetHide, VisibilityMaskOp::ClearShow];
const DEFAULT_VISIBILITY_MASK_OPS: [VisibilityMaskOp; 2] =
    [VisibilityMaskOp::ClearShow, VisibilityMaskOp::ClearHide];

pub(crate) fn visibility_mask_ops(visibility: Visibility) -> &'static [VisibilityMaskOp] {
    match visibility {
        Visibility::Show => &SHOW_VISIBILITY_MASK_OPS,
        Visibility::Hide => &HIDE_VISIBILITY_MASK_OPS,
        Visibility::Default => &DEFAULT_VISIBILITY_MASK_OPS,
    }
}

/// Loot filter hook manager
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub struct LootFilterHook {
    /// Address of the hook point in D2Sigma.dll
    hook_address: usize,
    /// Address of our trampoline code
    trampoline_address: usize,
    /// Address of global flag: show all loot (Alt mode)
    g_show_all_loot: usize,
    /// Address of call counter for debugging
    g_call_counter: usize,
    /// Address of last checked unit_id for debugging
    g_last_unit_id: usize,
    /// Address of hide mask (indexed by unit_id & MASK_INDEX_BITS)
    g_hide_mask: usize,
    /// Address of show mask (force-show overrides game filter)
    g_show_mask: usize,
    /// Unit_ids without a bit here are hidden by the trampoline until the
    /// Rust scanner analyzes them — prevents label flicker on fresh drops.
    g_inspected_mask: usize,
    /// When 1, trampoline returns AL=1 for every item (hold-to-reveal).
    g_force_show_all: usize,
    original_bytes: [u8; PATCH_SIZE],
    is_injected: bool,
    is_reattached: bool,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl LootFilterHook {
    /// Create a new hook manager (not yet injected)
    pub fn new() -> Self {
        Self {
            hook_address: 0,
            trampoline_address: 0,
            g_show_all_loot: 0,
            g_call_counter: 0,
            g_last_unit_id: 0,
            g_hide_mask: 0,
            g_show_mask: 0,
            g_inspected_mask: 0,
            g_force_show_all: 0,
            original_bytes: [0; PATCH_SIZE],
            is_injected: false,
            is_reattached: false,
        }
    }

    /// Check if hook is currently injected
    pub fn is_injected(&self) -> bool {
        self.is_injected
    }

    #[allow(dead_code)]
    pub fn is_reattached(&self) -> bool {
        self.is_reattached
    }

    pub fn inject(&mut self, ctx: &D2Context) -> Result<(), String> {
        if self.is_injected {
            return Err("Hook already injected".to_string());
        }

        if ctx.d2_sigma == 0 {
            return Err("D2Sigma.dll not found".to_string());
        }

        if let Some(addr) =
            ctx.process
                .scan_pattern(ctx.d2_sigma, ctx.d2_sigma_size, &FUNCTION_SIGNATURE)
        {
            log_info(&format!(
                "LootFilterHook: primary signature match at 0x{:08X} (fresh inject)",
                addr
            ));
            return self.fresh_inject(ctx, addr);
        }

        log_info("LootFilterHook: primary signature missing; trying wildcard scan");
        self.try_reattach(ctx)
    }

    fn fresh_inject(&mut self, ctx: &D2Context, found_addr: usize) -> Result<(), String> {
        self.hook_address = found_addr;
        let offset = found_addr - ctx.d2_sigma;

        #[cfg(target_os = "windows")]
        {
            self.trampoline_address = self.alloc_remote(&ctx.process, 256)?;

            self.g_show_all_loot = self.alloc_remote(&ctx.process, 1)?;
            self.g_force_show_all = self.alloc_remote(&ctx.process, 1)?;
            self.g_call_counter = self.alloc_remote(&ctx.process, 4)?;
            self.g_last_unit_id = self.alloc_remote(&ctx.process, 4)?;

            self.g_hide_mask = self.alloc_remote(&ctx.process, MASK_BYTES)?;
            self.g_show_mask = self.alloc_remote(&ctx.process, MASK_BYTES)?;
            self.g_inspected_mask = self.alloc_remote(&ctx.process, MASK_BYTES)?;
        }

        // One `mmap_remote` call instead of 8 separate `VirtualAllocEx`-style
        // allocations — each is a ptrace-hijacked remote call on Linux, and
        // this hook installs once per attach alongside the DPS hook and
        // D2Injector's own allocation, so keeping the count down matters for
        // startup latency. Sub-divided manually, same approach D2Injector
        // uses for its string/params scratch buffers.
        #[cfg(target_os = "linux")]
        {
            const REGION_SIZE: usize = 0x8000;
            let mmap_stub_addr = ctx.d2_client
                + crate::offsets::d2client::INJECT_BASE
                + crate::offsets::d2client::inject::LINUX_MMAP_STUB;
            let region = ctx.process.mmap_remote(mmap_stub_addr, REGION_SIZE)?;

            self.trampoline_address = region;
            self.g_show_all_loot = region + 256;
            self.g_force_show_all = region + 257;
            self.g_call_counter = region + 260;
            self.g_last_unit_id = region + 264;
            self.g_hide_mask = region + 512;
            self.g_show_mask = self.g_hide_mask + MASK_BYTES;
            self.g_inspected_mask = self.g_show_mask + MASK_BYTES;
        }

        log_info(&format!(
            "LootFilterHook: hook@D2Sigma+{:X}=0x{:08X} trampoline=0x{:08X} hide_mask=0x{:08X} show_mask=0x{:08X} inspected_mask=0x{:08X}",
            offset, self.hook_address, self.trampoline_address,
            self.g_hide_mask, self.g_show_mask, self.g_inspected_mask
        ));

        ctx.process.write_buffer(self.g_show_all_loot, &[1u8])?;
        ctx.process.write_buffer(self.g_force_show_all, &[0u8])?;
        ctx.process
            .write_buffer(self.g_call_counter, &[0u8, 0u8, 0u8, 0u8])?;
        ctx.process
            .write_buffer(self.g_last_unit_id, &[0u8, 0u8, 0u8, 0u8])?;

        let zeros = vec![0u8; MASK_BYTES];
        ctx.process.write_buffer(self.g_hide_mask, &zeros)?;
        ctx.process.write_buffer(self.g_show_mask, &zeros)?;
        ctx.process.write_buffer(self.g_inspected_mask, &zeros)?;

        let trampoline_code = self.generate_trampoline_code();
        ctx.process
            .write_buffer(self.trampoline_address, &trampoline_code)?;

        // Metadata before JMP patch: hook is never live without valid metadata.
        self.write_metadata_tail(&ctx.process)?;

        let mut saved = [0u8; PATCH_SIZE];
        ctx.process
            .read_buffer_into(self.hook_address, &mut saved)?;
        self.original_bytes = saved;

        #[cfg(target_os = "windows")]
        let write_result = {
            let mut old_protect = PAGE_PROTECTION_FLAGS(0);
            unsafe {
                VirtualProtectEx(
                    ctx.process.handle,
                    self.hook_address as *const std::ffi::c_void,
                    PATCH_SIZE,
                    PAGE_EXECUTE_READWRITE,
                    &mut old_protect,
                )
                .map_err(|e| format!("VirtualProtectEx failed: {}", e))?;
            }

            let jmp_patch = self.generate_jmp_patch();
            let write_result = ctx.process.write_buffer(self.hook_address, &jmp_patch);

            unsafe {
                let _ = VirtualProtectEx(
                    ctx.process.handle,
                    self.hook_address as *const std::ffi::c_void,
                    PATCH_SIZE,
                    old_protect,
                    &mut old_protect,
                );
            }
            write_result
        };

        // `write_buffer` already falls back to PTRACE_POKEDATA for
        // read+execute-only code pages (see `process.rs`), so no
        // VirtualProtectEx-equivalent dance is needed here.
        #[cfg(target_os = "linux")]
        let write_result = {
            let jmp_patch = self.generate_jmp_patch();
            ctx.process.write_buffer(self.hook_address, &jmp_patch)
        };

        write_result?;

        self.is_injected = true;
        self.is_reattached = false;

        log_info("LootFilterHook: injected");

        Ok(())
    }

    fn try_reattach(&mut self, ctx: &D2Context) -> Result<(), String> {
        let pattern: [Option<u8>; 9] = [
            Some(0xE9),
            None,
            None,
            None,
            None,
            Some(0x90),
            Some(0x90),
            Some(0x90),
            Some(0x90),
        ];

        let mut cursor = ctx.d2_sigma;
        let end = ctx.d2_sigma.saturating_add(ctx.d2_sigma_size);

        while cursor < end {
            let hit = match ctx.process.scan_pattern_wildcard(
                ctx.d2_sigma,
                ctx.d2_sigma_size,
                &pattern,
                cursor,
            ) {
                Some(a) => a,
                None => break,
            };
            cursor = hit + 1;

            let mut rel_bytes = [0u8; 4];
            if ctx
                .process
                .read_buffer_into(hit + 1, &mut rel_bytes)
                .is_err()
            {
                continue;
            }
            let rel = i32::from_le_bytes(rel_bytes);
            let tramp = ((hit as i64) + 5 + rel as i64) as usize;

            let mut first = [0u8; 1];
            if ctx.process.read_buffer_into(tramp, &mut first).is_err()
                || first[0] != TRAMPOLINE_FIRST_BYTE
            {
                continue;
            }

            let mut meta = [0u8; METADATA_SIZE];
            if ctx
                .process
                .read_buffer_into(tramp + METADATA_OFFSET, &mut meta)
                .is_err()
            {
                continue;
            }
            let magic = u32::from_le_bytes(meta[0..4].try_into().unwrap());
            let version = u32::from_le_bytes(meta[4..8].try_into().unwrap());

            if magic != MAGIC {
                if magic != 0 {
                    log_error(&format!(
                        "LootFilterHook: skipping wildcard hit at 0x{:08X}: foreign magic 0x{:08X}",
                        hit, magic
                    ));
                }
                continue;
            }

            if version != METADATA_VERSION {
                return Err(format!(
                    "Stale hook found but metadata version {} (expected {}) — please restart Diablo II",
                    version, METADATA_VERSION
                ));
            }

            let mut do_orig = [0u8; PATCH_SIZE];
            ctx.process
                .read_buffer_into(tramp + DO_ORIGINAL_OFFSET, &mut do_orig)
                .map_err(|e| format!(
                    "Stale hook found but couldn't read do_original block: {} — please restart Diablo II",
                    e
                ))?;
            if do_orig != FUNCTION_SIGNATURE {
                return Err(format!(
                    "Stale hook found but do_original bytes {:02X?} != expected {:02X?} — please restart Diablo II",
                    do_orig, FUNCTION_SIGNATURE
                ));
            }

            self.g_call_counter = u32::from_le_bytes(meta[8..12].try_into().unwrap()) as usize;
            self.g_show_all_loot = u32::from_le_bytes(meta[12..16].try_into().unwrap()) as usize;
            self.g_last_unit_id = u32::from_le_bytes(meta[16..20].try_into().unwrap()) as usize;
            self.g_show_mask = u32::from_le_bytes(meta[20..24].try_into().unwrap()) as usize;
            self.g_hide_mask = u32::from_le_bytes(meta[24..28].try_into().unwrap()) as usize;
            self.g_inspected_mask = u32::from_le_bytes(meta[28..32].try_into().unwrap()) as usize;
            self.g_force_show_all = u32::from_le_bytes(meta[32..36].try_into().unwrap()) as usize;

            self.hook_address = hit;
            self.trampoline_address = tramp;
            self.original_bytes = do_orig;
            self.is_injected = true;
            self.is_reattached = true;

            log_info(&format!(
                "LootFilterHook: reattached at 0x{:08X} trampoline=0x{:08X} (magic=0x{:08X} v{})",
                hit, tramp, MAGIC, METADATA_VERSION
            ));
            return Ok(());
        }

        Err(format!(
            "LootFilter function not found in D2Sigma.dll. Signature {:02X?} not found and no reusable JMP patch detected.",
            FUNCTION_SIGNATURE
        ))
    }

    fn write_metadata_tail(&self, process: &ProcessHandle) -> Result<(), String> {
        let mut buf = [0u8; METADATA_SIZE];
        buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&METADATA_VERSION.to_le_bytes());
        buf[8..12].copy_from_slice(&(self.g_call_counter as u32).to_le_bytes());
        buf[12..16].copy_from_slice(&(self.g_show_all_loot as u32).to_le_bytes());
        buf[16..20].copy_from_slice(&(self.g_last_unit_id as u32).to_le_bytes());
        buf[20..24].copy_from_slice(&(self.g_show_mask as u32).to_le_bytes());
        buf[24..28].copy_from_slice(&(self.g_hide_mask as u32).to_le_bytes());
        buf[28..32].copy_from_slice(&(self.g_inspected_mask as u32).to_le_bytes());
        buf[32..36].copy_from_slice(&(self.g_force_show_all as u32).to_le_bytes());
        process.write_buffer(self.trampoline_address + METADATA_OFFSET, &buf)
    }

    /// Remove the hook and restore original bytes
    pub fn eject(&mut self, ctx: &D2Context) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        // 1. Change memory protection to allow writing (Windows only —
        // `write_buffer` already handles this via PTRACE_POKEDATA on Linux).
        #[cfg(target_os = "windows")]
        let write_result = {
            let mut old_protect = PAGE_PROTECTION_FLAGS(0);
            unsafe {
                VirtualProtectEx(
                    ctx.process.handle,
                    self.hook_address as *const std::ffi::c_void,
                    PATCH_SIZE,
                    PAGE_EXECUTE_READWRITE,
                    &mut old_protect,
                )
                .map_err(|e| format!("VirtualProtectEx failed: {}", e))?;
            }

            // 2. Restore original bytes
            let write_result = ctx
                .process
                .write_buffer(self.hook_address, &self.original_bytes);

            // 3. Restore original memory protection
            unsafe {
                let _ = VirtualProtectEx(
                    ctx.process.handle,
                    self.hook_address as *const std::ffi::c_void,
                    PATCH_SIZE,
                    old_protect,
                    &mut old_protect,
                );
            }
            write_result
        };

        #[cfg(target_os = "linux")]
        let write_result = ctx
            .process
            .write_buffer(self.hook_address, &self.original_bytes);

        write_result?;

        // 4. Free allocated memory (optional, OS will clean up on process exit)
        // We intentionally don't free trampoline to avoid race conditions
        // (a thread might still be executing the trampoline code)

        self.is_injected = false;

        log_info("LootFilterHook: ejected");

        Ok(())
    }

    /// Set global show all loot flag (for Alt-mode)
    /// When false, ALL items are hidden (used when Alt is NOT pressed)
    /// When true, normal filtering applies
    pub fn set_show_all(&self, ctx: &D2Context, show: bool) -> Result<(), String> {
        let value = if show { 1u8 } else { 0u8 };
        ctx.process.write_buffer(self.g_show_all_loot, &[value])
    }

    pub fn set_force_show_all(&self, ctx: &D2Context, value: bool) -> Result<(), String> {
        let byte = if value { 1u8 } else { 0u8 };
        ctx.process.write_buffer(self.g_force_show_all, &[byte])
    }

    pub fn add_hidden_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        let bit_index = (unit_id & MASK_INDEX_BITS) as usize;
        let byte_index = bit_index >> 3;
        let bit_offset = bit_index & 7;

        let addr = self.g_hide_mask + byte_index;
        let current = ctx.process.read_memory::<u8>(addr)?;
        let new_byte = current | (1u8 << bit_offset);
        ctx.process.write_buffer(addr, &[new_byte])?;
        Ok(())
    }

    pub fn clear_hidden_items(&self, ctx: &D2Context) -> Result<(), String> {
        let zeros = vec![0u8; MASK_BYTES];
        ctx.process.write_buffer(self.g_hide_mask, &zeros)?;
        Ok(())
    }

    pub fn clear_hidden_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        self.clear_mask_unit_id(ctx, self.g_hide_mask, unit_id)
    }

    pub fn add_shown_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        let bit = (unit_id & MASK_INDEX_BITS) as usize;
        let byte_index = bit / 8;
        let bit_offset = bit % 8;

        let addr = self.g_show_mask + byte_index;
        let current = ctx.process.read_memory::<u8>(addr)?;
        let new_byte = current | (1u8 << bit_offset);
        ctx.process.write_buffer(addr, &[new_byte])?;
        Ok(())
    }

    pub fn clear_shown_items(&self, ctx: &D2Context) -> Result<(), String> {
        let zeros = vec![0u8; MASK_BYTES];
        ctx.process.write_buffer(self.g_show_mask, &zeros)?;
        Ok(())
    }

    pub fn clear_shown_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        self.clear_mask_unit_id(ctx, self.g_show_mask, unit_id)
    }

    /// Until this bit is set, the trampoline hides the unit — prevents label
    /// flicker on fresh drops before the scanner evaluates filter rules.
    pub fn add_inspected_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        let bit = (unit_id & MASK_INDEX_BITS) as usize;
        let byte_index = bit >> 3;
        let bit_offset = bit & 7;

        let addr = self.g_inspected_mask + byte_index;
        let current = ctx.process.read_memory::<u8>(addr)?;
        let new_byte = current | (1u8 << bit_offset);
        ctx.process.write_buffer(addr, &[new_byte])?;
        Ok(())
    }

    pub fn clear_inspected_mask(&self, ctx: &D2Context) -> Result<(), String> {
        let zeros = vec![0u8; MASK_BYTES];
        ctx.process.write_buffer(self.g_inspected_mask, &zeros)?;
        Ok(())
    }

    pub fn clear_unit_id_bits(&self, ctx: &D2Context, unit_ids: &[u32]) -> Result<(), String> {
        if unit_ids.is_empty() {
            return Ok(());
        }
        for mask_addr in [self.g_show_mask, self.g_hide_mask, self.g_inspected_mask] {
            let mut buf = vec![0u8; MASK_BYTES];
            ctx.process.read_buffer_into(mask_addr, &mut buf)?;
            let mut dirty = false;
            for &uid in unit_ids {
                let bit = (uid & MASK_INDEX_BITS) as usize;
                let byte = bit >> 3;
                let off = bit & 7;
                let mask = 1u8 << off;
                if buf[byte] & mask != 0 {
                    buf[byte] &= !mask;
                    dirty = true;
                }
            }
            if dirty {
                ctx.process.write_buffer(mask_addr, &buf)?;
            }
        }
        Ok(())
    }

    fn clear_mask_unit_id(
        &self,
        ctx: &D2Context,
        mask_addr: usize,
        unit_id: u32,
    ) -> Result<(), String> {
        let bit = (unit_id & MASK_INDEX_BITS) as usize;
        let byte_index = bit >> 3;
        let bit_offset = bit & 7;
        let bit_mask = 1u8 << bit_offset;
        let addr = mask_addr + byte_index;

        let current = ctx.process.read_memory::<u8>(addr)?;
        if current & bit_mask == 0 {
            return Ok(());
        }

        ctx.process.write_buffer(addr, &[current & !bit_mask])?;
        Ok(())
    }

    /// Allocate memory in remote process (Windows only — Linux does one
    /// `mmap_remote` call and sub-allocates manually, see `fresh_inject`).
    #[cfg(target_os = "windows")]
    fn alloc_remote(&self, process: &ProcessHandle, size: usize) -> Result<usize, String> {
        let address = unsafe {
            VirtualAllocEx(
                process.handle,
                None,
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };

        if address.is_null() {
            return Err("VirtualAllocEx failed".to_string());
        }

        Ok(address as usize)
    }

    /// Generate trampoline code (x86 assembly)
    ///
    /// On entry: ECX = pUnit (thiscall convention)
    /// Returns via AL: 0 = hide item, 1 = show item
    ///
    /// Flow:
    ///   if (g_force_show_all)           -> return 1 (hold-to-reveal hotkey)
    ///   if (!g_show_all_loot)           -> return 0 (hide everything)
    ///   if (pUnit == NULL)              -> original code
    ///   unit_id = [pUnit + 0x0C]
    ///   if (bit set in g_show_mask)     -> return 1 (force show, overrides game filter)
    ///   if (bit set in g_hide_mask)     -> return 0 (force hide)
    ///   if (bit NOT set in g_inspected_mask) -> return 0 (hide until Rust analyzes)
    ///   else                            -> original code
    fn generate_trampoline_code(&self) -> Vec<u8> {
        let mut code: Vec<u8> = Vec::new();

        let addr_counter = self.g_call_counter as u32;
        let addr_show_all = self.g_show_all_loot as u32;
        let addr_force_show = self.g_force_show_all as u32;
        let addr_unit_id = self.g_last_unit_id as u32;
        let addr_hide_mask = self.g_hide_mask as u32;
        let addr_show_mask = self.g_show_mask as u32;
        let addr_inspected_mask = self.g_inspected_mask as u32;
        let original_continue = (self.hook_address + PATCH_SIZE) as u32;

        // inc dword ptr [g_call_counter]        ; FF 05 <addr>
        code.push(0xFF);
        code.push(0x05);
        code.extend_from_slice(&addr_counter.to_le_bytes());

        // cmp byte ptr [g_force_show_all], 0    ; 80 3D <addr> 00
        code.push(0x80);
        code.push(0x3D);
        code.extend_from_slice(&addr_force_show.to_le_bytes());
        code.push(0x00);

        // jne return_show                       ; 75 <rel8>
        code.push(0x75);
        let patch_jne_force_show = code.len();
        code.push(0x00);

        // cmp byte ptr [g_show_all_loot], 0     ; 80 3D <addr> 00
        code.push(0x80);
        code.push(0x3D);
        code.extend_from_slice(&addr_show_all.to_le_bytes());
        code.push(0x00);

        // je return_hide                        ; 74 <rel8>
        code.push(0x74);
        let patch_je_show_all = code.len();
        code.push(0x00);

        // test ecx, ecx                         ; 85 C9
        code.push(0x85);
        code.push(0xC9);

        // je do_original                        ; 74 <rel8>
        code.push(0x74);
        let patch_je_null = code.len();
        code.push(0x00);

        // mov eax, [ecx+0x0C]                   ; 8B 41 0C
        code.push(0x8B);
        code.push(0x41);
        code.push(0x0C);

        // mov [g_last_unit_id], eax             ; A3 <addr>
        code.push(0xA3);
        code.extend_from_slice(&addr_unit_id.to_le_bytes());

        // and eax, MASK_INDEX_BITS              ; 25 <imm32>
        code.push(0x25);
        code.extend_from_slice(&MASK_INDEX_BITS.to_le_bytes());

        // bt dword ptr [g_show_mask], eax       ; 0F A3 05 <addr>
        code.push(0x0F);
        code.push(0xA3);
        code.push(0x05);
        code.extend_from_slice(&addr_show_mask.to_le_bytes());

        // jc return_show                        ; 72 <rel8>
        code.push(0x72);
        let patch_jc_show = code.len();
        code.push(0x00);

        // bt dword ptr [g_hide_mask], eax       ; 0F A3 05 <addr>
        code.push(0x0F);
        code.push(0xA3);
        code.push(0x05);
        code.extend_from_slice(&addr_hide_mask.to_le_bytes());

        // jc return_hide                        ; 72 <rel8>
        code.push(0x72);
        let patch_jc_hide = code.len();
        code.push(0x00);

        // bt dword ptr [g_inspected_mask], eax  ; 0F A3 05 <addr>
        code.push(0x0F);
        code.push(0xA3);
        code.push(0x05);
        code.extend_from_slice(&addr_inspected_mask.to_le_bytes());

        // jnc return_hide                       ; 73 <rel8>
        code.push(0x73);
        let patch_jnc_inspected = code.len();
        code.push(0x00);

        // do_original:
        let do_original_offset = code.len();

        // Replay the 9 bytes overwritten by the JMP patch:
        // sub esp, 8                            ; 83 EC 08
        code.push(0x83);
        code.push(0xEC);
        code.push(0x08);
        // push ebx                              ; 53
        code.push(0x53);
        // push ebp                              ; 55
        code.push(0x55);
        // mov ebx, ecx                          ; 8B D9
        code.push(0x8B);
        code.push(0xD9);
        // push esi                              ; 56
        code.push(0x56);
        // push edi                              ; 57
        code.push(0x57);

        // jmp rel32 -> original_continue (hook_address + PATCH_SIZE)
        code.push(0xE9);
        let jmp_target =
            original_continue as i32 - (self.trampoline_address as i32 + code.len() as i32 + 4);
        code.extend_from_slice(&jmp_target.to_le_bytes());

        // return_hide:
        let return_hide_offset = code.len();

        // xor al, al                            ; 32 C0
        code.push(0x32);
        code.push(0xC0);
        // ret                                   ; C3
        code.push(0xC3);

        // return_show:
        let return_show_offset = code.len();

        // mov al, 1                             ; B0 01
        code.push(0xB0);
        code.push(0x01);
        // ret                                   ; C3
        code.push(0xC3);

        // Patch rel8 jumps now that label offsets are known
        let patch_rel8 = |code: &mut Vec<u8>, at: usize, target: usize| {
            let rel = target as i32 - (at as i32 + 1);
            assert!(
                (-128..=127).contains(&rel),
                "rel8 out of range: from {} to {} (={})",
                at,
                target,
                rel
            );
            code[at] = (rel as i8) as u8;
        };
        patch_rel8(&mut code, patch_jne_force_show, return_show_offset);
        patch_rel8(&mut code, patch_je_show_all, return_hide_offset);
        patch_rel8(&mut code, patch_je_null, do_original_offset);
        patch_rel8(&mut code, patch_jc_show, return_show_offset);
        patch_rel8(&mut code, patch_jc_hide, return_hide_offset);
        patch_rel8(&mut code, patch_jnc_inspected, return_hide_offset);

        log_info(&format!(
            "LootFilterHook: Generated {} bytes of FULL trampoline (do_original=+{}, return_hide=+{}, return_show=+{})",
            code.len(), do_original_offset, return_hide_offset, return_show_offset
        ));

        let debug_bytes: String = code
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        log_info(&format!(
            "LootFilterHook: Trampoline bytes: {}",
            debug_bytes
        ));

        debug_assert!(
            code.len() <= METADATA_OFFSET,
            "trampoline code overlaps metadata tail"
        );
        debug_assert_eq!(do_original_offset, DO_ORIGINAL_OFFSET);

        code
    }

    /// Generate JMP patch for the hook point
    fn generate_jmp_patch(&self) -> [u8; PATCH_SIZE] {
        let mut patch = [0x90u8; PATCH_SIZE]; // NOP fill

        // Normal JMP to trampoline
        patch[0] = 0xE9;
        let rel_offset = self.trampoline_address as i32 - (self.hook_address as i32 + 5);
        patch[1..5].copy_from_slice(&rel_offset.to_le_bytes());

        patch
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl Default for LootFilterHook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Visibility;

    #[test]
    fn visibility_mask_ops_clear_stale_opposing_bits() {
        for (visibility, expected) in [
            (
                Visibility::Show,
                &[VisibilityMaskOp::SetShow, VisibilityMaskOp::ClearHide][..],
            ),
            (
                Visibility::Hide,
                &[VisibilityMaskOp::SetHide, VisibilityMaskOp::ClearShow][..],
            ),
            (
                Visibility::Default,
                &[VisibilityMaskOp::ClearShow, VisibilityMaskOp::ClearHide][..],
            ),
        ] {
            assert_eq!(visibility_mask_ops(visibility), expected);
        }
    }
}

// Stub for platforms without a real port (compilation only)
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct LootFilterHook;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl LootFilterHook {
    pub fn new() -> Self {
        Self
    }

    pub fn is_injected(&self) -> bool {
        false
    }

    pub fn inject(&mut self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn eject(&mut self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn set_show_all(
        &self,
        _ctx: &crate::process::D2Context,
        _show: bool,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn set_force_show_all(
        &self,
        _ctx: &crate::process::D2Context,
        _value: bool,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn add_hidden_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_hidden_items(&self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_hidden_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_unit_id_bits(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_ids: &[u32],
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn add_shown_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_shown_items(&self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_shown_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn add_inspected_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_inspected_mask(&self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl Default for LootFilterHook {
    fn default() -> Self {
        Self::new()
    }
}
