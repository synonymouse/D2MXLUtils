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
#[cfg(any(target_os = "windows", target_os = "linux"))]
const ALWAYS_SHOW_ITEMS_GETTER_PATTERN: &[Option<u8>] = &[
    Some(0xA1),
    None,
    None,
    None,
    None,
    Some(0x85),
    Some(0xC0),
    Some(0x75),
    None,
    Some(0x56),
    Some(0x68),
    Some(0xD0),
    Some(0x00),
    Some(0x00),
    Some(0x00),
    Some(0xE8),
    None,
    None,
    None,
    None,
    Some(0x8B),
    Some(0xF0),
];

/// `None` on no match, ambiguous match (>1 hit = signature too loose to trust),
/// or out-of-module decoded address.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn resolve_always_show_items_ptr_rva(
    process: &ProcessHandle,
    base: usize,
    size: usize,
) -> Option<usize> {
    let first =
        process.scan_pattern_wildcard(base, size, ALWAYS_SHOW_ITEMS_GETTER_PATTERN, base)?;
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
    /// `SizeOfImage` of `D2Sigma.dll`, or 0 if not loaded. Use as the upper
    /// bound for AOB scans over the module.
    pub d2_sigma_size: usize,
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
        let (d2_sigma, d2_sigma_size) = process.get_module_info("D2Sigma.dll").unwrap_or((0, 0));

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
            d2_sigma_size,
            always_show_items_ptr_rva,
        })
    }
}

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

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::ProcessHandle;
    use std::fs;
    use std::io::IoSliceMut;
    use std::os::unix::fs::FileExt;

    /// Window title the game uses (confirmed via `xprop`/`wmctrl` against a
    /// live session — the X11 `WM_CLASS` Wine assigns is not stable/unique
    /// ("steam_proton" was observed even for a non-Steam Wine build), so we
    /// match on the window title instead, same string used on Windows.
    pub const WINDOW_TITLE: &str = "Diablo II";

    /// Process-wide shared X11 connection. Every window-lookup/focus-check
    /// helper here — plus the hotkey pollers in `hotkeys.rs` and the
    /// click-through toggle in `main.rs` — used to open (and immediately
    /// drop) a brand new connection on every single call. Under concurrent
    /// load from ~5 hotkey-poll threads (each polling every ~30ms) plus
    /// the 250ms overlay sync tick, that's 100+ connection handshakes per
    /// second, which was enough to produce genuinely flaky reads (overlay
    /// visibility flapping, click-through state going stale) rather than
    /// just being wasteful. `RustConnection` is internally synchronized
    /// (see its own module docs) and explicitly designed to be shared
    /// across threads, so one connection for the whole process is both
    /// correct and far cheaper.
    pub fn x11_conn() -> Result<(&'static x11rb::rust_connection::RustConnection, usize), String> {
        static CONN: std::sync::OnceLock<
            Result<(x11rb::rust_connection::RustConnection, usize), String>,
        > = std::sync::OnceLock::new();
        match CONN.get_or_init(|| {
            x11rb::connect(None).map_err(|e| format!("X11 connection failed: {}", e))
        }) {
            Ok((conn, screen_num)) => Ok((conn, *screen_num)),
            Err(e) => Err(e.clone()),
        }
    }

    fn ptrace_hint(err: impl std::fmt::Display) -> String {
        format!(
            "{err} (if this is a permission error, run: sudo sysctl kernel.yama.ptrace_scope=0)"
        )
    }

    /// Find the real host PID of the window titled `title`, by walking
    /// `_NET_CLIENT_LIST` and reading `_NET_WM_PID` off the matching window.
    pub fn find_pid_by_window_title(title: &str) -> Result<u32, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_client_list = intern("_NET_CLIENT_LIST")?;
        let net_wm_pid = intern("_NET_WM_PID")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let utf8_string = intern("UTF8_STRING")?;

        let list_reply = conn
            .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
            .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

        let windows: Vec<u32> = list_reply
            .value32()
            .map(|it| it.collect())
            .unwrap_or_default();

        for win in windows {
            // Prefer the EWMH UTF-8 title; fall back to legacy WM_NAME.
            let name = conn
                .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                });

            if name.as_deref() != Some(title) {
                continue;
            }

            let pid_reply = conn
                .get_property(false, win, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
                .map_err(|e| format!("_NET_WM_PID request failed: {}", e))?
                .reply()
                .map_err(|e| format!("_NET_WM_PID reply failed: {}", e))?;

            let pid = pid_reply.value32().and_then(|mut pids| pids.next());
            if let Some(pid) = pid {
                return Ok(pid);
            }
        }

        Err(format!("Window titled '{}' not found", title))
    }

    /// Find the on-screen rectangle `(x, y, width, height)` of the window
    /// titled `title`, in absolute root-window coordinates. Used to anchor
    /// the notification overlay to the actual game window rather than a
    /// screen/monitor corner — the game is often windowed, not filling its
    /// monitor, so a monitor-corner anchor can end up a long way from the
    /// game window itself on an unusual multi-monitor layout.
    pub fn find_window_rect_by_title(title: &str) -> Result<(i32, i32, u32, u32), String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_client_list = intern("_NET_CLIENT_LIST")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let utf8_string = intern("UTF8_STRING")?;

        let list_reply = conn
            .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
            .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

        let windows: Vec<u32> = list_reply
            .value32()
            .map(|it| it.collect())
            .unwrap_or_default();

        for win in windows {
            let name = conn
                .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                });

            if name.as_deref() != Some(title) {
                continue;
            }

            let geom = conn
                .get_geometry(win)
                .map_err(|e| format!("get_geometry failed: {}", e))?
                .reply()
                .map_err(|e| format!("get_geometry reply failed: {}", e))?;

            // `get_geometry` gives size + position relative to the window's
            // immediate parent (often a WM-added decoration frame, not the
            // root) — translate (0, 0) into root-relative coordinates to
            // get the window's true on-screen position.
            let translated = conn
                .translate_coordinates(win, root, 0, 0)
                .map_err(|e| format!("translate_coordinates failed: {}", e))?
                .reply()
                .map_err(|e| format!("translate_coordinates reply failed: {}", e))?;

            return Ok((
                translated.dst_x as i32,
                translated.dst_y as i32,
                geom.width as u32,
                geom.height as u32,
            ));
        }

        Err(format!("Window titled '{}' not found", title))
    }

    /// Ask the window manager to activate (raise + focus) the window
    /// titled `title`, via the standard EWMH `_NET_ACTIVE_WINDOW`
    /// client-message request (the same mechanism `wmctrl -a` uses) rather
    /// than an `XSetInputFocus` call directly — going through the WM keeps
    /// its own stacking/focus bookkeeping consistent, which a raw focus
    /// call can desync from. Used to hand keyboard focus explicitly back
    /// to the D2 window when an overlay panel (edit mode / loot history /
    /// item search) closes, instead of relying on the WM's implicit
    /// behavior when the overlay unmaps — that implicit behavior isn't
    /// guaranteed under every focus policy and was the root cause of focus
    /// visibly "swapping" between the overlay and the game after closing
    /// a panel.
    pub fn activate_window_by_title(title: &str) -> Result<(), String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{
            AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt, EventMask,
        };

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_client_list = intern("_NET_CLIENT_LIST")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let utf8_string = intern("UTF8_STRING")?;
        let net_active_window = intern("_NET_ACTIVE_WINDOW")?;

        let list_reply = conn
            .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
            .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

        let windows: Vec<u32> = list_reply
            .value32()
            .map(|it| it.collect())
            .unwrap_or_default();

        for win in windows {
            let name = conn
                .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                        .ok()
                        .and_then(|c| c.reply().ok())
                        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                });

            if name.as_deref() != Some(title) {
                continue;
            }

            // source indication = 2 ("pager/other utility"), not 1
            // ("application activating its own window"). We're a separate
            // process handing focus to a *different* application's window
            // on the user's behalf — the same role a taskbar/pager plays,
            // not D2 reclaiming itself. This matters in practice, not just
            // semantically: KWin (confirmed live) applies real
            // focus-stealing-prevention scrutiny to source=1 requests —
            // weighed against the requesting app's own recent-user-input
            // timestamp — which this process has none of, since the user
            // never actually interacts with it directly. That scrutiny is
            // lenient with no competing focus history (works right after
            // launch) but starts silently ignoring the request once real
            // focus history exists (e.g. after alt-tabbing away and back),
            // which reproduced as an unrecoverable focus-flip loop: our
            // "give focus back to D2" request gets dropped, the overlay
            // (which the WM did auto-focus on map) reads as focused, we
            // hide it, focus reverts to D2, next tick shows the overlay
            // again, repeat — until a real user click legitimizes a
            // request. source=2 is the pager path, which WMs are expected
            // to honor without that scrutiny — timestamp = 0 (unknown) is
            // normal for it since pagers don't have a "last user event" of
            // their own; requestor's currently-active window = 0
            // (unknown/not tracked here).
            let event = ClientMessageEvent::new(
                32,
                win,
                net_active_window,
                ClientMessageData::from([2u32, 0, 0, 0, 0]),
            );
            conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| format!("send_event(_NET_ACTIVE_WINDOW) failed: {}", e))?;
            conn.flush()
                .map_err(|e| format!("X11 flush failed: {}", e))?;
            return Ok(());
        }

        Err(format!("Window titled '{}' not found", title))
    }

    /// Same as `activate_window_by_title`, but re-issues the request a few
    /// times (with a short sleep) until `is_window_focused_by_title`
    /// confirms it actually landed, instead of firing once and hoping.
    ///
    /// The single-shot version was found to still lose the focus-flicker
    /// race in practice: `overlay.show()` returning doesn't mean KWin has
    /// finished mapping/auto-focusing the overlay yet, so an activate call
    /// placed immediately after can land *before* the WM's own map-time
    /// focus grab — D2 flashes active, then the overlay steals it right
    /// back a moment later, and the next poll tick sees D2 unfocused again.
    /// Retrying for a short window absorbs that ordering race without
    /// switching the whole sync loop to be event-driven.
    pub fn activate_window_by_title_confirmed(title: &str) -> Result<(), String> {
        const MAX_ATTEMPTS: u32 = 6;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(15);

        let mut last_err = None;
        for attempt in 0..MAX_ATTEMPTS {
            match activate_window_by_title(title) {
                Ok(()) => {
                    std::thread::sleep(RETRY_DELAY);
                    if is_window_focused_by_title(title).unwrap_or(false) {
                        return Ok(());
                    }
                }
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(RETRY_DELAY);
                }
            }
            let _ = attempt;
        }
        Err(last_err.unwrap_or_else(|| {
            format!(
                "activate_window_by_title_confirmed: '{}' never confirmed focused after {} attempts",
                title, MAX_ATTEMPTS
            )
        }))
    }

    /// Whether the window titled `title` is currently the active/focused
    /// window, via `_NET_ACTIVE_WINDOW` on the root window. Used to hide
    /// the overlay when the user has switched away from the game, rather
    /// than leaving a stale notification toast drawn over whatever else
    /// they're looking at.
    pub fn is_window_focused_by_title(title: &str) -> Result<bool, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let utf8_string = intern("UTF8_STRING")?;

        let active_reply = conn
            .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

        let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
            return Ok(false);
        };
        if active == 0 {
            return Ok(false);
        }

        let name = conn
            .get_property(false, active, net_wm_name, utf8_string, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                conn.get_property(false, active, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            });

        Ok(name.as_deref() == Some(title))
    }

    /// Whether the currently active/focused window is either the window
    /// titled `title` (the game) or one of our own app's windows (matched
    /// by `_NET_WM_PID` against our own PID — this process draws several
    /// top-level webviews, e.g. the overlay and any open item-search/loot-
    /// history popovers, and hotkeys like edit-mode/loot-history should
    /// still fire while one of those has focus, not just the game window).
    pub fn is_d2_or_own_window_focused(title: &str) -> Result<bool, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
        let net_wm_pid = intern("_NET_WM_PID")?;
        let net_wm_name = intern("_NET_WM_NAME")?;
        let utf8_string = intern("UTF8_STRING")?;

        let active_reply = conn
            .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

        let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
            return Ok(false);
        };
        if active == 0 {
            return Ok(false);
        }

        let pid = conn
            .get_property(false, active, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut p| p.next()));
        if pid == Some(std::process::id()) {
            return Ok(true);
        }

        let name = conn
            .get_property(false, active, net_wm_name, utf8_string, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                conn.get_property(false, active, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            });

        Ok(name.as_deref() == Some(title))
    }

    /// Whether the currently active/focused window belongs to our own
    /// process (matched by `_NET_WM_PID`). Used to keep the overlay window
    /// alive while it holds keyboard focus itself — e.g. while the user is
    /// typing into the item-search box — rather than the game-focus poll
    /// hiding it out from under them the moment focus leaves D2.
    pub fn is_own_window_focused() -> Result<bool, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

        let (conn, screen_num) = x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        let intern = |name: &str| -> Result<u32, String> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
                .reply()
                .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
                .atom)
        };

        let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
        let net_wm_pid = intern("_NET_WM_PID")?;

        let active_reply = conn
            .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

        let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
            return Ok(false);
        };
        if active == 0 {
            return Ok(false);
        }

        let pid = conn
            .get_property(false, active, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut p| p.next()));

        Ok(pid == Some(std::process::id()))
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
            // exactly where the shellcode stubs in `injection.rs` get written.
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
        pub fn resolve_export(
            &self,
            module_base: usize,
            export_name: &str,
        ) -> Result<usize, String> {
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
            let addr_of_functions =
                module_base + self.read_memory::<u32>(export_dir + 0x1C)? as usize;
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
                    let func_rva =
                        self.read_memory::<u32>(addr_of_functions + ordinal * 4)? as usize;
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
        /// technique in `injection.rs`'s `D2Injector::new`, which this
        /// generalizes (parameterized by size via the incoming `EBX`
        /// param `call_remote` already delivers, rather than a hardcoded
        /// immediate) so `dps_hook` can reuse it for its own region.
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
}

