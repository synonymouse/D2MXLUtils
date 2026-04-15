//! Loot Filter Hook Module
//! Injects code into D2Sigma.dll to control item visibility based on iEarLevel field

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, VirtualProtectEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};

#[cfg(target_os = "windows")]
use crate::logger::{error as log_error, info as log_info};
#[cfg(target_os = "windows")]
use crate::process::{D2Context, ProcessHandle};

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

/// Approximate size of D2Sigma.dll to scan (2MB should be enough)
const D2SIGMA_SCAN_SIZE: usize = 0x200000;

/// Loot filter hook manager
#[cfg(target_os = "windows")]
pub struct LootFilterHook {
    /// Address of the hook point in D2Sigma.dll
    hook_address: usize,
    /// Address of our trampoline code
    trampoline_address: usize,
    /// Address of global flag: show all loot (Alt mode)
    g_show_all_loot: usize,
    /// Address of global flag: filter enabled
    g_filter_enabled: usize,
    /// Address of call counter for debugging
    g_call_counter: usize,
    /// Address of last checked unit_id for debugging
    g_last_unit_id: usize,
    /// Address of last checked iEarLevel for debugging
    g_last_ear_level: usize,
    /// Address of hide mask (256 bytes = 2048 bits for unit_id tracking)
    g_hide_mask: usize,
    /// Saved original bytes from the hook point
    original_bytes: [u8; PATCH_SIZE],
    /// Whether the hook is currently injected
    is_injected: bool,
    /// Process handle for cleanup
    process_handle: HANDLE,
}

#[cfg(target_os = "windows")]
impl LootFilterHook {
    /// Create a new hook manager (not yet injected)
    pub fn new() -> Self {
        Self {
            hook_address: 0,
            trampoline_address: 0,
            g_show_all_loot: 0,
            g_filter_enabled: 0,
            g_call_counter: 0,
            g_last_unit_id: 0,
            g_last_ear_level: 0,
            g_hide_mask: 0,
            original_bytes: [0; PATCH_SIZE],
            is_injected: false,
            process_handle: HANDLE::default(),
        }
    }

    /// Check if hook is currently injected
    pub fn is_injected(&self) -> bool {
        self.is_injected
    }

    /// Inject the hook into D2Sigma.dll
    pub fn inject(&mut self, ctx: &D2Context) -> Result<(), String> {
        if self.is_injected {
            return Err("Hook already injected".to_string());
        }

        if ctx.d2_sigma == 0 {
            return Err("D2Sigma.dll not found".to_string());
        }

        // Find the loot filter function by signature scanning
        log_info("LootFilterHook: Scanning for function signature...");
        let found_addr = ctx
            .process
            .scan_pattern(ctx.d2_sigma, D2SIGMA_SCAN_SIZE, &FUNCTION_SIGNATURE)
            .ok_or_else(|| {
                format!(
                    "LootFilter function not found in D2Sigma.dll. Signature {:02X?} not found.",
                    FUNCTION_SIGNATURE
                )
            })?;

        self.hook_address = found_addr;
        self.process_handle = ctx.process.handle;

        let offset = found_addr - ctx.d2_sigma;
        log_info(&format!(
            "LootFilterHook: Found function at D2Sigma+{:X} (0x{:08X})",
            offset, self.hook_address
        ));

        // 1. Allocate memory for trampoline code (256 bytes)
        self.trampoline_address = self.alloc_remote(&ctx.process, 256)?;

        // 2. Allocate memory for global flags and debug variables
        self.g_show_all_loot = self.alloc_remote(&ctx.process, 1)?;
        self.g_filter_enabled = self.alloc_remote(&ctx.process, 1)?;
        self.g_call_counter = self.alloc_remote(&ctx.process, 4)?;
        self.g_last_unit_id = self.alloc_remote(&ctx.process, 4)?;
        self.g_last_ear_level = self.alloc_remote(&ctx.process, 1)?;

        // 3. Allocate 256 bytes for hide mask (2048 bits for unit_id tracking)
        self.g_hide_mask = self.alloc_remote(&ctx.process, 256)?;

        log_info(&format!(
            "LootFilterHook: Trampoline=0x{:08X}, g_show_all=0x{:08X}, g_filter_en=0x{:08X}, g_counter=0x{:08X}, g_unit_id=0x{:08X}, g_ear=0x{:08X}, g_hide_mask=0x{:08X}",
            self.trampoline_address, self.g_show_all_loot, self.g_filter_enabled, self.g_call_counter, self.g_last_unit_id, self.g_last_ear_level, self.g_hide_mask
        ));

        // 4. Initialize global flags (both TRUE by default) and debug variables
        ctx.process.write_buffer(self.g_show_all_loot, &[1u8])?;
        ctx.process.write_buffer(self.g_filter_enabled, &[1u8])?;
        ctx.process.write_buffer(self.g_call_counter, &[0u8, 0u8, 0u8, 0u8])?;
        ctx.process.write_buffer(self.g_last_unit_id, &[0u8, 0u8, 0u8, 0u8])?;
        ctx.process.write_buffer(self.g_last_ear_level, &[0u8])?;

        // 5. Initialize hide mask to all zeros (show all items by default)
        let zeros = vec![0u8; 256];
        ctx.process.write_buffer(self.g_hide_mask, &zeros)?;

        // 4. Generate and write trampoline code
        let trampoline_code = self.generate_trampoline_code();
        ctx.process
            .write_buffer(self.trampoline_address, &trampoline_code)?;

        // Verify trampoline was written correctly
        let mut verify_trampoline = vec![0u8; 32];
        if let Ok(()) = ctx.process.read_buffer_into(self.trampoline_address, &mut verify_trampoline) {
            let verify_str: String = verify_trampoline.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
            log_info(&format!("LootFilterHook: Verified trampoline at 0x{:08X}: {}", self.trampoline_address, verify_str));
        }

        // 5. Save original bytes
        let mut saved = [0u8; PATCH_SIZE];
        ctx.process
            .read_buffer_into(self.hook_address, &mut saved)?;
        self.original_bytes = saved;

        log_info(&format!(
            "LootFilterHook: Saved original bytes: {:02X?}",
            self.original_bytes
        ));

        // 6. Change memory protection to allow writing
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

        // 8. Write JMP patch
        let jmp_patch = self.generate_jmp_patch();
        let write_result = ctx.process.write_buffer(self.hook_address, &jmp_patch);

        // 9. Restore original memory protection
        unsafe {
            let _ = VirtualProtectEx(
                ctx.process.handle,
                self.hook_address as *const std::ffi::c_void,
                PATCH_SIZE,
                old_protect,
                &mut old_protect,
            );
        }

        write_result?;

        self.is_injected = true;

        // Verify the JMP was written correctly
        let mut verify_jmp = [0u8; PATCH_SIZE];
        if let Ok(()) = ctx.process.read_buffer_into(self.hook_address, &mut verify_jmp) {
            log_info(&format!(
                "LootFilterHook: Verified JMP at hook point: {:02X?}",
                verify_jmp
            ));
        }

        log_info("LootFilterHook: Hook injected successfully");

        Ok(())
    }

