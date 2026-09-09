use crate::process::ProcessHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bytes(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessMemorySample {
    pub working_set_bytes: Bytes,
    pub private_commit_bytes: Bytes,
}

pub(crate) fn sample(process: &ProcessHandle) -> Option<ProcessMemorySample> {
    #[cfg(target_os = "windows")]
    {
        query_handle(process.handle)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = process;
        None
    }
}

#[cfg(target_os = "windows")]
pub(super) fn query_handle(
    handle: windows::Win32::Foundation::HANDLE,
) -> Option<ProcessMemorySample> {
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>()).ok()?;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size,
        ..Default::default()
    };
    // SAFETY: FFI receives an aligned, initialized EX buffer of the advertised size.
    // WinAPI accepts its common-prefix pointer and rejects invalid process handles.
    unsafe {
        GetProcessMemoryInfo(
            handle,
            std::ptr::from_mut(&mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            size,
        )
    }
    .ok()?;
    Some(ProcessMemorySample {
        working_set_bytes: Bytes(u64::try_from(counters.WorkingSetSize).ok()?),
        private_commit_bytes: Bytes(u64::try_from(counters.PrivateUsage).ok()?),
    })
}