#[cfg(target_os = "linux")]
pub use linux_impl::activate_window_by_title as linux_activate_window_by_title;
#[cfg(target_os = "linux")]
pub use linux_impl::activate_window_by_title_confirmed as linux_activate_window_by_title_confirmed;
#[cfg(target_os = "linux")]
pub use linux_impl::find_window_rect_by_title as linux_find_window_rect_by_title;
#[cfg(target_os = "linux")]
pub use linux_impl::is_d2_or_own_window_focused as linux_is_d2_or_own_window_focused;
#[cfg(target_os = "linux")]
pub use linux_impl::is_own_window_focused as linux_is_own_window_focused;
#[cfg(target_os = "linux")]
pub use linux_impl::is_window_focused_by_title as linux_is_window_focused_by_title;
#[cfg(target_os = "linux")]
pub use linux_impl::x11_conn as linux_x11_conn;
#[cfg(target_os = "linux")]
pub use linux_impl::WINDOW_TITLE as LINUX_WINDOW_TITLE;

#[cfg(target_os = "linux")]
pub struct ProcessHandle {
    pub pid: u32,
}

// SAFETY: all access goes through pid-addressed syscalls
// (process_vm_readv/writev, ptrace) with no thread-affine kernel object —
// safe to use from any thread.
#[cfg(target_os = "linux")]
unsafe impl Send for ProcessHandle {}
#[cfg(target_os = "linux")]
unsafe impl Sync for ProcessHandle {}

