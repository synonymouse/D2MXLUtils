//! D2 code injection module
//! Provides functionality for injecting code and calling game functions via remote threads

#[cfg(target_os = "windows")]
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetCurrentProcess, GetExitCodeThread, WaitForSingleObject, INFINITE,
};

use crate::offsets::{d2client, d2common, d2lang};
use crate::process::ProcessHandle;

/// Allocated memory region in the target process
#[cfg(target_os = "windows")]
pub struct RemoteAlloc {
    handle: HANDLE,
    pub address: usize,
    size: usize,
}

// Own a duplicate process handle: SharedScannerState may drop its context
// before the injector. Scratch calls are synchronous and finished before
// the injector is dropped. Published trampolines explicitly persist instead.
#[cfg(target_os = "windows")]
impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        unsafe {
            if self.address != 0 {
                let _ = VirtualFreeEx(self.handle, self.address as *mut c_void, 0, MEM_RELEASE);
            }
            let _ = CloseHandle(self.handle);
        }
    }
}

// SAFETY: holds a Win32 HANDLE and a usize remote-process address; both
// are immutable after construction and Win32 ops on them are thread-safe.
#[cfg(target_os = "windows")]
unsafe impl Send for RemoteAlloc {}
#[cfg(target_os = "windows")]
unsafe impl Sync for RemoteAlloc {}

#[cfg(target_os = "windows")]
impl RemoteAlloc {
    /// Allocate memory in the remote process
    pub fn new(process: &ProcessHandle, size: usize) -> Result<Self, String> {
        let mut handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                process.handle,
                GetCurrentProcess(),
                &mut handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .map_err(|e| format!("DuplicateHandle for remote allocation failed: {}", e))?;
        }
        let address = unsafe {
            VirtualAllocEx(
                handle,
                None,
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };

        if address.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("VirtualAllocEx failed".to_string());
        }

        Ok(Self {
            handle,
            address: address as usize,
            size,
        })
    }

    /// Transfer a published hook to the game-process lifetime. Its metadata
    /// allows later scanner instances to reattach without another allocation.
    pub fn persist(mut self) {
        self.address = 0;
    }
}

/// Published memory region in the target process.
///
/// Unlike scratch buffers used by completed remote calls, published memory is
/// linked into the game's internal data structures (e.g. Automap BST) and
/// traversed asynchronously by the game engine's render thread. It must NEVER
/// be freed via `VirtualFreeEx` while the target process is alive, as doing so
/// unmaps the page and causes access violations (0xC0000005) if the renderer
/// is mid-traversal or if any references survive.
///
/// Its lifetime is tied to the target game process (`Game.exe`); the OS kernel
/// reclaims the allocation cleanly upon process termination. Its remote address
/// is recorded at `d2client::inject::CELL_BUFFER_PTR` to allow scanner re-attachment
/// across restarts without re-allocating.
pub struct PublishedRemoteBuffer {
    pub address: usize,
    pub size: usize,
}

#[cfg(all(test, target_os = "windows"))]
mod allocation_tests {
    use super::*;
    use windows::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_FREE};

    fn local_process() -> ProcessHandle {
        let mut handle = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentProcess(),
                GetCurrentProcess(),
                &mut handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .unwrap();
        }
        ProcessHandle {
            handle,
            pid: std::process::id(),
        }
    }

    fn memory_state(address: usize) -> windows::Win32::System::Memory::VIRTUAL_ALLOCATION_TYPE {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        assert_ne!(
            unsafe {
                VirtualQueryEx(
                    GetCurrentProcess(),
                    Some(address as *const c_void),
                    &mut info,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            },
            0
        );
        info.State
    }

    #[test]
    fn scratch_storage_is_freed_even_after_original_process_handle_closes() {
        let process = local_process();
        let allocation = RemoteAlloc::new(&process, 0x1000).unwrap();
        let address = allocation.address;
        assert_eq!(memory_state(address), MEM_COMMIT);
        drop(process);
        drop(allocation);
        assert_eq!(memory_state(address), MEM_FREE);
    }

    #[test]
    fn published_trampoline_survives_owner_drop_for_reattachment() {
        let process = local_process();
        let allocation = RemoteAlloc::new(&process, 0x8000).unwrap();
        let address = allocation.address;
        allocation.persist();
        assert_eq!(memory_state(address), MEM_COMMIT);
        // This test never executes the region, so freeing it is safe.
        unsafe {
            VirtualFreeEx(process.handle, address as *mut c_void, 0, MEM_RELEASE).unwrap();
        }
    }
}

