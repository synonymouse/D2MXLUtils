//! D2 code injection module
//! Provides functionality for injecting code and calling game functions via remote threads

#[cfg(target_os = "windows")]
use ::windows::Win32::Foundation::HANDLE;

use crate::stat_telemetry::StatTelemetryCounters;

mod calls;
mod install;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::remote_thread;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::remote_thread;

#[cfg(all(test, target_os = "windows"))]
mod test_support;
#[cfg(all(test, target_os = "windows"))]
pub(crate) use test_support::MarkerTestAllocator;

/// Allocated memory region in the target process
#[cfg(target_os = "windows")]
pub struct RemoteAlloc {
    handle: HANDLE,
    pub address: usize,
    size: usize,
}

// NOTE: We intentionally do NOT free the remote memory in Drop.
// The OS will reclaim the memory when the game process exits. We no longer
// reuse the same buffers across different scanner instances or game
// processes – each `D2Injector` allocates its own buffers – but we still
// avoid calling VirtualFreeEx manually to keep the implementation simple
// and robust across process restarts.
#[cfg(target_os = "windows")]
impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        // Intentionally no-op: remote memory is released when the
        // Diablo II process terminates.
    }
}

// SAFETY: holds a Win32 HANDLE and a usize remote-process address; both
// are immutable after construction and Win32 ops on them are thread-safe.
#[cfg(target_os = "windows")]
unsafe impl Send for RemoteAlloc {}
#[cfg(target_os = "windows")]
unsafe impl Sync for RemoteAlloc {}

/// Injector for D2 game functions
#[cfg(target_os = "windows")]
pub struct D2Injector {
    pub(crate) telemetry: StatTelemetryCounters,
    #[cfg(test)]
    pub(crate) marker_allocator: Option<std::sync::Mutex<MarkerTestAllocator>>,
    /// Allocated buffer for strings/data in game memory
    pub string_buffer: RemoteAlloc,
    /// Allocated buffer for parameters
    pub params_buffer: RemoteAlloc,

    /// Addresses of injected functions
    pub inject_get_string: usize,
    pub inject_get_item_name: usize,
    pub inject_get_item_stat: usize,
    pub inject_get_unit_stat: usize,
    pub inject_new_automap_cell: usize,
}

// SAFETY: Send is sound because all state lives in remote process memory;
// Sync is sound only because callers wrap in `Arc<Mutex<D2Injector>>` —
// `string_buffer`/`params_buffer` are a shared scratch arena and concurrent
// `&D2Injector` calls would corrupt each other. See `scanner_state.rs`.
#[cfg(target_os = "windows")]
unsafe impl Send for D2Injector {}
#[cfg(target_os = "windows")]
unsafe impl Sync for D2Injector {}

#[cfg(target_os = "linux")]
pub struct RemoteAlloc {
    pub address: usize,
}

#[cfg(target_os = "linux")]
pub struct D2Injector {
    pub(crate) telemetry: StatTelemetryCounters,
    pub string_buffer: RemoteAlloc,
    pub params_buffer: RemoteAlloc,
    pub inject_get_string: usize,
    pub inject_get_item_name: usize,
    pub inject_get_item_stat: usize,
    pub inject_get_unit_stat: usize,
    pub inject_new_automap_cell: usize,
}

// --- Stub for other OSes (compilation only) ---

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct RemoteAlloc {
    pub address: usize,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct D2Injector;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl D2Injector {
    pub fn new(
        _process: &crate::process::ProcessHandle,
        _d2_client: usize,
        _d2_common: usize,
        _d2_lang: usize,
    ) -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
}