#[cfg(target_os = "linux")]
pub fn open_process_by_window_class(class_name: &str) -> Result<ProcessHandle, String> {
    let pid = linux_impl::find_pid_by_window_title(class_name)?;
    Ok(ProcessHandle { pid })
}

#[cfg(target_os = "linux")]
pub struct D2Context {
    pub process: ProcessHandle,
    pub d2_client: usize,
    pub d2_common: usize,
    pub d2_win: usize,
    pub d2_lang: usize,
    pub d2_sigma: usize,
    pub d2_sigma_size: usize,
    pub always_show_items_ptr_rva: Option<usize>,
}

#[cfg(target_os = "linux")]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        let process = open_process_by_window_class(linux_impl::WINDOW_TITLE)?;
        let d2_client = process.get_module_base("D2Client.dll")?;
        let d2_common = process.get_module_base("D2Common.dll")?;
        let d2_win = process.get_module_base("D2Win.dll")?;
        let d2_lang = process.get_module_base("D2Lang.dll")?;
        let (d2_sigma, d2_sigma_size) = process.get_module_info("D2Sigma.dll").unwrap_or((0, 0));

        let always_show_items_ptr_rva = if d2_sigma != 0 && d2_sigma_size != 0 {
            resolve_always_show_items_ptr_rva(&process, d2_sigma, d2_sigma_size)
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
            d2_sigma_size,
            always_show_items_ptr_rva,
        })
    }
}

