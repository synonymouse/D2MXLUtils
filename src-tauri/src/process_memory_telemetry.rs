//! Process memory telemetry for Diablo II (Game.exe).
//!
//! Samples process working set, private commit, total free virtual memory,
//! and the largest contiguous free virtual block. This enables empirical
//! tracking of address space consumption, heap growth, and virtual memory
//! fragmentation during live gameplay sessions.

#![cfg(any(target_os = "windows", target_os = "linux"))]

#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_FREE, MEM_RESERVE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};

use crate::process::ProcessHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum VirtualAddressSpaceMetrics {
    Complete {
        total_free_bytes: usize,
        largest_free_block_bytes: usize,
        total_committed_bytes: usize,
        total_reserved_bytes: usize,
    },
    IncompleteScan {
        scanned_up_to: usize,
    },
    Unavailable,
}

impl Default for VirtualAddressSpaceMetrics {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ProcessMemorySnapshot {
    pub working_set_bytes: Option<usize>,
    pub private_commit_bytes: Option<usize>,
    pub virtual_memory: VirtualAddressSpaceMetrics,
}

impl ProcessMemorySnapshot {
    pub fn summary_string(&self) -> String {
        let ws_str = self
            .working_set_bytes
            .map(|b| format!("{:.1}MB", b as f64 / 1_048_576.0))
            .unwrap_or_else(|| "unavailable".to_string());
        let commit_str = self
            .private_commit_bytes
            .map(|b| format!("{:.1}MB", b as f64 / 1_048_576.0))
            .unwrap_or_else(|| "unavailable".to_string());

        let vm_str = match self.virtual_memory {
            VirtualAddressSpaceMetrics::Complete {
                total_free_bytes,
                largest_free_block_bytes,
                ..
            } => {
                format!(
                    "FreeVM={:.1}MB LargestFreeBlock={:.1}MB",
                    total_free_bytes as f64 / 1_048_576.0,
                    largest_free_block_bytes as f64 / 1_048_576.0
                )
            }
            VirtualAddressSpaceMetrics::IncompleteScan { scanned_up_to } => {
                format!("FreeVM=incomplete(scanned_to=0x{:08X})", scanned_up_to)
            }
            VirtualAddressSpaceMetrics::Unavailable => "FreeVM=unavailable".to_string(),
        };

        format!("WS={} Commit={} | {}", ws_str, commit_str, vm_str)
    }
}

#[cfg(target_os = "windows")]
pub fn sample_process_memory(process: &ProcessHandle) -> Result<ProcessMemorySnapshot, String> {
    let mut snapshot = ProcessMemorySnapshot::default();

    // 1. Working set and private commit bytes via GetProcessMemoryInfo
    let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
    let pmc_size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    unsafe {
        if GetProcessMemoryInfo(process.handle, &mut pmc as *mut _ as *mut _, pmc_size).is_ok() {
            snapshot.working_set_bytes = Some(pmc.WorkingSetSize);
            snapshot.private_commit_bytes = Some(pmc.PrivateUsage);
        }
    }

    // 2. Virtual address space scan via VirtualQueryEx (scan 32-bit user space: 0 .. 0x7FFE_0000)
    let mut cursor = 0usize;
    const USER32_MAX_ADDR: usize = 0x7FFE_0000;
    let mut total_free = 0usize;
    let mut largest_free = 0usize;
    let mut total_commit = 0usize;
    let mut total_reserve = 0usize;
    let mut queried_any = false;

    while cursor < USER32_MAX_ADDR {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let bytes_queried = unsafe {
            VirtualQueryEx(
                process.handle,
                Some(cursor as *const c_void),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };

        if bytes_queried == 0 || info.RegionSize == 0 {
            break;
        }
        queried_any = true;

        match info.State {
            MEM_FREE => {
                total_free = total_free.saturating_add(info.RegionSize);
                if info.RegionSize > largest_free {
                    largest_free = info.RegionSize;
                }
            }
            MEM_COMMIT => {
                total_commit = total_commit.saturating_add(info.RegionSize);
            }
            MEM_RESERVE => {
                total_reserve = total_reserve.saturating_add(info.RegionSize);
            }
            _ => {}
        }

        let next = cursor.saturating_add(info.RegionSize);
        if next <= cursor {
            break;
        }
        cursor = next;
    }

    if queried_any && cursor >= USER32_MAX_ADDR {
        snapshot.virtual_memory = VirtualAddressSpaceMetrics::Complete {
            total_free_bytes: total_free,
            largest_free_block_bytes: largest_free,
            total_committed_bytes: total_commit,
            total_reserved_bytes: total_reserve,
        };
    } else if queried_any {
        snapshot.virtual_memory = VirtualAddressSpaceMetrics::IncompleteScan {
            scanned_up_to: cursor,
        };
    } else {
        snapshot.virtual_memory = VirtualAddressSpaceMetrics::Unavailable;
    }

    Ok(snapshot)
}

#[cfg(target_os = "linux")]
pub fn sample_process_memory(_process: &ProcessHandle) -> Result<ProcessMemorySnapshot, String> {
    Ok(ProcessMemorySnapshot::default())
}
