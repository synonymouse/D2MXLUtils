#[cfg(target_os = "windows")]
use std::ffi::{c_void, OsStr};
#[cfg(target_os = "windows")]
use std::mem;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::{
    EnumProcessModules, GetModuleBaseNameW, GetModuleInformation, MODULEINFO,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

// --- Windows Implementation ---

#[cfg(target_os = "windows")]
pub struct ProcessHandle {
    pub handle: HANDLE,
    pub pid: u32,
}

#[cfg(target_os = "windows")]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

// SAFETY: HANDLE is a kernel-object reference; the Win32 calls we make on
// it are thread-safe. CloseHandle runs once via Drop on the last Arc.
#[cfg(target_os = "windows")]
unsafe impl Send for ProcessHandle {}
#[cfg(target_os = "windows")]
unsafe impl Sync for ProcessHandle {}

#[cfg(target_os = "windows")]
impl ProcessHandle {
    pub fn read_memory<T: Copy>(&self, address: usize) -> Result<T, String> {
        let mut buffer: T = unsafe { mem::zeroed() };
        let mut bytes_read: usize = 0;

        unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                &mut buffer as *mut T as *mut c_void,
                mem::size_of::<T>(),
                Some(&mut bytes_read),
            )
            .map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
        }

        if bytes_read != mem::size_of::<T>() {
            return Err("Incomplete read".to_string());
        }

        Ok(buffer)
    }

    pub fn read_buffer(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        let mut buffer = vec![0u8; size];
        let mut bytes_read: usize = 0;

        unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                size,
                Some(&mut bytes_read),
            )
            .map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
        }

        if bytes_read != size {
            return Err("Incomplete read".to_string());
        }

        Ok(buffer)
    }

    /// Read into an existing buffer slice
    pub fn read_buffer_into(&self, address: usize, buffer: &mut [u8]) -> Result<(), String> {
        let mut bytes_read: usize = 0;

        unsafe {
            ReadProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                buffer.len(),
                Some(&mut bytes_read),
            )
            .map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
        }

        if bytes_read != buffer.len() {
            return Err("Incomplete read".to_string());
        }

        Ok(())
    }

    pub fn write_buffer(&self, address: usize, buffer: &[u8]) -> Result<(), String> {
        let mut bytes_written: usize = 0;
        unsafe {
            WriteProcessMemory(
                self.handle,
                address as *const c_void,
                buffer.as_ptr() as *const c_void,
                buffer.len(),
                Some(&mut bytes_written),
            )
            .map_err(|e| format!("WriteProcessMemory failed: {}", e))?;
        }

        if bytes_written != buffer.len() {
            return Err("Incomplete write".to_string());
        }

        Ok(())
    }

    pub fn get_module_base(&self, module_name: &str) -> Result<usize, String> {
        self.get_module_info(module_name).map(|(base, _)| base)
    }

    /// Resolve a module by name into `(base, SizeOfImage)`.
    pub fn get_module_info(&self, module_name: &str) -> Result<(usize, usize), String> {
        let mut modules = [Default::default(); 1024];
        let mut cb_needed = 0;

        unsafe {
            EnumProcessModules(
                self.handle,
                modules.as_mut_ptr(),
                (modules.len() * mem::size_of::<HINSTANCE>()) as u32,
                &mut cb_needed,
            )
            .map_err(|e| format!("EnumProcessModules failed: {}", e))?;
        }

        let module_count = cb_needed as usize / mem::size_of::<HINSTANCE>();
        for i in 0..module_count {
            let module = modules[i];
            let mut buffer = [0u16; 256];
            let len = unsafe { GetModuleBaseNameW(self.handle, module, &mut buffer) };

            if len > 0 {
                let name = String::from_utf16_lossy(&buffer[..len as usize]);
                if name.eq_ignore_ascii_case(module_name) {
                    let mut info = MODULEINFO::default();
                    unsafe {
                        GetModuleInformation(
                            self.handle,
                            module,
                            &mut info,
                            mem::size_of::<MODULEINFO>() as u32,
                        )
                        .map_err(|e| format!("GetModuleInformation failed: {}", e))?;
                    }
                    return Ok((info.lpBaseOfDll as usize, info.SizeOfImage as usize));
                }
            }
        }

        Err(format!("Module '{}' not found", module_name))
    }

    /// Scan memory for a byte pattern within a given range.
    /// Returns the address where the pattern was found, or None.
    pub fn scan_pattern(&self, start: usize, size: usize, pattern: &[u8]) -> Option<usize> {
        if pattern.is_empty() || size < pattern.len() {
            return None;
        }

        // Read memory in chunks to avoid huge allocations
        const CHUNK_SIZE: usize = 0x10000; // 64KB chunks
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut offset = 0;

        while offset < size {
            let read_size = std::cmp::min(CHUNK_SIZE, size - offset);
            let addr = start + offset;

            // Try to read this chunk
            let mut bytes_read: usize = 0;
            let result = unsafe {
                ReadProcessMemory(
                    self.handle,
                    addr as *const c_void,
                    buffer.as_mut_ptr() as *mut c_void,
                    read_size,
                    Some(&mut bytes_read),
                )
            };

            if result.is_err() || bytes_read == 0 {
                // Skip unreadable regions
                offset += CHUNK_SIZE;
                continue;
            }

            // Search for pattern in this chunk
            let search_len = if bytes_read >= pattern.len() {
                bytes_read - pattern.len() + 1
            } else {
                0
            };

            for i in 0..search_len {
                if &buffer[i..i + pattern.len()] == pattern {
                    return Some(addr + i);
                }
            }

            // Overlap by pattern length at chunk boundaries; .max(1) guarantees
            // forward progress at the tail where read_size < pattern.len() would
            // otherwise yield 0 and loop forever.
            offset += read_size.saturating_sub(pattern.len()).max(1);
        }

        None
    }

    /// Scan memory for a byte pattern where `None` matches any byte.
    /// `start_from` skips matches before that absolute address — pass `start`
    /// for a full scan or `last_hit + 1` to resume.
    pub fn scan_pattern_wildcard(
        &self,
        start: usize,
        size: usize,
        pattern: &[Option<u8>],
        start_from: usize,
    ) -> Option<usize> {
        if pattern.is_empty() || size < pattern.len() {
            return None;
        }

        const CHUNK_SIZE: usize = 0x10000;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut offset = 0;

        while offset < size {
            let read_size = std::cmp::min(CHUNK_SIZE, size - offset);
            let addr = start + offset;

            let mut bytes_read: usize = 0;
            let result = unsafe {
                ReadProcessMemory(
                    self.handle,
                    addr as *const c_void,
                    buffer.as_mut_ptr() as *mut c_void,
                    read_size,
                    Some(&mut bytes_read),
                )
            };

            if result.is_err() || bytes_read == 0 {
                offset += CHUNK_SIZE;
                continue;
            }

            let search_len = if bytes_read >= pattern.len() {
                bytes_read - pattern.len() + 1
            } else {
                0
            };

            for i in 0..search_len {
                let candidate = addr + i;
                if candidate < start_from {
                    continue;
                }
                let window = &buffer[i..i + pattern.len()];
                if pattern
                    .iter()
                    .zip(window.iter())
                    .all(|(p, b)| p.map_or(true, |x| x == *b))
                {
                    return Some(candidate);
                }
            }

            // `.max(1)`: at the module tail `read_size < pattern.len()`
            // would otherwise yield 0 and loop forever (matches `scan_pattern`).
            offset += read_size.saturating_sub(pattern.len()).max(1);
        }

        None
    }
}