// --- Linux ptrace: PTRACE_POKEDATA write fallback + remote-call primitive ---
// (remote-call/register plumbing for `injection.rs` lives here so it can
// share the 32-bit register-layout handling with the write fallback.)
#[cfg(target_os = "linux")]
pub(crate) mod linux_ptrace {
    use nix::sys::ptrace;
    use nix::sys::signal::Signal;
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::Pid;
    use std::mem;

    fn attach_hint(e: impl std::fmt::Display) -> String {
        format!("PTRACE_ATTACH failed: {e} (try: sudo sysctl kernel.yama.ptrace_scope=0)")
    }

    fn attach_and_wait(tid: Pid) -> Result<(), String> {
        ptrace::attach(tid).map_err(attach_hint)?;
        match waitpid(tid, None) {
            Ok(WaitStatus::Stopped(_, _)) => Ok(()),
            other => {
                let _ = ptrace::detach(tid, None);
                Err(format!("unexpected wait status after attach: {:?}", other))
            }
        }
    }

    /// Every thread id currently in the process, via `/proc/<pid>/task/`.
    /// Falls back to `[pid]` if the listing can't be read (e.g. the process
    /// just exited) — `call_remote` treats a failed attach on any candidate
    /// as "try the next one", so an empty/stale list just degrades to the
    /// single-thread behavior rather than erroring outright.
    fn list_thread_ids(pid: u32) -> Vec<i32> {
        let tids: Vec<i32> = std::fs::read_dir(format!("/proc/{}/task", pid))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
            .collect();
        if tids.is_empty() {
            vec![pid as i32]
        } else {
            tids
        }
    }

