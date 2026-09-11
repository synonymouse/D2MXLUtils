//! Linux injector construction and ptrace-backed execution.

use super::{D2Injector, RemoteAlloc};
use crate::offsets::{d2client, d2common};
use crate::process::ProcessHandle;
use crate::stat_telemetry::StatTelemetryCounters;

/// Linux analog of `CreateRemoteThread` — `ptrace` has no "spawn a thread"
/// primitive, so this hijacks one existing thread's execution context
/// instead: freeze it, redirect `EIP`/`ESP` into the shellcode with a
/// scratch stack, run to a planted `0xCC` trap (every shellcode routine
/// already ends in a bare `ret`, which pops our fake return address and
/// lands on the trap), read `EAX`, then restore every original register
/// exactly and detach. Mirrors `GetExitCodeThread`'s role by returning EAX.
pub fn remote_thread(
    process: &ProcessHandle,
    func_addr: usize,
    param: usize,
) -> Result<u32, String> {
    crate::process::linux_ptrace::call_remote(process.pid, func_addr, param)
}

// `string_buffer`/`params_buffer` need a real allocation the same way
// `VirtualAllocEx` provides one on Windows — scavenging `D2Client.dll`'s
// padding turned out to be unsafe past a couple hundred bytes (empirically,
// only `INJECT_BASE+0x0..0x200` is genuinely free; further offsets contain
// real data). Linux has no direct `VirtualAllocEx` equivalent reachable
// without code already running in the target, so we get one the way any
// native Linux injector would: write a tiny hand-built `mmap2` syscall stub
// into the confirmed-free padding and run it once via `remote_thread`,
// exactly like the permanent shellcode stubs in install.rs.
impl D2Injector {
    pub fn new(
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
        d2_lang: usize,
    ) -> Result<Self, String> {
        let inject_base = d2_client + d2client::INJECT_BASE;

        // One 0x2000 (8 KiB) mapping covers both buffers: string_buffer at
        // +0x0 (4096 bytes, matches the wchar[2048] stats buffer size),
        // params_buffer at +0x1000. See `ProcessHandle::mmap_remote` for
        // the mechanism (shared with `dps/hook/linux.rs`'s own region allocation).
        let mmap_stub_addr = inject_base + d2client::inject::LINUX_MMAP_STUB;
        let mapped = process.mmap_remote(mmap_stub_addr, 0x2000)?;

        let string_buffer = RemoteAlloc { address: mapped };
        let params_buffer = RemoteAlloc {
            address: mapped + 0x1000,
        };

        let inject_get_string = inject_base + d2client::inject::GET_STRING;
        let inject_get_item_name = inject_base + d2client::inject::GET_ITEM_NAME;
        let inject_get_item_stat = inject_base + d2client::inject::GET_ITEM_STAT;
        let inject_get_unit_stat = inject_base + d2common::INJECT_GET_UNIT_STAT;
        let inject_new_automap_cell = inject_base + d2client::inject::NEW_AUTOMAP_CELL;

        let injector = Self {
            telemetry: StatTelemetryCounters::default(),
            string_buffer,
            params_buffer,
            inject_get_string,
            inject_get_item_name,
            inject_get_item_stat,
            inject_get_unit_stat,
            inject_new_automap_cell,
        };

        injector.inject_functions(process, d2_client, d2_common, d2_lang)?;

        Ok(injector)
    }
}
