// --- Linux Implementation ---
//
// The game runs under Wine/Proton, but Wine maps the guest's Windows virtual
// addresses 1:1 onto the host process's real address space — the "process" a
// Win32 debugger sees via ReadProcessMemory *is* the real Linux process, just
// accessed through Wine's own per-wineserver-session virtualization. We skip
// that virtualization entirely and talk to the host process directly:
//
// - Window/PID lookup goes through X11 (`_NET_WM_PID`), not `FindWindowW` —
//   X11 windows are visible on the whole display regardless of which
//   Wine/Proton prefix created them, unlike Win32 window handles which are
//   scoped per-wineserver.
// - Memory read/write goes through `process_vm_readv`/`process_vm_writev`
//   (the direct Linux syscalls `ReadProcessMemory`/`WriteProcessMemory` are
//   themselves implemented on top of, inside Wine).
//
// Both require the same permission Yama's `ptrace_scope` gates for
// `PTRACE_ATTACH` (see `man process_vm_readv`), since we are not a parent of
// the target process: `sudo sysctl kernel.yama.ptrace_scope=0`, or
// `sudo setcap cap_sys_ptrace+ep <binary>`.

use std::fs;
use std::io::IoSliceMut;
use std::os::unix::fs::FileExt;

pub struct ProcessHandle {
    pub pid: u32,
}

// SAFETY: all access goes through pid-addressed syscalls
// (process_vm_readv/writev, ptrace) with no thread-affine kernel object —
// safe to use from any thread.
unsafe impl Send for ProcessHandle {}
unsafe impl Sync for ProcessHandle {}

pub fn open_process_by_window_class(class_name: &str) -> Result<ProcessHandle, String> {
    let pid = super::linux_x11::find_pid_by_window_title(class_name)?;
    Ok(ProcessHandle { pid })
}

fn ptrace_hint(err: impl std::fmt::Display) -> String {
    format!("{err} (if this is a permission error, run: sudo sysctl kernel.yama.ptrace_scope=0)")
}

