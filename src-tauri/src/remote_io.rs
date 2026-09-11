//! Borrowed remote read/write support shared by DPS and hovered-item capture.

#[cfg(target_os = "linux")]
use crate::process::ProcessHandle;
#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};

/// Opaque per-OS process reference threaded through `HookState`/
/// `RingReader`/`read_remote`/`write_remote` — a Win32 `HANDLE` on
/// Windows, a bare pid on Linux (matching `process::ProcessHandle`'s own
/// shape there). Letting `RingReader`/the shared prologue-classification
/// logic stay OS-agnostic beyond this one type.
#[cfg(target_os = "windows")]
pub(crate) type ProcessRef = HANDLE;
#[cfg(target_os = "linux")]
pub(crate) type ProcessRef = u32;

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