    /// `process_vm_writev` — not wrapped by `nix`, so a raw syscall.
    pub fn process_vm_writev(pid: u32, address: usize, buffer: &[u8]) -> Result<(), String> {
        #[repr(C)]
        struct IoVec {
            iov_base: *const u8,
            iov_len: usize,
        }
        let local = IoVec {
            iov_base: buffer.as_ptr(),
            iov_len: buffer.len(),
        };
        let remote = IoVec {
            iov_base: address as *const u8,
            iov_len: buffer.len(),
        };
        let n = unsafe {
            libc::syscall(
                libc::SYS_process_vm_writev,
                pid as libc::pid_t,
                &local as *const IoVec,
                1usize,
                &remote as *const IoVec,
                1usize,
                0usize,
            )
        };
        if n == buffer.len() as i64 {
            Ok(())
        } else {
            Err(format!(
                "process_vm_writev failed (ret={}, errno={})",
                n,
                std::io::Error::last_os_error()
            ))
        }
    }

    /// Word-at-a-time `PTRACE_POKEDATA` write, assuming `tid` is already
    /// ptrace-attached and stopped. Bypasses page protection the same way
    /// debuggers plant `0xCC` breakpoints into `.text` — needed for writing
    /// the shellcode stubs into `D2Client.dll`'s mapped image.
    fn poke_write_attached(tid: Pid, address: usize, buffer: &[u8]) -> Result<(), String> {
        let word_size = mem::size_of::<usize>();
        let mut offset = 0usize;
        while offset < buffer.len() {
            let word_addr = address + offset;
            // Read-modify-write so a write shorter than a full word doesn't
            // clobber trailing bytes we didn't intend to touch.
            let existing = ptrace::read(tid, word_addr as *mut _)
                .map_err(|e| format!("PTRACE_PEEKDATA failed: {}", e))?;
            let mut word_bytes = existing.to_ne_bytes();
            let remaining = buffer.len() - offset;
            let take = remaining.min(word_size);
            word_bytes[..take].copy_from_slice(&buffer[offset..offset + take]);
            let new_word = i64::from_ne_bytes(word_bytes);
            unsafe {
                ptrace::write(tid, word_addr as *mut _, new_word)
                    .map_err(|e| format!("PTRACE_POKEDATA failed: {}", e))?;
            }
            offset += take;
        }
        Ok(())
    }

    pub fn poke_write(pid: u32, address: usize, buffer: &[u8]) -> Result<(), String> {
        let tid = Pid::from_raw(pid as i32);
        attach_and_wait(tid)?;
        let result = poke_write_attached(tid, address, buffer);
        let _ = ptrace::detach(tid, None);
        result
    }

    /// The game process is a genuine **64-bit** ELF (confirmed empirically:
    /// raw `PTRACE_GETREGS` returns the full 216-byte/27-field native
    /// `libc::user_regs_struct`, not a 32-bit-compat 68-byte one) — Wine
    /// runs the 32-bit Windows code by switching the CPU into legacy
    /// compatibility mode (`cs = 0x23`, `ss/ds/es = 0x2b`: the classic Linux
    /// `__USER32_CS`/`__USER32_DS` selectors) within that same 64-bit task,
    /// rather than via the kernel's ia32-compat *task* mode. So `nix`'s
    /// typed `getregs`/`setregs` (host-native x86_64 layout) are exactly
    /// right here — the earlier hand-rolled 32-bit struct was wrong for
    /// this specific Wine build. `rip`/`rsp`/`rbx`/`rax` only need their low
    /// 32 bits touched (zero-extended); `cs`/`ss`/`ds`/`es` must be left
    /// completely untouched so the CPU stays in 32-bit compat mode for our
    /// injected call.
    fn getregs(tid: Pid) -> Result<libc::user_regs_struct, String> {
        ptrace::getregs(tid).map_err(|e| format!("PTRACE_GETREGS failed: {}", e))
    }

    fn setregs(tid: Pid, regs: &libc::user_regs_struct) -> Result<(), String> {
        ptrace::setregs(tid, *regs).map_err(|e| format!("PTRACE_SETREGS failed: {}", e))
    }

    /// Retries this many times to catch the target thread actually
    /// executing userspace code rather than blocked mid-syscall (see
    /// `call_remote`'s doc comment) before giving up.
    const ATTACH_RETRY_LIMIT: u32 = 100;