    /// Remove the hook and restore original bytes
    pub fn eject(&mut self, ctx: &D2Context) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        log_info("LootFilterHook: Ejecting hook...");

        // 1. Change memory protection to allow writing
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
        let write_result = ctx.process
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

        write_result?;

        // 4. Free allocated memory (optional, OS will clean up on process exit)
        // We intentionally don't free trampoline to avoid race conditions
        // (a thread might still be executing the trampoline code)

        self.is_injected = false;

        log_info("LootFilterHook: Hook ejected successfully");

        Ok(())
    }

    /// Set global show all loot flag (for Alt-mode)
    /// When false, ALL items are hidden (used when Alt is NOT pressed)
    /// When true, normal filtering applies
    pub fn set_show_all(&self, ctx: &D2Context, show: bool) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        let value = if show { 1u8 } else { 0u8 };
        ctx.process.write_buffer(self.g_show_all_loot, &[value])
    }

    /// Enable or disable the filter
    /// When disabled, original D2Sigma loot filter behavior is used
    pub fn set_filter_enabled(&self, ctx: &D2Context, enabled: bool) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        let value = if enabled { 1u8 } else { 0u8 };
        ctx.process.write_buffer(self.g_filter_enabled, &[value])
    }

    /// Get the call counter (for debugging)
    pub fn get_call_counter(&self, ctx: &D2Context) -> Result<u32, String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        ctx.process.read_memory::<u32>(self.g_call_counter)
    }

    /// Get the last checked unit_id (for debugging)
    pub fn get_last_unit_id(&self, ctx: &D2Context) -> Result<u32, String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        ctx.process.read_memory::<u32>(self.g_last_unit_id)
    }

    /// Get the last checked iEarLevel (for debugging)
    pub fn get_last_ear_level(&self, ctx: &D2Context) -> Result<u8, String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        ctx.process.read_memory::<u8>(self.g_last_ear_level)
    }

    /// Add a unit_id to the hide mask (item will be hidden)
    pub fn add_hidden_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        let bit_index = (unit_id & 0x7FF) as usize; // mod 2048
        let byte_index = bit_index >> 3; // div 8
        let bit_offset = bit_index & 7; // mod 8

        let addr = self.g_hide_mask + byte_index;
        let current = ctx.process.read_memory::<u8>(addr)?;
        let new_byte = current | (1u8 << bit_offset);
        ctx.process.write_buffer(addr, &[new_byte])
    }

    /// Remove a unit_id from the hide mask (item will be shown)
    pub fn remove_hidden_unit_id(&self, ctx: &D2Context, unit_id: u32) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        let bit_index = (unit_id & 0x7FF) as usize;
        let byte_index = bit_index >> 3;
        let bit_offset = bit_index & 7;

        let addr = self.g_hide_mask + byte_index;
        let current = ctx.process.read_memory::<u8>(addr)?;
        let new_byte = current & !(1u8 << bit_offset);
        ctx.process.write_buffer(addr, &[new_byte])
    }

    /// Clear the entire hide mask (show all items)
    pub fn clear_hidden_items(&self, ctx: &D2Context) -> Result<(), String> {
        if !self.is_injected {
            return Err("Hook not injected".to_string());
        }

        let zeros = vec![0u8; 256];
        ctx.process.write_buffer(self.g_hide_mask, &zeros)
    }

    /// Allocate memory in remote process
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
    ///   if (!g_filter_enabled)          -> original code (built-in MXL filter decides)
    ///   if (!g_show_all_loot)           -> return 0 (hide everything)
    ///   if (pUnit == NULL)              -> original code
    ///   unit_id = [pUnit + 0x0C]
    ///   if (bit set in g_hide_mask)     -> return 0 (hide)
    ///   else                            -> original code
    fn generate_trampoline_code(&self) -> Vec<u8> {
        let mut code: Vec<u8> = Vec::new();

        let addr_counter = self.g_call_counter as u32;
        let addr_filter = self.g_filter_enabled as u32;
        let addr_show_all = self.g_show_all_loot as u32;
        let addr_unit_id = self.g_last_unit_id as u32;
        let addr_hide_mask = self.g_hide_mask as u32;
        let original_continue = (self.hook_address + PATCH_SIZE) as u32;

        // inc dword ptr [g_call_counter]        ; FF 05 <addr>
        code.push(0xFF);
        code.push(0x05);
        code.extend_from_slice(&addr_counter.to_le_bytes());

        // cmp byte ptr [g_filter_enabled], 0    ; 80 3D <addr> 00
        code.push(0x80);
        code.push(0x3D);
        code.extend_from_slice(&addr_filter.to_le_bytes());
        code.push(0x00);

        // je do_original                        ; 74 <rel8>
        code.push(0x74);
        let patch_je_filter = code.len();
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

        // and eax, 0x7FF                        ; 25 FF 07 00 00
        code.push(0x25);
        code.extend_from_slice(&0x7FFu32.to_le_bytes());

        // bt dword ptr [g_hide_mask], eax       ; 0F A3 05 <addr>
        // Tests bit EAX (0..2047) in the 256-byte array at g_hide_mask.
        // Sets CF=1 if bit is set.
        code.push(0x0F);
        code.push(0xA3);
        code.push(0x05);
        code.extend_from_slice(&addr_hide_mask.to_le_bytes());

        // jc return_hide                        ; 72 <rel8>
        code.push(0x72);
        let patch_jc_hide = code.len();
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
        let jmp_target = original_continue as i32
            - (self.trampoline_address as i32 + code.len() as i32 + 4);
        code.extend_from_slice(&jmp_target.to_le_bytes());

        // return_hide:
        let return_hide_offset = code.len();

        // xor al, al                            ; 32 C0
        code.push(0x32);
        code.push(0xC0);
        // ret                                   ; C3
        code.push(0xC3);

        // Patch rel8 jumps now that label offsets are known
        let patch_rel8 = |code: &mut Vec<u8>, at: usize, target: usize| {
            let rel = target as i32 - (at as i32 + 1);
            assert!(
                (-128..=127).contains(&rel),
                "rel8 out of range: from {} to {} (={})", at, target, rel
            );
            code[at] = (rel as i8) as u8;
        };
        patch_rel8(&mut code, patch_je_filter, do_original_offset);
        patch_rel8(&mut code, patch_je_show_all, return_hide_offset);
        patch_rel8(&mut code, patch_je_null, do_original_offset);
        patch_rel8(&mut code, patch_jc_hide, return_hide_offset);

        log_info(&format!(
            "LootFilterHook: Generated {} bytes of FULL trampoline (do_original=+{}, return_hide=+{})",
            code.len(), do_original_offset, return_hide_offset
        ));

        let debug_bytes: String = code
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        log_info(&format!("LootFilterHook: Trampoline bytes: {}", debug_bytes));

        code
    }

    /// Generate JMP patch for the hook point
    fn generate_jmp_patch(&self) -> [u8; PATCH_SIZE] {
        let mut patch = [0x90u8; PATCH_SIZE]; // NOP fill

        // Normal JMP to trampoline
        patch[0] = 0xE9;
        let rel_offset =
            self.trampoline_address as i32 - (self.hook_address as i32 + 5);
        patch[1..5].copy_from_slice(&rel_offset.to_le_bytes());

        patch
    }
}

#[cfg(target_os = "windows")]
impl Default for LootFilterHook {
    fn default() -> Self {
        Self::new()
    }
}

// Stub for non-Windows (compilation only)
#[cfg(not(target_os = "windows"))]
pub struct LootFilterHook;

#[cfg(not(target_os = "windows"))]
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

    pub fn set_show_all(&self, _ctx: &crate::process::D2Context, _show: bool) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn set_filter_enabled(
        &self,
        _ctx: &crate::process::D2Context,
        _enabled: bool,
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

    pub fn remove_hidden_unit_id(
        &self,
        _ctx: &crate::process::D2Context,
        _unit_id: u32,
    ) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn clear_hidden_items(&self, _ctx: &crate::process::D2Context) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }
}

#[cfg(not(target_os = "windows"))]
impl Default for LootFilterHook {
    fn default() -> Self {
        Self::new()
    }
}