impl ProcessHandle {
    pub fn read_memory<T: Copy>(&self, address: usize) -> Result<T, String> {
        let mut buffer: T = unsafe { std::mem::zeroed() };
        let ptr = &mut buffer as *mut T as *mut u8;
        let size = std::mem::size_of::<T>();
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, size) };
        self.read_buffer_into(address, slice)?;
        Ok(buffer)
    }

    pub fn read_buffer(&self, address: usize, size: usize) -> Result<Vec<u8>, String> {
        let mut buffer = vec![0u8; size];
        self.read_buffer_into(address, &mut buffer)?;
        Ok(buffer)
    }

    pub fn read_buffer_into(&self, address: usize, buffer: &mut [u8]) -> Result<(), String> {
        use nix::sys::uio::{process_vm_readv, RemoteIoVec};
        use nix::unistd::Pid;

        let len = buffer.len();
        let mut local = [IoSliceMut::new(buffer)];
        let remote = [RemoteIoVec { base: address, len }];

        let n = process_vm_readv(Pid::from_raw(self.pid as i32), &mut local, &remote)
            .map_err(ptrace_hint)?;

        if n != buffer.len() {
            return Err("Incomplete read".to_string());
        }
        Ok(())
    }

    pub fn write_buffer(&self, address: usize, buffer: &[u8]) -> Result<(), String> {
        // `process_vm_writev` (unlike Win32 `WriteProcessMemory`) respects
        // real page protection and fails on read+execute-only pages —
        // exactly where the shellcode stubs in `injection/install.rs` get written.
        // Try the fast path first, fall back to `PTRACE_POKEDATA`, which
        // bypasses protection the same way debuggers plant breakpoints.
        if super::linux_ptrace::process_vm_writev(self.pid, address, buffer).is_ok() {
            return Ok(());
        }
        super::linux_ptrace::poke_write(self.pid, address, buffer)
    }

    pub fn get_module_base(&self, module_name: &str) -> Result<usize, String> {
        self.get_module_info(module_name).map(|(base, _)| base)
    }

    /// Resolve a module by name into `(base, SizeOfImage)` by parsing
    /// `/proc/<pid>/maps` for its file-backed mapping, then reading
    /// `SizeOfImage` straight out of the mapped PE header.
    pub fn get_module_info(&self, module_name: &str) -> Result<(usize, usize), String> {
        let maps = fs::read_to_string(format!("/proc/{}/maps", self.pid))
            .map_err(|e| format!("reading /proc/{}/maps failed: {}", self.pid, e))?;

        let base = maps
            .lines()
            .find_map(|line| {
                let path = line.split_once(' ').map(|_| line)?.rsplit(' ').next()?;
                if !path
                    .to_ascii_lowercase()
                    .ends_with(&module_name.to_ascii_lowercase())
                {
                    return None;
                }
                let range = line.split_whitespace().next()?;
                let start = range.split('-').next()?;
                usize::from_str_radix(start, 16).ok()
            })
            .ok_or_else(|| format!("Module '{}' not found", module_name))?;

        // PE header: e_lfanew at DOS header +0x3C, then
        // IMAGE_NT_HEADERS.OptionalHeader.SizeOfImage at a fixed offset
        // past the PE signature + COFF header (0x18 into OptionalHeader,
        // same for PE32 and PE32+).
        let e_lfanew = self.read_memory::<u32>(base + 0x3C)? as usize;
        let size_of_image = self.read_memory::<u32>(base + e_lfanew + 0x18 + 0x38)? as usize;

        Ok((base, size_of_image))
    }

    /// Resolve `export_name`'s absolute address in a module already
    /// mapped into this process, by walking its PE export directory.
    /// Linux analog of `GetProcAddress` — used to find e.g.
    /// `KERNEL32.dll!GetTickCount` for the DPS hook's trampoline,
    /// where (unlike `GetModuleHandleA`/`GetProcAddress` on Windows)
    /// there's no OS-provided shortcut for "this DLL's export table
    /// entry" since it's a *remote* process's module, not our own.
    pub fn resolve_export(&self, module_base: usize, export_name: &str) -> Result<usize, String> {
        let e_lfanew = self.read_memory::<u32>(module_base + 0x3C)? as usize;
        let opt_header = module_base + e_lfanew + 0x18;
        // DataDirectory[0] (Export Table) sits at +0x60 into the PE32
        // Optional Header (Magic..NumberOfRvaAndSizes = 0x60 bytes).
        let export_table_rva = self.read_memory::<u32>(opt_header + 0x60)? as usize;
        if export_table_rva == 0 {
            return Err(format!("module at {:#x} has no export table", module_base));
        }
        let export_dir = module_base + export_table_rva;

        let number_of_names = self.read_memory::<u32>(export_dir + 0x18)? as usize;
        let addr_of_functions = module_base + self.read_memory::<u32>(export_dir + 0x1C)? as usize;
        let addr_of_names = module_base + self.read_memory::<u32>(export_dir + 0x20)? as usize;
        let addr_of_name_ordinals =
            module_base + self.read_memory::<u32>(export_dir + 0x24)? as usize;

        for i in 0..number_of_names {
            let name_rva = self.read_memory::<u32>(addr_of_names + i * 4)? as usize;
            let name_addr = module_base + name_rva;
            let name_bytes = self.read_buffer(name_addr, export_name.len() + 1)?;
            if name_bytes.len() > export_name.len()
                && &name_bytes[..export_name.len()] == export_name.as_bytes()
                && name_bytes[export_name.len()] == 0
            {
                let ordinal = self.read_memory::<u16>(addr_of_name_ordinals + i * 2)? as usize;
                let func_rva = self.read_memory::<u32>(addr_of_functions + ordinal * 4)? as usize;
                return Ok(module_base + func_rva);
            }
        }

        Err(format!(
            "export '{}' not found in module at {:#x}",
            export_name, module_base
        ))
    }

    /// Allocate `size` bytes of RWX memory in the remote process via a
    /// hand-written `mmap2` syscall stub, staged at `stub_addr`
    /// (caller-provided scratch — must already be free, writable, and
    /// executable; see `offsets::d2client::inject::LINUX_MMAP_STUB`).
    /// There's no Linux equivalent of `VirtualAllocEx` reachable
    /// without code already running in the target, so we get one the
    /// way any native Linux injector would — see the identical
    /// technique in `injection/linux.rs`'s `D2Injector::new`, which this
    /// generalizes (parameterized by size via the incoming `EBX`
    /// param `call_remote` already delivers, rather than a hardcoded
    /// immediate) so `dps/hook/linux.rs` can reuse it for its own region.
    pub fn mmap_remote(&self, stub_addr: usize, size: usize) -> Result<usize, String> {
        #[rustfmt::skip]
        let mmap_stub: [u8; 32] = [
            0x89, 0xD9,                   // mov ecx, ebx      (length = incoming param)
            0x31, 0xDB,                   // xor ebx, ebx      (addr = NULL)
            0xB8, 0xC0, 0x00, 0x00, 0x00, // mov eax, 192 (mmap2)
            0xBA, 0x07, 0x00, 0x00, 0x00, // mov edx, 7        (PROT_READ|WRITE|EXEC)
            0xBE, 0x22, 0x00, 0x00, 0x00, // mov esi, 0x22     (MAP_PRIVATE|MAP_ANONYMOUS)
            0xBF, 0xFF, 0xFF, 0xFF, 0xFF, // mov edi, -1       (fd)
            0xBD, 0x00, 0x00, 0x00, 0x00, // mov ebp, 0        (pgoffset)
            0xCD, 0x80,                   // int 0x80
            0xC3,                         // ret
        ];
        self.write_buffer(stub_addr, &mmap_stub)?;
        let mapped = super::linux_ptrace::call_remote(self.pid, stub_addr, size)?;
        if (mapped as i32) < 0 {
            return Err(format!(
                "mmap2 in remote process failed (errno {})",
                -(mapped as i32)
            ));
        }
        Ok(mapped as usize)
    }

    pub fn scan_pattern(&self, start: usize, size: usize, pattern: &[u8]) -> Option<usize> {
        if pattern.is_empty() || size < pattern.len() {
            return None;
        }

        const CHUNK_SIZE: usize = 0x10000;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut offset = 0;

        while offset < size {
            let read_size = std::cmp::min(CHUNK_SIZE, size - offset);
            let addr = start + offset;

            if self
                .read_buffer_into(addr, &mut buffer[..read_size])
                .is_err()
            {
                offset += CHUNK_SIZE;
                continue;
            }

            let search_len = read_size.saturating_sub(pattern.len()) + 1;
            for i in 0..search_len {
                if &buffer[i..i + pattern.len()] == pattern {
                    return Some(addr + i);
                }
            }

            offset += read_size.saturating_sub(pattern.len()).max(1);
        }

        None
    }

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

            if self
                .read_buffer_into(addr, &mut buffer[..read_size])
                .is_err()
            {
                offset += CHUNK_SIZE;
                continue;
            }

            let search_len = read_size.saturating_sub(pattern.len()) + 1;
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

            offset += read_size.saturating_sub(pattern.len()).max(1);
        }

        None
    }
}

// Silence unused-import warning when FileExt ends up unused on some
// code paths (kept for a potential /proc/pid/mem fallback).
#[allow(unused)]
fn _keep_fileext_import(f: &fs::File) {
    let _ = f.metadata();
}
