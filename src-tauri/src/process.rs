#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use std::ffi::{c_void, OsStr};
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::mem;

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
            unsafe { let _ = CloseHandle(self.handle); }
        }
    }
}

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
                Some(&mut bytes_read)
            ).map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
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
                Some(&mut bytes_read)
            ).map_err(|e| format!("ReadProcessMemory failed: {}", e))?;
        }

        if bytes_read != size {
             return Err("Incomplete read".to_string());
        }

        Ok(buffer)
    }

    pub fn write_buffer(&self, address: usize, buffer: &[u8]) -> Result<(), String> {
        let mut bytes_written: usize = 0;
        unsafe {
            WriteProcessMemory(
                 self.handle,
                 address as *const c_void,
                 buffer.as_ptr() as *const c_void,
                 buffer.len(),
                 Some(&mut bytes_written)
            ).map_err(|e| format!("WriteProcessMemory failed: {}", e))?;
        }
        
        if bytes_written != buffer.len() {
            return Err("Incomplete write".to_string());
        }
        
        Ok(())
    }

    pub fn get_module_base(&self, module_name: &str) -> Result<usize, String> {
        let mut modules = [Default::default(); 1024];
        let mut cb_needed = 0;

        unsafe {
            EnumProcessModules(
                self.handle,
                modules.as_mut_ptr(),
                (modules.len() * mem::size_of::<HINSTANCE>()) as u32,
                &mut cb_needed
            ).map_err(|e| format!("EnumProcessModules failed: {}", e))?;
        }

        let module_count = cb_needed as usize / mem::size_of::<HINSTANCE>();
        for i in 0..module_count {
            let module = modules[i];
            let mut buffer = [0u16; 256];
            let len = unsafe {
                GetModuleBaseNameW(self.handle, module, &mut buffer)
            };
            
            if len > 0 {
                let name = String::from_utf16_lossy(&buffer[..len as usize]);
                if name.eq_ignore_ascii_case(module_name) {
                    return Ok(module.0 as usize);
                }
            }
        }
        
        Err(format!("Module '{}' not found", module_name))
    }
}

#[cfg(target_os = "windows")]
pub fn open_process_by_window_class(class_name: &str) -> Result<ProcessHandle, String> {
    unsafe {
        let wide_class_name: Vec<u16> = OsStr::new(class_name).encode_wide().chain(Some(0)).collect();
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

#[cfg(target_os = "windows")]
pub struct D2Context {
    pub process: ProcessHandle,
    pub d2_client: usize,
    pub d2_common: usize,
    pub d2_win: usize,
    pub d2_lang: usize,
    pub d2_sigma: usize,
}

#[cfg(target_os = "windows")]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        let process = open_process_by_window_class("Diablo II")?;
        let d2_client = process.get_module_base("D2Client.dll")?;
        let d2_common = process.get_module_base("D2Common.dll")?;
        let d2_win = process.get_module_base("D2Win.dll")?;
        let d2_lang = process.get_module_base("D2Lang.dll")?;
        let d2_sigma = process.get_module_base("D2Sigma.dll").unwrap_or(0);

        Ok(Self {
            process,
            d2_client,
            d2_common,
            d2_win,
            d2_lang,
            d2_sigma,
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