/// Execute a function in the remote process via CreateRemoteThread
/// Returns the thread exit code (which is often the return value of the function)
#[cfg(target_os = "windows")]
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

/// Linux analog of `CreateRemoteThread` — `ptrace` has no "spawn a thread"
/// primitive, so this hijacks one existing thread's execution context
/// instead: freeze it, redirect `EIP`/`ESP` into the shellcode with a
/// scratch stack, run to a planted `0xCC` trap (every shellcode routine
/// already ends in a bare `ret`, which pops our fake return address and
/// lands on the trap), read `EAX`, then restore every original register
/// exactly and detach. Mirrors `GetExitCodeThread`'s role by returning EAX.
#[cfg(target_os = "linux")]
pub fn remote_thread(
    process: &ProcessHandle,
    func_addr: usize,
    param: usize,
) -> Result<u32, String> {
    crate::process::linux_ptrace::call_remote(process.pid, func_addr, param)
}

/// Helper to swap endianness for injection code (little-endian)
fn swap_endian(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

/// Injector for D2 game functions
#[cfg(target_os = "windows")]
pub struct D2Injector {
    /// Allocated buffer for strings/data in game memory (scratch: freed on drop)
    pub string_buffer: RemoteAlloc,
    /// Allocated buffer for parameters (scratch: freed on drop)
    pub params_buffer: RemoteAlloc,
    /// Pre-allocated buffer for automap marker cells (published: game-process lifetime)
    pub cell_buffer: PublishedRemoteBuffer,

    /// Addresses of injected functions
    pub inject_get_string: usize,
    pub inject_get_item_name: usize,
    pub inject_get_item_stat: usize,
    pub inject_get_unit_stat: usize,
    pub inject_new_automap_cell: usize,

    pub remote_calls_get_string: AtomicU64,
    pub remote_calls_get_item_name: AtomicU64,
    pub remote_calls_get_item_stat: AtomicU64,
    pub remote_calls_get_unit_stat: AtomicU64,
    pub remote_calls_new_automap_cell: AtomicU64,
}

#[cfg(target_os = "windows")]
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
        let cell_buf_ptr_addr = inject_base + d2client::inject::CELL_BUFFER_PTR;

        // Lifetime policy: published marker cells survive in the game process.
        // Check if a published cell buffer already exists from a previous attach.
        let existing_addr = process.read_memory::<u32>(cell_buf_ptr_addr).unwrap_or(0) as usize;
        let cell_buffer_address =
            if existing_addr != 0 && process.read_memory::<u32>(existing_addr).is_ok() {
                existing_addr
            } else {
                let alloc = RemoteAlloc::new(process, 0x1000)?;
                let addr = alloc.address;
                alloc.persist(); // transfer to game process lifetime; never VirtualFreeEx while game is running
                let _ = process.write_buffer(cell_buf_ptr_addr, &(addr as u32).to_le_bytes());
                addr
            };
        let cell_buffer = PublishedRemoteBuffer {
            address: cell_buffer_address,
            size: 0x1000,
        };

        let inject_get_string = inject_base + d2client::inject::GET_STRING;
        let inject_get_item_name = inject_base + d2client::inject::GET_ITEM_NAME;
        let inject_get_item_stat = inject_base + d2client::inject::GET_ITEM_STAT;
        let inject_get_unit_stat = inject_base + d2common::INJECT_GET_UNIT_STAT;
        let inject_new_automap_cell = inject_base + d2client::inject::NEW_AUTOMAP_CELL;

        let injector = Self {
            string_buffer,
            params_buffer,
            cell_buffer,
            inject_get_string,
            inject_get_item_name,
            inject_get_item_stat,
            inject_get_unit_stat,
            inject_new_automap_cell,
            remote_calls_get_string: AtomicU64::new(0),
            remote_calls_get_item_name: AtomicU64::new(0),
            remote_calls_get_item_stat: AtomicU64::new(0),
            remote_calls_get_unit_stat: AtomicU64::new(0),
            remote_calls_new_automap_cell: AtomicU64::new(0),
        };

        // Inject the code
        injector.inject_functions(process, d2_client, d2_common, d2_lang)?;

        Ok(injector)
    }
}

