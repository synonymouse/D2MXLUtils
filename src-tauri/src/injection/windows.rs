//! Windows allocation, injector construction and remote-thread execution.

use std::ffi::c_void;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{
    VirtualAllocEx, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, WaitForSingleObject, INFINITE,
};

use super::{D2Injector, RemoteAlloc};
use crate::offsets::{d2client, d2common};
use crate::process::ProcessHandle;
use crate::stat_telemetry::StatTelemetryCounters;

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
pub fn remote_thread(
    process: &ProcessHandle,
    func_addr: usize,
    param: usize,
) -> Result<u32, String> {
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
        let result = GetExitCodeThread(thread, &mut exit_code)
            .map_err(|e| format!("GetExitCodeThread failed: {}", e));

        // CreateRemoteThread's handle was otherwise never closed — this
        // function runs on every injected remote-function call (item
        // name/stat lookups, automap cell creation, ...), called
        // repeatedly during normal play, so each call leaked one kernel
        // thread handle. Over a long session with hundreds/thousands of
        // item drops that's a large, steadily growing handle count —
        // exactly the shape of a Windows-specific "gets slower over
        // hours" bug. Close unconditionally, after reading the exit code
        // but regardless of whether that read succeeded.
        let _ = CloseHandle(thread);

        result?;
        Ok(exit_code)
    }
}

impl D2Injector {
    /// Create a new injector and inject all necessary functions
    pub fn new(
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
        d2_lang: usize,
    ) -> Result<Self, String> {
        // Allocate fresh buffers in the game process for this injector.
        // This avoids keeping stale addresses when the Diablo II process
        // is closed and later restarted, which previously broke
        // re-attachment of the scanner after the first game session.
        let string_buffer = RemoteAlloc::new(process, 0x1000)?;
        let params_buffer = RemoteAlloc::new(process, 0x100)?;

        let inject_base = d2_client + d2client::INJECT_BASE;
        let inject_get_string = inject_base + d2client::inject::GET_STRING;
        let inject_get_item_name = inject_base + d2client::inject::GET_ITEM_NAME;
        let inject_get_item_stat = inject_base + d2client::inject::GET_ITEM_STAT;
        let inject_get_unit_stat = inject_base + d2common::INJECT_GET_UNIT_STAT;
        let inject_new_automap_cell = inject_base + d2client::inject::NEW_AUTOMAP_CELL;

        let injector = Self {
            telemetry: StatTelemetryCounters::default(),
            #[cfg(test)]
            marker_allocator: None,
            string_buffer,
            params_buffer,
            inject_get_string,
            inject_get_item_name,
            inject_get_item_stat,
            inject_get_unit_stat,
            inject_new_automap_cell,
        };

        // Inject the code
        injector.inject_functions(process, d2_client, d2_common, d2_lang)?;

        Ok(injector)
    }
}