    /// Linux analog of `CreateRemoteThread(func_addr, param)` +
    /// `WaitForSingleObject(INFINITE)` + `GetExitCodeThread`. `ptrace` has
    /// no "spawn a thread" primitive, so this hijacks the target's own
    /// thread execution context instead of creating a new one: freeze it,
    /// redirect `EIP`=`func_addr`/`EBX`=`param` (matches the existing
    /// shellcode's calling convention) with `ESP` backed onto the thread's
    /// *own already-mapped* stack (offset down from its live value — no
    /// separate scratch allocation needed), and push a fake return address
    /// pointing at an unmapped page. Every existing shellcode stub already
    /// ends in a bare `ret`, so it pops that address and immediately
    /// SIGSEGVs — our signal that the call finished, with `EAX` already
    /// holding the return value the callee set before `ret`. Restore the
    /// original registers exactly and detach, even on an error path.
    ///
    /// Empirically (see `process::live_probe::minimal_int3_hijack`),
    /// `PTRACE_ATTACH` can catch a thread mid-syscall (deep in libc/vdso,
    /// e.g. blocked in `futex`/`poll`) — forcing a new `rip` in that state
    /// does not behave like a clean userspace jump (the thread faults a
    /// couple of bytes off from wherever the kernel's syscall-return path
    /// was really headed, not at our intended address at all). So after
    /// attaching we require `rip` to already be within shouting distance of
    /// `func_addr` (i.e. actually executing D2Client.dll code, not kernel/
    /// libc internals) before touching any registers, retrying the
    /// attach/detach cycle otherwise.
    pub fn call_remote(pid: u32, func_addr: usize, param: usize) -> Result<u32, String> {
        // Cycle through every thread in the process each round instead of
        // sleep-retrying a single fixed tid: the game (~25 threads in
        // practice) almost always has *some* thread actively running
        // userspace code at any instant, even when any one particular
        // thread is off blocked in a syscall. Trying them all before
        // sleeping converges far faster than waiting for one specific
        // thread's turn to come back around.
        let tids = list_thread_ids(pid);

        let (tid, orig_regs) = {
            let mut found = None;
            'rounds: for _ in 0..ATTACH_RETRY_LIMIT {
                for &raw_tid in &tids {
                    let candidate = Pid::from_raw(raw_tid);
                    if attach_and_wait(candidate).is_err() {
                        continue; // thread may have exited; try the next one
                    }
                    let r = match getregs(candidate) {
                        Ok(r) => r,
                        Err(_) => {
                            let _ = ptrace::detach(candidate, None);
                            continue;
                        }
                    };
                    if (r.rip as usize).abs_diff(func_addr) < 0x0200_0000 {
                        found = Some((candidate, r));
                        break 'rounds;
                    }
                    let _ = ptrace::detach(candidate, None);
                }
                // A full pass over every thread came up empty — give them a
                // moment to make progress before trying again.
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            found
                .ok_or_else(|| "gave up waiting for target thread to leave a syscall".to_string())?
        };

        let result = (|| -> Result<u32, String> {
            let mut call_regs = orig_regs;
            call_regs.rip = func_addr as u64;
            call_regs.rbx = param as u64;

            // Fake return address: page 0 is never mapped, so landing here
            // faults immediately rather than executing garbage.
            const FAKE_RETURN: u32 = 0x1;
            let new_esp = (orig_regs.rsp as u32).wrapping_sub(0x400) & !0xF;
            call_regs.rsp = new_esp as u64;

            poke_write_attached(tid, new_esp as usize, &FAKE_RETURN.to_le_bytes())?;
            setregs(tid, &call_regs)?;
            ptrace::cont(tid, None).map_err(|e| format!("PTRACE_CONT failed: {}", e))?;

            match waitpid(tid, None) {
                Ok(WaitStatus::Stopped(_, sig))
                    if sig == Signal::SIGSEGV || sig == Signal::SIGTRAP =>
                {
                    let result_regs = getregs(tid)?;
                    Ok(result_regs.rax as u32)
                }
                other => Err(format!(
                    "unexpected wait status after remote call: {:?}",
                    other
                )),
            }
        })();

        // Restore exactly, unconditionally — even if the call above failed
        // partway through (e.g. timed out mid-flight).
        let _ = setregs(tid, &orig_regs);
        let _ = ptrace::detach(tid, None);

        result
    }
}

