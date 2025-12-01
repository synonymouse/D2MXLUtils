//! D2 code injection module
//! Provides functionality for injecting code and calling game functions via remote threads

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, WaitForSingleObject, INFINITE,
};
#[cfg(target_os = "windows")]
use std::ffi::c_void;

use crate::offsets::{d2client, d2common};
use crate::process::ProcessHandle;

/// Allocated memory region in the target process
#[cfg(target_os = "windows")]
pub struct RemoteAlloc {
    handle: HANDLE,
    pub address: usize,
    size: usize,
}

#[cfg(target_os = "windows")]
impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        if self.address != 0 {
            unsafe {
                let _ = VirtualFreeEx(self.handle, self.address as *mut c_void, 0, MEM_RELEASE);
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl RemoteAlloc {
    /// Allocate memory in the remote process
    pub fn new(process: &ProcessHandle, size: usize) -> Result<Self, String> {
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

        Ok(Self {
            handle: process.handle,
            address: address as usize,
            size,
        })
    }
}

/// Execute a function in the remote process via CreateRemoteThread
/// Returns the thread exit code (which is often the return value of the function)
#[cfg(target_os = "windows")]
pub fn remote_thread(process: &ProcessHandle, func_addr: usize, param: usize) -> Result<u32, String> {
    unsafe {
        let thread = CreateRemoteThread(
            process.handle,
            None,
            0,
            Some(std::mem::transmute(func_addr)),
            Some(param as *const c_void),
            0,
            None,
        )
        .map_err(|e| format!("CreateRemoteThread failed: {}", e))?;

        WaitForSingleObject(thread, INFINITE);

        let mut exit_code: u32 = 0;
        GetExitCodeThread(thread, &mut exit_code)
            .map_err(|e| format!("GetExitCodeThread failed: {}", e))?;

        Ok(exit_code)
    }
}

/// Helper to swap endianness for injection code (little-endian)
fn swap_endian(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// Injector for D2 game functions
#[cfg(target_os = "windows")]
pub struct D2Injector {
    /// Allocated buffer for strings/data in game memory
    pub string_buffer: RemoteAlloc,
    /// Allocated buffer for parameters
    pub params_buffer: RemoteAlloc,
    
    /// Addresses of injected functions
    pub inject_print: usize,
    pub inject_get_string: usize,
    pub inject_get_item_name: usize,
    pub inject_get_item_stat: usize,
    pub inject_get_unit_stat: usize,
}

#[cfg(target_os = "windows")]
impl D2Injector {
    /// Create a new injector and inject all necessary functions
    pub fn new(
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
    ) -> Result<Self, String> {
        // Allocate buffers in game memory
        let string_buffer = RemoteAlloc::new(process, 0x1000)?;
        let params_buffer = RemoteAlloc::new(process, 0x100)?;

        let inject_base = d2_client + d2client::INJECT_BASE;
        let inject_print = inject_base + d2client::inject::PRINT;
        let inject_get_string = inject_base + d2client::inject::GET_STRING;
        let inject_get_item_name = inject_base + d2client::inject::GET_ITEM_NAME;
        let inject_get_item_stat = inject_base + d2client::inject::GET_ITEM_STAT;
        let inject_get_unit_stat = inject_base + d2common::INJECT_GET_UNIT_STAT;

        let injector = Self {
            string_buffer,
            params_buffer,
            inject_print,
            inject_get_string,
            inject_get_item_name,
            inject_get_item_stat,
            inject_get_unit_stat,
        };

        // Inject the code
        injector.inject_functions(process, d2_client, d2_common)?;

        Ok(injector)
    }

    /// Inject all helper functions into game memory
    fn inject_functions(
        &self,
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
    ) -> Result<(), String> {
        let inject_base = d2_client + d2client::INJECT_BASE;
        let string_addr = self.string_buffer.address as u32;
        let _params_addr = self.params_buffer.address as u32;

        // PrintString injection
        // D2Client.dll+CDE01 - 53                    - push ebx
        // D2Client.dll+CDE01 - 68 *                  - push D2Client.dll+CDE20 (string addr)
        // D2Client.dll+CDE06 - 31 C0                 - xor eax,eax
        // D2Client.dll+CDE08 - E8 *                  - call D2Client.dll+7D850 (print func)
        // D2Client.dll+CDE0D - C3                    - ret
        let print_offset = (d2_client + d2client::func::PRINT_STRING) as i32
            - (inject_base + d2client::inject::PRINT + 0x0D) as i32;
        let mut print_code: Vec<u8> = vec![0x53, 0x68];
        print_code.extend_from_slice(&swap_endian(string_addr));
        print_code.extend_from_slice(&[0x31, 0xC0, 0xE8]);
        print_code.extend_from_slice(&(print_offset as u32).to_le_bytes());
        print_code.push(0xC3);
        process.write_buffer(self.inject_print, &print_code)?;

        // GetString injection (D2Lang_GetStringById)
        // D2Client.dll+CDE10 - 8B CB                 - mov ecx,ebx
        // D2Client.dll+CDE12 - E8 *                  - call D2Lang.dll+GetStringById
        // D2Client.dll+CDE17 - C3                    - ret
        // Note: This needs D2Lang address, simplified version stores result
        let get_string_code: Vec<u8> = vec![0x8B, 0xCB, 0xC3]; // mov ecx,ebx; ret (simplified)
        process.write_buffer(self.inject_get_string, &get_string_code)?;

        // GetItemName injection
        // push 0x100 (max length)
        // push string_addr
        // push ebx (pUnit)
        // call D2Client+0x914F0
        // ret
        let get_name_offset = (d2_client + d2client::func::GET_ITEM_NAME) as i32
            - (inject_base + d2client::inject::GET_ITEM_NAME + 0x10) as i32;
        let mut get_name_code: Vec<u8> = vec![0x68, 0x00, 0x01, 0x00, 0x00, 0x68];
        get_name_code.extend_from_slice(&swap_endian(string_addr));
        get_name_code.push(0x53); // push ebx
        get_name_code.push(0xE8);
        get_name_code.extend_from_slice(&(get_name_offset as u32).to_le_bytes());
        get_name_code.push(0xC3);
        process.write_buffer(self.inject_get_item_name, &get_name_code)?;

        // GetItemStat injection
        // push edi
        // mov edi, string_addr
        // push 0
        // push 0
        // push ebx (pUnit)
        // call D2Client+0x560B0
        // pop edi
        // ret
        let get_stat_offset = (d2_client + d2client::func::GET_ITEM_STAT) as i32
            - (inject_base + d2client::inject::GET_ITEM_STAT + 0x0E) as i32;
        let mut get_stat_code: Vec<u8> = vec![0x57, 0xBF];
        get_stat_code.extend_from_slice(&swap_endian(string_addr));
        get_stat_code.extend_from_slice(&[0x6A, 0x00, 0x6A, 0x00, 0x53, 0xE8]);
        get_stat_code.extend_from_slice(&(get_stat_offset as u32).to_le_bytes());
        get_stat_code.extend_from_slice(&[0x5F, 0xC3]);
        process.write_buffer(self.inject_get_item_stat, &get_stat_code)?;

        // GetUnitStat injection
        // push 0
        // push [ebx] (stat id)
        // push [ebx+4] (pUnit)
        // call D2Common+0x38B70
        // mov [string_addr], eax
        // ret
        let get_unit_stat_offset = (d2_common + d2common::GET_UNIT_STAT) as i32
            - (inject_base + d2common::INJECT_GET_UNIT_STAT + 0x0B) as i32;
        let mut get_unit_stat_code: Vec<u8> = vec![0x6A, 0x00, 0xFF, 0x33, 0xFF, 0x73, 0x04, 0xE8];
        get_unit_stat_code.extend_from_slice(&(get_unit_stat_offset as u32).to_le_bytes());
        get_unit_stat_code.push(0xA3);
        get_unit_stat_code.extend_from_slice(&swap_endian(string_addr));
        get_unit_stat_code.push(0xC3);
        process.write_buffer(self.inject_get_unit_stat, &get_unit_stat_code)?;

        Ok(())
    }

    /// Get item name by calling the injected function
    pub fn get_item_name(&self, process: &ProcessHandle, p_unit: u32) -> Result<String, String> {
        // Clear buffer before use
        let zeros = vec![0u8; 256];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemName with pUnit in EBX
        remote_thread(process, self.inject_get_item_name, p_unit as usize)?;

        // Read the result string (wide char)
        let buffer = process.read_buffer(self.string_buffer.address, 256)?;
        
        // Convert from UTF-16LE to String
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();
        
        Ok(String::from_utf16_lossy(&wide))
    }

    /// Get item stats by calling the injected function
    pub fn get_item_stats(&self, process: &ProcessHandle, p_unit: u32) -> Result<String, String> {
        // Clear buffer before use
        let zeros = vec![0u8; 2048];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemStats with pUnit in EBX
        remote_thread(process, self.inject_get_item_stat, p_unit as usize)?;

        // Read the result string
        let buffer = process.read_buffer(self.string_buffer.address, 2048)?;
        
        // Convert from UTF-16LE to String
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();
        
        Ok(String::from_utf16_lossy(&wide))
    }

    /// Get a unit stat value
    pub fn get_unit_stat(
        &self,
        process: &ProcessHandle,
        p_unit: u32,
        stat_id: u32,
    ) -> Result<u32, String> {
        // Write params: [stat_id, p_unit]
        process.write_buffer(self.params_buffer.address, &stat_id.to_le_bytes())?;
        process.write_buffer(self.params_buffer.address + 4, &p_unit.to_le_bytes())?;

        // Call GetUnitStat with params pointer in EBX
        remote_thread(
            process,
            self.inject_get_unit_stat,
            self.params_buffer.address,
        )?;

        // Read result from string buffer
        process.read_memory::<u32>(self.string_buffer.address)
    }

    /// Print a string to the game chat
    pub fn print_string(
        &self,
        process: &ProcessHandle,
        text: &str,
        color: u32,
    ) -> Result<(), String> {
        // Convert string to UTF-16LE and write to buffer
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: Vec<u8> = wide.iter().flat_map(|&c| c.to_le_bytes()).collect();
        
        process.write_buffer(self.string_buffer.address, &bytes)?;

        // Call PrintString with color in EBX
        remote_thread(process, self.inject_print, color as usize)?;

        Ok(())
    }
}

// --- Stub for Non-Windows ---

#[cfg(not(target_os = "windows"))]
pub struct RemoteAlloc {
    pub address: usize,
}

#[cfg(not(target_os = "windows"))]
pub struct D2Injector;

#[cfg(not(target_os = "windows"))]
impl D2Injector {
    pub fn new(
        _process: &crate::process::ProcessHandle,
        _d2_client: usize,
        _d2_common: usize,
    ) -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
}