#[cfg(target_os = "windows")]
pub fn open_process_by_window_class(class_name: &str) -> Result<ProcessHandle, String> {
    unsafe {
        let wide_class_name: Vec<u16> = OsStr::new(class_name)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let hwnd = FindWindowW(PCWSTR(wide_class_name.as_ptr()), PCWSTR::null())
            .map_err(|_| format!("Window class '{}' not found", class_name))?;

        if hwnd.0.is_null() {
            return Err(format!("Window class '{}' not found", class_name));
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if pid == 0 {
            return Err("Failed to get process ID".to_string());
        }

        // Request only necessary permissions for memory reading/writing and thread creation
        let access_flags = PROCESS_VM_READ
            | PROCESS_VM_WRITE
            | PROCESS_VM_OPERATION
            | PROCESS_QUERY_INFORMATION
            | PROCESS_CREATE_THREAD;

        let handle = OpenProcess(access_flags, false, pid)
            .map_err(|e| format!("Failed to open process: {}", e))?;

        Ok(ProcessHandle { handle, pid })
    }
}

/// AOB anchoring on the lazy-init body of the always-show-items getter
/// `D2Sigma+0x57470`. The 4 bytes after the leading `A1` are the absolute
/// VA of the cached struct pointer.
///
/// ```text
/// A1 ?? ?? ?? ?? 85 C0 75 ?? 56 68 D0 00 00 00 E8 ?? ?? ?? ?? 8B F0
/// ```
#[cfg(target_os = "windows")]
const ALWAYS_SHOW_ITEMS_GETTER_PATTERN: &[Option<u8>] = &[
    Some(0xA1), None, None, None, None,
    Some(0x85), Some(0xC0),
    Some(0x75), None,
    Some(0x56),
    Some(0x68), Some(0xD0), Some(0x00), Some(0x00), Some(0x00),
    Some(0xE8), None, None, None, None,
    Some(0x8B), Some(0xF0),
];

/// `None` on no match, ambiguous match (>1 hit = signature too loose to trust),
/// or out-of-module decoded address.
#[cfg(target_os = "windows")]
fn resolve_always_show_items_ptr_rva(
    process: &ProcessHandle,
    base: usize,
    size: usize,
) -> Option<usize> {
    let first = process.scan_pattern_wildcard(
        base,
        size,
        ALWAYS_SHOW_ITEMS_GETTER_PATTERN,
        base,
    )?;
    if process
        .scan_pattern_wildcard(base, size, ALWAYS_SHOW_ITEMS_GETTER_PATTERN, first + 1)
        .is_some()
    {
        return None;
    }
    let abs_va = process.read_memory::<u32>(first + 1).ok()? as usize;
    if abs_va < base || abs_va >= base.saturating_add(size) {
        return None;
    }
    Some(abs_va - base)
}

#[cfg(target_os = "windows")]
pub struct D2Context {
    pub process: ProcessHandle,
    pub d2_client: usize,
    pub d2_common: usize,
    pub d2_win: usize,
    pub d2_lang: usize,
    pub d2_sigma: usize,
    /// `None` if the AOB signature didn't resolve — feature unavailable.
    pub always_show_items_ptr_rva: Option<usize>,
}

#[cfg(target_os = "windows")]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        let process = open_process_by_window_class("Diablo II")?;
        let d2_client = process.get_module_base("D2Client.dll")?;
        let d2_common = process.get_module_base("D2Common.dll")?;
        let d2_win = process.get_module_base("D2Win.dll")?;
        let d2_lang = process.get_module_base("D2Lang.dll")?;
        let (d2_sigma, d2_sigma_size) = process
            .get_module_info("D2Sigma.dll")
            .unwrap_or((0, 0));

        let always_show_items_ptr_rva = if d2_sigma != 0 && d2_sigma_size != 0 {
            let rva = resolve_always_show_items_ptr_rva(&process, d2_sigma, d2_sigma_size);
            match rva {
                Some(rva) => crate::logger::info(&format!(
                    "Resolved always-show-items static at D2Sigma+{:#x}",
                    rva
                )),
                None => crate::logger::error(
                    "always-show-items: AOB signature did not resolve in D2Sigma.dll",
                ),
            }
            rva
        } else {
            None
        };

        Ok(Self {
            process,
            d2_client,
            d2_common,
            d2_win,
            d2_lang,
            d2_sigma,
            always_show_items_ptr_rva,
        })
    }
}

// --- Stub for Non-Windows (Compilation only) ---

#[cfg(not(target_os = "windows"))]
pub struct D2Context {
    pub d2_client: usize,
}

#[cfg(not(target_os = "windows"))]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
}