// --- Stub for other OSes (compilation only) ---

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct D2Context {
    pub d2_client: usize,
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl D2Context {
    pub fn new() -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
}

// --- Live probe (manual verification only) ---
//
// `#[ignore]`d so `cargo test` never runs these in CI — they need an
// actually-running game and touch its process memory. Run explicitly with:
//   cargo test --target x86_64-unknown-linux-gnu -- --ignored --nocapture live_probe
#[cfg(all(test, target_os = "linux"))]
mod live_probe {
    use super::*;

    /// Scan every readable region of the live process for `needle`, both
    /// as raw ASCII and as UTF-16LE (D2's UI text fields can be either
    /// depending on the control), printing each hit with surrounding
    /// context bytes. One-off RE tool: run with
    /// `SCAN_NEEDLE=<marker> cargo test --target x86_64-unknown-linux-gnu \
    ///   scan_memory_for_string -- --ignored --nocapture`
    /// after typing `<marker>` into a live UI text field, to locate its
    /// backing buffer address without guessing at struct layouts.
    #[test]
    #[ignore]
    fn scan_memory_for_string() {
        let needle_str =
            std::env::var("SCAN_NEEDLE").expect("set SCAN_NEEDLE=<marker text> before running");
        let ascii_needle = needle_str.as_bytes().to_vec();
        let utf16_needle: Vec<u8> = needle_str
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();

        let ctx = D2Context::new().expect("attach failed");
        let pid = ctx.process.pid;

        let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid))
            .expect("reading /proc/<pid>/maps failed");

        let mut total_hits = 0usize;
        for line in maps.lines() {
            let mut parts = line.split_whitespace();
            let Some(range) = parts.next() else { continue };
            let Some(perms) = parts.next() else { continue };
            if !perms.starts_with('r') {
                continue;
            }
            let Some((start_s, end_s)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start_s, 16),
                usize::from_str_radix(end_s, 16),
            ) else {
                continue;
            };
            let size = end.saturating_sub(start);
            // Skip absurdly large regions (e.g. reserved-but-unbacked
            // ranges) to keep this a quick manual tool, not a full dump.
            if size == 0 || size > 256 * 1024 * 1024 {
                continue;
            }

            let Ok(buf) = ctx.process.read_buffer(start, size) else {
                continue;
            };

            for (needle, kind) in [(&ascii_needle, "ascii"), (&utf16_needle, "utf16le")] {
                if needle.is_empty() {
                    continue;
                }
                let mut offset = 0usize;
                while let Some(pos) = buf[offset..]
                    .windows(needle.len())
                    .position(|w| w == needle.as_slice())
                {
                    let addr = start + offset + pos;
                    total_hits += 1;
                    let ctx_start = (offset + pos).saturating_sub(32);
                    let ctx_end = (offset + pos + needle.len() + 32).min(buf.len());
                    println!(
                        "[{}] hit @ {:#x} (region {}-{} perms={})",
                        kind, addr, range, perms, perms
                    );
                    println!("  bytes: {:02x?}", &buf[ctx_start..ctx_end]);
                    println!(
                        "  ascii: {:?}",
                        String::from_utf8_lossy(&buf[ctx_start..ctx_end])
                    );
                    offset += pos + 1;
                    if offset >= buf.len() {
                        break;
                    }
                }
            }
        }
        println!("total hits: {}", total_hits);
    }

    /// Dump a window of memory around a known-good hit address (found via
    /// `scan_memory_for_string`) to visually inspect the surrounding
    /// struct layout — e.g. sibling UI control fields for Password /
    /// Description next to a confirmed Game Name buffer. Run with
    /// `SCAN_ADDR=0x... [SCAN_BEFORE=0x400] [SCAN_AFTER=0x400] cargo test \
    ///   --target x86_64-unknown-linux-gnu dump_region -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_region() {
        let addr = usize::from_str_radix(
            std::env::var("SCAN_ADDR")
                .expect("set SCAN_ADDR=0x... before running")
                .trim_start_matches("0x"),
            16,
        )
        .expect("SCAN_ADDR must be hex");
        let before: usize = std::env::var("SCAN_BEFORE")
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x400);
        let after: usize = std::env::var("SCAN_AFTER")
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x400);

        let ctx = D2Context::new().expect("attach failed");
        let start = addr.saturating_sub(before);
        let size = before + after;
        let buf = ctx
            .process
            .read_buffer(start, size)
            .expect("read_buffer failed");

        for (i, chunk) in buf.chunks(16).enumerate() {
            let line_addr = start + i * 16;
            let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            let marker = if line_addr <= addr && addr < line_addr + 16 {
                " <=="
            } else {
                ""
            };
            println!(
                "{:#010x}: {:<48} {}{}",
                line_addr,
                hex.join(" "),
                ascii,
                marker
            );
        }
    }

    /// Search every readable region for 4-byte little-endian pointer
    /// values equal to `SCAN_PTR` (a target address found via
    /// `scan_memory_for_string`) — i.e. "what points at this address".
    /// Used to trace a transient heap buffer back to whatever stable
    /// parent structure holds a pointer to it.
    #[test]
    #[ignore]
    fn scan_memory_for_pointer() {
        let target = usize::from_str_radix(
            std::env::var("SCAN_PTR")
                .expect("set SCAN_PTR=0x... before running")
                .trim_start_matches("0x"),
            16,
        )
        .expect("SCAN_PTR must be hex") as u32;
        let needle = target.to_le_bytes();

        let ctx = D2Context::new().expect("attach failed");
        let pid = ctx.process.pid;
        let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid))
            .expect("reading /proc/<pid>/maps failed");

        let mut total_hits = 0usize;
        for line in maps.lines() {
            let mut parts = line.split_whitespace();
            let Some(range) = parts.next() else { continue };
            let Some(perms) = parts.next() else { continue };
            if !perms.starts_with('r') {
                continue;
            }
            let Some((start_s, end_s)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start_s, 16),
                usize::from_str_radix(end_s, 16),
            ) else {
                continue;
            };
            let size = end.saturating_sub(start);
            if size == 0 || size > 256 * 1024 * 1024 {
                continue;
            }
            let Ok(buf) = ctx.process.read_buffer(start, size) else {
                continue;
            };

            let mut offset = 0usize;
            while offset + 4 <= buf.len() {
                if buf[offset..offset + 4] == needle {
                    let addr = start + offset;
                    total_hits += 1;
                    println!("hit @ {:#x} (region {} perms={})", addr, range, perms);
                }
                offset += 4;
            }
        }
        println!("total hits: {}", total_hits);
    }

    #[test]
    #[ignore]
    fn attach_and_resolve_modules() {
        let ctx = D2Context::new().expect("attach failed");
        println!("pid = {}", ctx.process.pid);
        println!("d2_client = {:#x}", ctx.d2_client);
        println!("d2_common = {:#x}", ctx.d2_common);
        println!("d2_win = {:#x}", ctx.d2_win);
        println!("d2_lang = {:#x}", ctx.d2_lang);
        println!(
            "d2_sigma = {:#x} (size {:#x})",
            ctx.d2_sigma, ctx.d2_sigma_size
        );
        assert_ne!(ctx.d2_client, 0);
        assert_ne!(ctx.d2_common, 0);
        assert_ne!(ctx.d2_win, 0);
        assert_ne!(ctx.d2_lang, 0);
    }

    #[test]
    #[ignore]
    fn read_player_unit_pointer() {
        use crate::offsets::d2client;

        let ctx = D2Context::new().expect("attach failed");
        let player_unit_ptr = ctx
            .process
            .read_memory::<u32>(ctx.d2_client + d2client::PLAYER_UNIT)
            .expect("read failed");
        println!("PLAYER_UNIT ptr = {:#x}", player_unit_ptr);
        // Nonzero only while actually in-game; this just proves the read
        // pipeline (process_vm_readv against the real host PID) works.
    }

    #[test]
    #[ignore]
    fn find_free_padding_runs() {
        use crate::offsets::d2client;

        let ctx = D2Context::new().expect("attach failed");
        let inject_base = ctx.d2_client + d2client::INJECT_BASE;
        // Scan a generous window past the existing stubs (which top out
        // around +0x76) looking for long all-zero runs — candidates for
        // `string_buffer` (need ~0x1000 bytes) and `params_buffer` (~0x100).
        let dump = ctx
            .process
            .read_buffer(inject_base, 0x4000)
            .expect("read failed");

        let mut runs: Vec<(usize, usize)> = Vec::new(); // (start, len)
        let mut run_start: Option<usize> = None;
        for (i, &b) in dump.iter().enumerate() {
            if b == 0 {
                if run_start.is_none() {
                    run_start = Some(i);
                }
            } else if let Some(start) = run_start.take() {
                runs.push((start, i - start));
            }
        }
        if let Some(start) = run_start {
            runs.push((start, dump.len() - start));
        }

        runs.sort_by(|a, b| b.1.cmp(&a.1));
        println!("longest zero runs within INJECT_BASE+0x0..0x4000:");
        for (start, len) in runs.iter().take(10) {
            println!(
                "  +{:#x}, len {:#x} (ends at +{:#x})",
                start,
                len,
                start + len
            );
        }
    }

    /// End-to-end: mmap allocation + shellcode install + a ptrace-hijacked
    /// call, via `D2Injector::get_string` (pure read-only string-table
    /// lookup — no game-state mutation, the safest first real call target).
    #[test]
    #[ignore]
    fn injector_get_string_roundtrip() {
        let ctx = D2Context::new().expect("attach failed");
        let injector = crate::injection::D2Injector::new(
            &ctx.process,
            ctx.d2_client,
            ctx.d2_common,
            ctx.d2_lang,
        )
        .expect("D2Injector::new failed");
        println!("string_buffer @ {:#x}", injector.string_buffer.address);
        println!("params_buffer @ {:#x}", injector.params_buffer.address);

        for id in [1u16, 2, 100, 1000] {
            match injector.get_string(&ctx.process, id, 64) {
                Ok(s) => println!("get_string({id}) = {:?}", s),
                Err(e) => println!("get_string({id}) failed: {e}"),
            }
        }
    }
}