/// Linux `D2Injector::new` — see below; shares this OS-agnostic
/// `RemoteAlloc`/field shape, so `inject_functions` (identical shellcode
/// install on every OS — it only calls the already-abstracted
/// `ProcessHandle::write_buffer`) lives once, in the shared impl block
/// below, rather than duplicated per OS.

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct RemoteCallStats {
    pub get_string: u64,
    pub get_item_name: u64,
    pub get_item_stat: u64,
    pub get_unit_stat: u64,
    pub new_automap_cell: u64,
}

impl RemoteCallStats {
    pub fn total(&self) -> u64 {
        self.get_string
            + self.get_item_name
            + self.get_item_stat
            + self.get_unit_stat
            + self.new_automap_cell
    }

    pub fn calculate_rates(
        &self,
        previous: &RemoteCallStats,
        elapsed_secs: f64,
    ) -> RemoteCallRates {
        if elapsed_secs <= 0.0001 {
            return RemoteCallRates::default();
        }
        RemoteCallRates {
            get_string: (self.get_string.saturating_sub(previous.get_string)) as f64 / elapsed_secs,
            get_item_name: (self.get_item_name.saturating_sub(previous.get_item_name)) as f64
                / elapsed_secs,
            get_item_stat: (self.get_item_stat.saturating_sub(previous.get_item_stat)) as f64
                / elapsed_secs,
            get_unit_stat: (self.get_unit_stat.saturating_sub(previous.get_unit_stat)) as f64
                / elapsed_secs,
            new_automap_cell: (self
                .new_automap_cell
                .saturating_sub(previous.new_automap_cell)) as f64
                / elapsed_secs,
            total: (self.total().saturating_sub(previous.total())) as f64 / elapsed_secs,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct RemoteCallRates {
    pub get_string: f64,
    pub get_item_name: f64,
    pub get_item_stat: f64,
    pub get_unit_stat: f64,
    pub new_automap_cell: f64,
    pub total: f64,
}

/// Public call wrappers — identical on every OS, since they only go through
/// the already OS-abstracted `remote_thread`/`ProcessHandle::read_buffer`/
/// `write_buffer`. Shared by both the Windows and Linux `D2Injector`, which
/// have the same field shape (`string_buffer`/`params_buffer: RemoteAlloc`,
/// `inject_*: usize`) even though `RemoteAlloc`/`new`/`inject_functions`
/// differ per OS above/below.
impl D2Injector {
    /// Retrieve instantaneous counts of remote-thread invocations by operation.
    pub fn remote_call_stats(&self) -> RemoteCallStats {
        RemoteCallStats {
            get_string: self.remote_calls_get_string.load(Ordering::Relaxed),
            get_item_name: self.remote_calls_get_item_name.load(Ordering::Relaxed),
            get_item_stat: self.remote_calls_get_item_stat.load(Ordering::Relaxed),
            get_unit_stat: self.remote_calls_get_unit_stat.load(Ordering::Relaxed),
            new_automap_cell: self.remote_calls_new_automap_cell.load(Ordering::Relaxed),
        }
    }

    /// Get item name by calling the injected function
    pub fn get_item_name(&self, process: &ProcessHandle, p_unit: u32) -> Result<String, String> {
        self.remote_calls_get_item_name
            .fetch_add(1, Ordering::Relaxed);
        // Clear buffer before use
        // Original D2Stats reads wchar[256] → 512 bytes
        let zeros = vec![0u8; 512];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemName with pUnit in EBX
        remote_thread(process, self.inject_get_item_name, p_unit as usize)?;

        // Read the result string (wide char)
        let buffer = process.read_buffer(self.string_buffer.address, 512)?;

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
        self.remote_calls_get_item_stat
            .fetch_add(1, Ordering::Relaxed);
        // Clear buffer before use
        // Original D2Stats reads wchar[2048] → 4096 bytes
        let zeros = vec![0u8; 4096];
        process.write_buffer(self.string_buffer.address, &zeros)?;

        // Call GetItemStats with pUnit in EBX
        remote_thread(process, self.inject_get_item_stat, p_unit as usize)?;

        // Read the result string
        let buffer = process.read_buffer(self.string_buffer.address, 4096)?;

        // Convert from UTF-16LE to String
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();

        Ok(String::from_utf16_lossy(&wide))
    }

    /// Resolve a string-table ID to a wide string by calling D2Lang_GetStringById.
    /// Used to read base item names from items.txt during tier-cache construction.
    ///
    /// Returns at most `max_chars` wide characters (stops at the first NUL).
    pub fn get_string(
        &self,
        process: &ProcessHandle,
        name_id: u16,
        max_chars: usize,
    ) -> Result<String, String> {
        self.remote_calls_get_string.fetch_add(1, Ordering::Relaxed);
        // remote_thread returns the thread's exit code which is EAX from our
        // shellcode — for GetStringById this is a pointer into the game's
        // string table (UTF-16).
        let str_ptr = remote_thread(process, self.inject_get_string, name_id as usize)? as usize;
        if str_ptr == 0 {
            return Ok(String::new());
        }

        let byte_len = max_chars.saturating_mul(2);
        let buffer = process.read_buffer(str_ptr, byte_len)?;
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|&c| c != 0)
            .collect();

        Ok(String::from_utf16_lossy(&wide))
    }

    /// Allocate a fresh `AutomapCell` from the game's pool. Caller fills
    /// the fields; the engine reclaims the cell on area change.
    pub fn new_automap_cell(&self, process: &ProcessHandle) -> Result<u32, String> {
        self.remote_calls_new_automap_cell
            .fetch_add(1, Ordering::Relaxed);
        let cell = remote_thread(process, self.inject_new_automap_cell, 0)?;
        Ok(cell)
    }

    /// Get a unit stat value
    pub fn get_unit_stat(
        &self,
        process: &ProcessHandle,
        p_unit: u32,
        stat_id: u32,
    ) -> Result<u32, String> {
        self.remote_calls_get_unit_stat
            .fetch_add(1, Ordering::Relaxed);
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

    /// Inject all helper functions into game memory. Identical on every OS
    /// — it only calls `ProcessHandle::write_buffer`, which is already
    /// OS-abstracted; the shellcode bytes themselves are plain x86 machine
    /// code with no Windows/Linux distinction.
    fn inject_functions(
        &self,
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
        d2_lang: usize,
    ) -> Result<(), String> {
        let inject_base = d2_client + d2client::INJECT_BASE;
        let string_addr = self.string_buffer.address as u32;
        let _params_addr = self.params_buffer.address as u32;

        // GetString injection (D2Lang_GetStringById)
        // From D2Stats.au3:2800-2806 — calls D2Lang.dll+0x9450 by absolute address.
        //   D2Client.dll+CDE10 - 8B CB                 - mov ecx,ebx        ; iNameID
        //   D2Client.dll+CDE12 - 31 C0                 - xor eax,eax
        //   D2Client.dll+CDE14 - BB *                  - mov ebx,D2Lang+0x9450
        //   D2Client.dll+CDE19 - FF D3                 - call ebx
        //   D2Client.dll+CDE1B - C3                    - ret
        let get_string_target = (d2_lang + d2lang::GET_STRING_BY_ID) as u32;
        let mut get_string_code: Vec<u8> = vec![0x8B, 0xCB, 0x31, 0xC0, 0xBB];
        get_string_code.extend_from_slice(&swap_endian(get_string_target));
        get_string_code.extend_from_slice(&[0xFF, 0xD3, 0xC3]);
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
        // D2Client.dll+CDE40 - 57                    - push edi
        // D2Client.dll+CDE41 - BF *                  - mov edi,D2Client.dll+CDEF0 (string addr)
        // D2Client.dll+CDE43 - 6A 00                 - push 00
        // D2Client.dll+CDE45 - 6A 01                 - push 01
        // D2Client.dll+CDE47 - 53                    - push ebx (pUnit)
        // D2Client.dll+CDE4B - E8 *                  - call D2Client.dll+560B0 (GetItemStat)
        // D2Client.dll+CDE50 - 5F                    - pop edi
        // D2Client.dll+CDE51 - C3                    - ret
        //
        // NOTE: The relative offset must match the original AutoIt injection:
        //   iIDWNTT = (D2Client+0x560B0) - (D2Client+0xCDE4E)
        // So we subtract (inject_base + GET_ITEM_STAT + 0x10), *not* +0x0E.
        let get_stat_offset = (d2_client + d2client::func::GET_ITEM_STAT) as i32
            - (inject_base + d2client::inject::GET_ITEM_STAT + 0x10) as i32;
        let mut get_stat_code: Vec<u8> = vec![0x57, 0xBF];
        get_stat_code.extend_from_slice(&swap_endian(string_addr));
        // push 0, push 1, push ebx (pUnit), call GetItemStatЫФ
        get_stat_code.extend_from_slice(&[0x6A, 0x00, 0x6A, 0x01, 0x53, 0xE8]);
        get_stat_code.extend_from_slice(&(get_stat_offset as u32).to_le_bytes());
        get_stat_code.extend_from_slice(&[0x5F, 0xC3]);
        process.write_buffer(self.inject_get_item_stat, &get_stat_code)?;

        // GetUnitStat injection
        // push 0; push [ebx]; push [ebx+4]; call rel32; mov [str], eax; ret
        // rel32 base is byte after the call's imm32 (offset 0x0C).
        let get_unit_stat_offset = (d2_common + d2common::GET_UNIT_STAT) as i32
            - (inject_base + d2common::INJECT_GET_UNIT_STAT + 0x0C) as i32;
        let mut get_unit_stat_code: Vec<u8> = vec![0x6A, 0x00, 0xFF, 0x33, 0xFF, 0x73, 0x04, 0xE8];
        get_unit_stat_code.extend_from_slice(&(get_unit_stat_offset as u32).to_le_bytes());
        get_unit_stat_code.push(0xA3);
        get_unit_stat_code.extend_from_slice(&swap_endian(string_addr));
        get_unit_stat_code.push(0xC3);
        process.write_buffer(self.inject_get_unit_stat, &get_unit_stat_code)?;

        // NewAutomapCell: `E8 [rel32]; C3` — call + ret. `__fastcall` with
        // no args, so no setup needed. EAX on return = AutomapCell*.
        let new_cell_offset = (d2_client + d2client::func::NEW_AUTOMAP_CELL) as i32
            - (inject_base + d2client::inject::NEW_AUTOMAP_CELL + 5) as i32;
        let mut new_cell_code: Vec<u8> = vec![0xE8];
        new_cell_code.extend_from_slice(&(new_cell_offset as u32).to_le_bytes());
        new_cell_code.push(0xC3);
        process.write_buffer(self.inject_new_automap_cell, &new_cell_code)?;

        Ok(())
    }
}

// SAFETY: Send is sound because all state lives in remote process memory;
// Sync is sound only because callers wrap in `Arc<Mutex<D2Injector>>` —
// `string_buffer`/`params_buffer` are a shared scratch arena and concurrent
// `&D2Injector` calls would corrupt each other. See `scanner_state.rs`.
#[cfg(target_os = "windows")]
unsafe impl Send for D2Injector {}
#[cfg(target_os = "windows")]
unsafe impl Sync for D2Injector {}

// --- Linux Implementation ---
//
// `string_buffer`/`params_buffer` need a real allocation the same way
// `VirtualAllocEx` provides one on Windows — scavenging `D2Client.dll`'s
// padding turned out to be unsafe past a couple hundred bytes (empirically,
// only `INJECT_BASE+0x0..0x200` is genuinely free; further offsets contain
// real data). Linux has no direct `VirtualAllocEx` equivalent reachable
// without code already running in the target, so we get one the way any
// native Linux injector would: write a tiny hand-built `mmap2` syscall stub
// into the confirmed-free padding and run it once via `remote_thread`,
// exactly like the four permanent shellcode stubs below.
#[cfg(target_os = "linux")]
pub struct RemoteAlloc {
    pub address: usize,
}

#[cfg(target_os = "linux")]
pub struct D2Injector {
    pub string_buffer: RemoteAlloc,
    pub params_buffer: RemoteAlloc,
    pub cell_buffer: PublishedRemoteBuffer,
    pub inject_get_string: usize,
    pub inject_get_item_name: usize,
    pub inject_get_item_stat: usize,
    pub inject_get_unit_stat: usize,
    pub inject_new_automap_cell: usize,
    pub remote_calls_get_string: AtomicU64,
    pub remote_calls_get_item_name: AtomicU64,
    pub remote_calls_get_item_stat: AtomicU64,
    pub remote_calls_get_unit_stat: AtomicU64,
    pub remote_calls_new_automap_cell: AtomicU64,
}

#[cfg(target_os = "linux")]
impl D2Injector {
    pub fn new(
        process: &ProcessHandle,
        d2_client: usize,
        d2_common: usize,
        d2_lang: usize,
    ) -> Result<Self, String> {
        let inject_base = d2_client + d2client::INJECT_BASE;

        // One 0x3000 (12 KiB) mapping covers all three buffers: string_buffer at
        // +0x0 (4096 bytes, matches the wchar[2048] stats buffer size),
        // params_buffer at +0x1000, and cell_buffer at +0x2000. See
        // `ProcessHandle::mmap_remote` for the mechanism.
        let mmap_stub_addr = inject_base + d2client::inject::LINUX_MMAP_STUB;
        let mapped = process.mmap_remote(mmap_stub_addr, 0x3000)?;

        let string_buffer = RemoteAlloc { address: mapped };
        let params_buffer = RemoteAlloc {
            address: mapped + 0x1000,
        };
        let cell_buffer = PublishedRemoteBuffer {
            address: mapped + 0x2000,
            size: 0x1000,
        };

        let inject_get_string = inject_base + d2client::inject::GET_STRING;
        let inject_get_item_name = inject_base + d2client::inject::GET_ITEM_NAME;
        let inject_get_item_stat = inject_base + d2client::inject::GET_ITEM_STAT;
        let inject_get_unit_stat = inject_base + d2common::INJECT_GET_UNIT_STAT;
        let inject_new_automap_cell = inject_base + d2client::inject::NEW_AUTOMAP_CELL;

        let injector = Self {
            string_buffer,
            params_buffer,
            cell_buffer,
            inject_get_string,
            inject_get_item_name,
            inject_get_item_stat,
            inject_get_unit_stat,
            inject_new_automap_cell,
            remote_calls_get_string: AtomicU64::new(0),
            remote_calls_get_item_name: AtomicU64::new(0),
            remote_calls_get_item_stat: AtomicU64::new(0),
            remote_calls_get_unit_stat: AtomicU64::new(0),
            remote_calls_new_automap_cell: AtomicU64::new(0),
        };

        injector.inject_functions(process, d2_client, d2_common, d2_lang)?;

        Ok(injector)
    }
}

// --- Stub for other OSes (compilation only) ---

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct RemoteAlloc {
    pub address: usize,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct PublishedRemoteBuffer {
    pub address: usize,
    pub size: usize,
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

    pub fn remote_call_stats(&self) -> RemoteCallStats {
        RemoteCallStats::default()
    }
}
