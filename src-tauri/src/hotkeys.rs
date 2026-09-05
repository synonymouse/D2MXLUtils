//! Global hotkey management for D2MXLUtils
//!
//! Handles registration and handling of global hotkeys using Windows API.
//! Primarily used for showing/hiding the main window over the game.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::logger::{error as log_error, info as log_info};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
};

/// X11 key-state polling, replacing `GetAsyncKeyState`. All 6 hotkey
/// features already poll on a ~30ms timer rather than using a true event
/// loop (`RegisterHotKey`'s `WM_HOTKEY` can't deliver release events or
/// accept modifier-only chords, so even the one Windows mechanism that
/// *could* use a message loop shares the polling design) — so this keeps
/// the exact same architecture on Linux, just swapping the leaf
/// "is this key down right now" primitive from `GetAsyncKeyState` to
/// `XQueryKeymap` (system-wide, like `GetAsyncKeyState` — not scoped to
/// whichever window has focus, works the same via XWayland). One
/// difference worth knowing: `RegisterHotKey` also *consumes* the key
/// combo so the game/focused app never sees it; polling only observes,
/// so on Linux the key still reaches whatever's focused too. Unlikely to
/// matter for the combos this app defaults to (Ctrl+K, Alt+F, etc.).
#[cfg(target_os = "linux")]
mod x11_keys {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    // Windows VK_* code -> X11 keysym(s). Multiple entries (e.g. Ctrl) are
    // "either physical key counts" — VK_CONTROL doesn't distinguish L/R.
    pub const VK_SHIFT: u32 = 0x10;
    pub const VK_CONTROL: u32 = 0x11;
    pub const VK_MENU: u32 = 0x12; // Alt
    pub const VK_LWIN: u32 = 0x5B;
    pub const VK_RWIN: u32 = 0x5C;

    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_SHIFT_R: u32 = 0xffe2;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_CONTROL_R: u32 = 0xffe4;
    const XK_ALT_L: u32 = 0xffe9;
    const XK_ALT_R: u32 = 0xffea;
    const XK_SUPER_L: u32 = 0xffeb;
    const XK_SUPER_R: u32 = 0xffec;

    /// Maps a single-value VK code (a specific key, not a modifier pair)
    /// to its X11 keysym. Covers letters, digits, F-keys, and the common
    /// named keys this app's hotkey picker is likely to produce.
    fn vk_to_keysym(vk: u32) -> Option<u32> {
        match vk {
            0x30..=0x39 => Some(vk),        // '0'-'9' — same codepoint as ASCII/keysym
            0x41..=0x5A => Some(vk + 0x20), // 'A'-'Z' -> lowercase keysym (physical key, shift-independent)
            0x70..=0x7B => Some(0xffbe + (vk - 0x70)), // F1..F12 -> XK_F1..XK_F12
            0x08 => Some(0xff08),           // Backspace
            0x09 => Some(0xff09),           // Tab
            0x0D => Some(0xff0d),           // Return
            0x1B => Some(0xff1b),           // Escape
            0x20 => Some(0x0020),           // Space
            0x25 => Some(0xff51),           // Left
            0x26 => Some(0xff52),           // Up
            0x27 => Some(0xff53),           // Right
            0x28 => Some(0xff54),           // Down
            0x21 => Some(0xff55),           // PageUp
            0x22 => Some(0xff56),           // PageDown
            0x23 => Some(0xff57),           // End
            0x24 => Some(0xff50),           // Home
            0x2D => Some(0xff63),           // Insert
            0x2E => Some(0xffff),           // Delete
            _ => None,
        }
    }

    struct KeyboardMapping {
        min_kc: u8,
        max_kc: u8,
        per_kc: usize,
        keysyms: Vec<u32>,
    }

    /// The keycode->keysym mapping only changes when the user changes
    /// keyboard layout at the OS level, which doesn't happen mid-session
    /// — fetch it once instead of on every ~30ms poll. Combined with
    /// `x11_conn()` (shared connection, see its doc comment), a chord
    /// check is now a single `query_keymap` round-trip instead of two
    /// requests over a throwaway connection.
    fn keyboard_mapping() -> Option<&'static KeyboardMapping> {
        static MAPPING: std::sync::OnceLock<Option<KeyboardMapping>> = std::sync::OnceLock::new();
        MAPPING
            .get_or_init(|| {
                let (conn, _) = crate::process::linux_x11_conn().ok()?;
                let setup = conn.setup();
                let min_kc = setup.min_keycode;
                let max_kc = setup.max_keycode;
                if max_kc < min_kc {
                    return None;
                }
                let count = max_kc - min_kc + 1;
                let reply = conn
                    .get_keyboard_mapping(min_kc, count)
                    .ok()?
                    .reply()
                    .ok()?;
                let per_kc = reply.keysyms_per_keycode as usize;
                if per_kc == 0 {
                    return None;
                }
                Some(KeyboardMapping {
                    min_kc,
                    max_kc,
                    per_kc,
                    keysyms: reply.keysyms,
                })
            })
            .as_ref()
    }

    /// One `query_keymap` round-trip over the shared connection, checked
    /// against every keysym the chord needs.
    fn keys_down(keysyms: &[u32]) -> Vec<bool> {
        let mut result = vec![false; keysyms.len()];
        let Some(mapping) = keyboard_mapping() else {
            return result;
        };
        let Ok((conn, _)) = crate::process::linux_x11_conn() else {
            return result;
        };
        let Ok(km_cookie) = conn.query_keymap() else {
            return result;
        };
        let Ok(km) = km_cookie.reply() else {
            return result;
        };

        let is_kc_down = |kc: u8| {
            let byte = kc as usize / 8;
            let bit = kc as usize % 8;
            km.keys
                .get(byte)
                .map(|b| b & (1 << bit) != 0)
                .unwrap_or(false)
        };

        for (chunk_i, chunk) in mapping.keysyms.chunks(mapping.per_kc).enumerate() {
            let kc = mapping.min_kc as usize + chunk_i;
            if kc > mapping.max_kc as usize {
                break;
            }
            if !chunk.iter().any(|ks| keysyms.contains(ks)) {
                continue;
            }
            if is_kc_down(kc as u8) {
                for (i, &target) in keysyms.iter().enumerate() {
                    if chunk.contains(&target) {
                        result[i] = true;
                    }
                }
            }
        }
        result
    }

    pub fn is_key_down(vk: u32) -> bool {
        match vk_to_keysym(vk) {
            Some(ks) => keys_down(&[ks]).first().copied().unwrap_or(false),
            None => false,
        }
    }

    pub fn is_modifier_down(vk: u32) -> bool {
        let pair = match vk {
            VK_CONTROL => [XK_CONTROL_L, XK_CONTROL_R],
            VK_SHIFT => [XK_SHIFT_L, XK_SHIFT_R],
            VK_MENU => [XK_ALT_L, XK_ALT_R],
            VK_LWIN => [XK_SUPER_L, XK_SUPER_L],
            VK_RWIN => [XK_SUPER_R, XK_SUPER_R],
            _ => return false,
        };
        let down = keys_down(&pair);
        down.iter().any(|&b| b)
    }
}

#[cfg(target_os = "linux")]
fn is_d2_or_app_foreground_linux() -> bool {
    crate::process::linux_is_d2_or_own_window_focused(crate::process::LINUX_WINDOW_TITLE)
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_d2_foreground_linux() -> bool {
    crate::process::linux_is_window_focused_by_title(crate::process::LINUX_WINDOW_TITLE)
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn chord_keys_are_pressed_linux(hk: &HotkeyConfig) -> bool {
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;

    if hk.key_code == 0 && hk.modifiers == 0 {
        return false;
    }
    if is_mouse_hotkey_key(hk.key_code) {
        // No portable "is this mouse button held" primitive without a
        // global pointer-grab; unsupported on Linux for v1.
        return false;
    }

    if hk.modifiers & MOD_CONTROL != 0 && !x11_keys::is_modifier_down(x11_keys::VK_CONTROL) {
        return false;
    }
    if hk.modifiers & MOD_SHIFT != 0 && !x11_keys::is_modifier_down(x11_keys::VK_SHIFT) {
        return false;
    }
    if hk.modifiers & MOD_ALT != 0 && !x11_keys::is_modifier_down(x11_keys::VK_MENU) {
        return false;
    }
    if hk.modifiers & MOD_WIN != 0
        && !(x11_keys::is_modifier_down(x11_keys::VK_LWIN)
            || x11_keys::is_modifier_down(x11_keys::VK_RWIN))
    {
        return false;
    }

    if hk.key_code != 0 && !x11_keys::is_key_down(hk.key_code) {
        return false;
    }

    true
}

#[cfg(target_os = "linux")]
fn chord_is_pressed_linux(hk: &HotkeyConfig) -> bool {
    is_d2_or_app_foreground_linux() && chord_keys_are_pressed_linux(hk)
}

#[cfg(target_os = "linux")]
fn chord_is_pressed_d2_only_linux(hk: &HotkeyConfig) -> bool {
    is_d2_foreground_linux() && chord_keys_are_pressed_linux(hk)
}

/// Hotkey ID for toggle main window
const HOTKEY_ID_TOGGLE_MAIN: i32 = 1;

/// Hotkey configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    /// Virtual key code (e.g., 0x4B for 'K')
    pub key_code: u32,
    /// Modifier flags (Ctrl, Shift, Alt, Win)
    pub modifiers: u32,
    /// Human-readable representation (e.g., "Ctrl+K")
    pub display: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenItemSearchPayload {
    query: Option<String>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key_code: 0x4B,    // 'K' key
            modifiers: 0x0002, // MOD_CONTROL
            display: "Ctrl+K".to_string(),
        }
    }
}

fn is_mouse_hotkey_key(key_code: u32) -> bool {
    (0x04..=0x06).contains(&key_code)
}

/// Global state for hotkey management
pub struct HotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
}

impl HotkeyState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(HotkeyConfig::default())),
        }
    }

    /// Start the hotkey listener thread
    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Hotkey listener already running, restarting with new config");
            self.stop();
            // Give the thread time to stop
            thread::sleep(std::time::Duration::from_millis(100));
        }

        // Update current hotkey
        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                hotkey_thread_windows(is_running, app_handle, hotkey);
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                hotkey_thread_linux(is_running, app_handle, hotkey);
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Global hotkeys are only supported on Windows and Linux");
        }
    }

    /// Stop the hotkey listener thread
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        log_info("Hotkey listener stop requested");
    }
}

#[cfg(target_os = "windows")]
fn is_d2_or_app_foreground() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }

        let our_pid = GetCurrentProcessId();
        let mut fg_pid: u32 = 0;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid as *mut u32));
        if fg_pid == our_pid {
            return true;
        }

        let class: Vec<u16> = OsStr::new("Diablo II")
            .encode_wide()
            .chain(Some(0))
            .collect();
        if let Ok(d2) = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            return !d2.0.is_null() && fg.0 == d2.0;
        }

        false
    }
}

#[cfg(target_os = "windows")]
fn is_d2_foreground() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetForegroundWindow};

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let class: Vec<u16> = OsStr::new("Diablo II")
            .encode_wide()
            .chain(Some(0))
            .collect();
        match FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            Ok(d2) => !d2.0.is_null() && fg.0 == d2.0,
            Err(_) => false,
        }
    }
}

#[cfg(target_os = "windows")]
fn hotkey_thread_windows(is_running: Arc<AtomicBool>, app_handle: AppHandle, hotkey: HotkeyConfig) {
    log_info(&format!(
        "Hotkey thread starting with: {} (key={:#x}, mods={:#x})",
        hotkey.display, hotkey.key_code, hotkey.modifiers
    ));

    if is_mouse_hotkey_key(hotkey.key_code) {
        log_info(&format!(
            "Mouse hotkey {} using polling watcher",
            hotkey.display
        ));

        let mut prev_down = false;
        while is_running.load(Ordering::SeqCst) {
            let active = chord_keys_are_pressed(&hotkey);
            if active && !prev_down {
                log_info("Toggle main window mouse hotkey pressed");
                toggle_main_window(&app_handle);
            }
            prev_down = active;
            thread::sleep(std::time::Duration::from_millis(30));
        }

        log_info("Mouse hotkey thread stopped");
        return;
    }

    // Register the hotkey
    let modifiers = HOT_KEY_MODIFIERS(hotkey.modifiers) | MOD_NOREPEAT;

    let result = unsafe {
        RegisterHotKey(
            HWND::default(),
            HOTKEY_ID_TOGGLE_MAIN,
            modifiers,
            hotkey.key_code,
        )
    };

    if result.is_err() {
        log_error(&format!(
            "Failed to register hotkey {}: {:?}",
            hotkey.display, result
        ));
        is_running.store(false, Ordering::SeqCst);
        return;
    }

    log_info(&format!(
        "Hotkey {} registered successfully",
        hotkey.display
    ));

    // Message loop using PeekMessage to allow checking is_running flag
    let mut msg = MSG::default();
    while is_running.load(Ordering::SeqCst) {
        unsafe {
            // Use PeekMessage to check for messages without blocking
            let has_message = PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE);

            if has_message.as_bool() {
                if msg.message == WM_HOTKEY {
                    let hotkey_id = msg.wParam.0 as i32;
                    if hotkey_id == HOTKEY_ID_TOGGLE_MAIN {
                        log_info("Toggle main window hotkey pressed");
                        toggle_main_window(&app_handle);
                    }
                }

                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // No message, sleep a bit to avoid busy-waiting
                thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }

    // Unregister hotkey before exiting
    unsafe {
        let _ = UnregisterHotKey(HWND::default(), HOTKEY_ID_TOGGLE_MAIN);
    }

    log_info("Hotkey thread stopped");
}

#[cfg(target_os = "linux")]
fn hotkey_thread_linux(is_running: Arc<AtomicBool>, app_handle: AppHandle, hotkey: HotkeyConfig) {
    log_info(&format!(
        "Hotkey thread starting with: {} (key={:#x}, mods={:#x})",
        hotkey.display, hotkey.key_code, hotkey.modifiers
    ));

    if is_mouse_hotkey_key(hotkey.key_code) {
        log_info(&format!(
            "Mouse hotkey {} is not supported on Linux, ignoring",
            hotkey.display
        ));
        return;
    }

    let mut prev_down = false;
    while is_running.load(Ordering::SeqCst) {
        let active = chord_keys_are_pressed_linux(&hotkey);
        if active && !prev_down {
            log_info("Toggle main window hotkey pressed");
            toggle_main_window(&app_handle);
        }
        prev_down = active;
        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Hotkey thread stopped");
}

/// Toggle the main window visibility
fn toggle_main_window(app_handle: &AppHandle) {
    if let Some(main_window) = app_handle.get_webview_window("main") {
        match main_window.is_visible() {
            Ok(visible) => {
                if visible {
                    log_info("Hiding main window");
                    if let Err(e) = main_window.hide() {
                        log_error(&format!("Failed to hide main window: {}", e));
                    }
                } else {
                    log_info("Showing main window");
                    if let Err(e) = main_window.show() {
                        log_error(&format!("Failed to show main window: {}", e));
                    }
                    // Also bring to front and focus
                    if let Err(e) = main_window.set_focus() {
                        log_error(&format!("Failed to focus main window: {}", e));
                    }
                }
                // Emit event to frontend
                if let Err(e) = app_handle.emit("main-window-toggled", !visible) {
                    log_error(&format!("Failed to emit main-window-toggled event: {}", e));
                }
            }
            Err(e) => {
                log_error(&format!("Failed to check main window visibility: {}", e));
            }
        }
    } else {
        log_error("Main window not found");
    }
}

/// Tauri command: Update the hotkey configuration
#[tauri::command]
pub fn update_hotkey(
    state: tauri::State<HotkeyState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!("Updating hotkey to: {}", hotkey.display));
    state.start(app, hotkey);
    Ok(())
}

// Edit-mode watcher: RegisterHotKey can't deliver release events or accept
// modifier-only chords, so we poll GetAsyncKeyState and emit on edge
// transitions instead.

pub struct EditModeState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
}

impl EditModeState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(HotkeyConfig {
                key_code: 0,
                modifiers: 0x0001 | 0x0002, // MOD_ALT | MOD_CONTROL
                display: "Ctrl+Alt".to_string(),
            })),
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Edit-mode watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                edit_mode_thread_windows(is_running, current_hotkey, app_handle);
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                edit_mode_thread_linux(is_running, current_hotkey, app_handle);
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Edit-mode watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

impl Default for EditModeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
fn is_key_down(vk: u16) -> bool {
    // High bit of GetAsyncKeyState is set while the key is held.
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

#[cfg(target_os = "windows")]
fn chord_is_pressed(hk: &HotkeyConfig) -> bool {
    if !is_d2_or_app_foreground() {
        return false;
    }

    chord_keys_are_pressed(hk)
}

#[cfg(target_os = "windows")]
fn chord_is_pressed_d2_only(hk: &HotkeyConfig) -> bool {
    is_d2_foreground() && chord_keys_are_pressed(hk)
}

#[cfg(target_os = "windows")]
fn chord_keys_are_pressed(hk: &HotkeyConfig) -> bool {
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;

    if hk.key_code == 0 && hk.modifiers == 0 {
        return false;
    }

    if hk.modifiers & MOD_CONTROL != 0 && !is_key_down(VK_CONTROL.0) {
        return false;
    }
    if hk.modifiers & MOD_SHIFT != 0 && !is_key_down(VK_SHIFT.0) {
        return false;
    }
    if hk.modifiers & MOD_ALT != 0 && !is_key_down(VK_MENU.0) {
        return false;
    }
    if hk.modifiers & MOD_WIN != 0 && !(is_key_down(VK_LWIN.0) || is_key_down(VK_RWIN.0)) {
        return false;
    }

    if hk.key_code != 0 && !is_key_down(hk.key_code as u16) {
        return false;
    }

    true
}

#[cfg(target_os = "windows")]
fn edit_mode_thread_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
) {
    log_info("Edit-mode watcher thread starting");

    let mut last_active = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        // Reset on reconfigure so we don't emit a phantom release for the old chord.
        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            if last_active {
                let _ =
                    app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": false }));
            }
            last_active = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed(&hk);

        if active != last_active {
            if let Err(e) =
                app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": active }))
            {
                log_error(&format!("Failed to emit overlay-edit-mode event: {}", e));
            }
            last_active = active;
        }

        thread::sleep(std::time::Duration::from_millis(30));
    }

    // Release on shutdown so the overlay doesn't stay stuck in interactive mode.
    if last_active {
        let _ = app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": false }));
    }
    log_info("Edit-mode watcher thread stopped");
}

#[cfg(target_os = "linux")]
fn edit_mode_thread_linux(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
) {
    log_info("Edit-mode watcher thread starting");

    let mut last_active = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            if last_active {
                let _ =
                    app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": false }));
            }
            last_active = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed_linux(&hk);

        if active != last_active {
            if let Err(e) =
                app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": active }))
            {
                log_error(&format!("Failed to emit overlay-edit-mode event: {}", e));
            }
            last_active = active;
        }

        thread::sleep(std::time::Duration::from_millis(30));
    }

    if last_active {
        let _ = app_handle.emit("overlay-edit-mode", serde_json::json!({ "active": false }));
    }
    log_info("Edit-mode watcher thread stopped");
}

#[tauri::command]
pub fn update_edit_mode_hotkey(
    state: tauri::State<EditModeState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!("Updating edit-mode hotkey to: {}", hotkey.display));
    state.start(app, hotkey);
    Ok(())
}

// Drives an AtomicBool the scanner mirrors into the hook's g_force_show_all.
pub struct RevealHiddenState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    active: Arc<AtomicBool>,
}

impl RevealHiddenState {
    pub fn new(active: Arc<AtomicBool>) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(HotkeyConfig {
                key_code: 0x5A, // 'Z'
                modifiers: 0,
                display: "Z".to_string(),
            })),
            active,
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Reveal-hidden watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();
        let active = self.active.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                reveal_hidden_thread_windows(is_running, current_hotkey, active, app_handle);
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                reveal_hidden_thread_linux(is_running, current_hotkey, active, app_handle);
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Reveal-hidden watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey, active);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn reveal_hidden_thread_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    active: Arc<AtomicBool>,
    app_handle: AppHandle,
) {
    log_info("Reveal-hidden watcher thread starting");

    let mut last_active = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        // Reset on reconfigure so a stuck-down old key doesn't keep reveal active.
        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            if last_active {
                active.store(false, Ordering::SeqCst);
                let _ = app_handle.emit(
                    "reveal-hidden-state",
                    serde_json::json!({ "active": false }),
                );
            }
            last_active = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let pressed = chord_is_pressed(&hk);

        if pressed != last_active {
            active.store(pressed, Ordering::SeqCst);
            if let Err(e) = app_handle.emit(
                "reveal-hidden-state",
                serde_json::json!({ "active": pressed }),
            ) {
                log_error(&format!("Failed to emit reveal-hidden-state event: {}", e));
            }
            last_active = pressed;
        }

        thread::sleep(std::time::Duration::from_millis(30));
    }

    if last_active {
        active.store(false, Ordering::SeqCst);
        let _ = app_handle.emit(
            "reveal-hidden-state",
            serde_json::json!({ "active": false }),
        );
    }
    log_info("Reveal-hidden watcher thread stopped");
}

#[cfg(target_os = "linux")]
fn reveal_hidden_thread_linux(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    active: Arc<AtomicBool>,
    app_handle: AppHandle,
) {
    log_info("Reveal-hidden watcher thread starting");

    let mut last_active = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            if last_active {
                active.store(false, Ordering::SeqCst);
                let _ = app_handle.emit(
                    "reveal-hidden-state",
                    serde_json::json!({ "active": false }),
                );
            }
            last_active = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let pressed = chord_is_pressed_linux(&hk);

        if pressed != last_active {
            active.store(pressed, Ordering::SeqCst);
            if let Err(e) = app_handle.emit(
                "reveal-hidden-state",
                serde_json::json!({ "active": pressed }),
            ) {
                log_error(&format!("Failed to emit reveal-hidden-state event: {}", e));
            }
            last_active = pressed;
        }

        thread::sleep(std::time::Duration::from_millis(30));
    }

    if last_active {
        active.store(false, Ordering::SeqCst);
        let _ = app_handle.emit(
            "reveal-hidden-state",
            serde_json::json!({ "active": false }),
        );
    }
    log_info("Reveal-hidden watcher thread stopped");
}

#[tauri::command]
pub fn update_reveal_hidden_hotkey(
    state: tauri::State<RevealHiddenState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!(
        "Updating reveal-hidden hotkey to: {}",
        hotkey.display
    ));
    state.start(app, hotkey);
    Ok(())
}

/// Watcher for the "toggle loot history" hotkey. Polls the configured key
/// every ~30ms and emits `toggle-loot-history` to the overlay webview on
/// rising-edge press. Toggle semantics — frontend manages visibility state.
pub struct LootHistoryHotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
}

impl LootHistoryHotkeyState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(default_loot_history_hotkey())),
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Loot-history watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                loot_history_hotkey_thread_windows(is_running, current_hotkey, app_handle);
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                loot_history_hotkey_thread_linux(is_running, current_hotkey, app_handle);
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Loot-history watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

impl Default for LootHistoryHotkeyState {
    fn default() -> Self {
        Self::new()
    }
}

fn default_loot_history_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x4E,
        modifiers: 0x0001,
        display: "Alt+N".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn loot_history_hotkey_thread_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
) {
    log_info("Loot-history hotkey watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        // Reset edge detector on reconfigure so a held old key doesn't fire.
        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed(&hk);

        if active && !prev_down {
            if let Err(e) = app_handle.emit("toggle-loot-history", ()) {
                log_error(&format!("Failed to emit toggle-loot-history: {}", e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Loot-history hotkey watcher thread stopped");
}

#[cfg(target_os = "linux")]
fn loot_history_hotkey_thread_linux(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
) {
    log_info("Loot-history hotkey watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed_linux(&hk);

        if active && !prev_down {
            if let Err(e) = app_handle.emit("toggle-loot-history", ()) {
                log_error(&format!("Failed to emit toggle-loot-history: {}", e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Loot-history hotkey watcher thread stopped");
}

#[tauri::command]
pub fn update_loot_history_hotkey(
    state: tauri::State<LootHistoryHotkeyState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!(
        "Updating loot-history hotkey to: {}",
        hotkey.display
    ));
    state.start(app, hotkey);
    Ok(())
}

pub struct ItemSearchHotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    scanner_state: Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
}

impl ItemSearchHotkeyState {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new(
        scanner_state: Arc<
            std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
        >,
    ) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(default_item_search_hotkey())),
            scanner_state,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(default_item_search_hotkey())),
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Item-search watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();

        #[cfg(target_os = "windows")]
        {
            let scanner_state = self.scanner_state.clone();
            thread::spawn(move || {
                item_search_hotkey_thread_windows(
                    is_running,
                    current_hotkey,
                    scanner_state,
                    app_handle,
                );
            });
        }

        #[cfg(target_os = "linux")]
        {
            let scanner_state = self.scanner_state.clone();
            thread::spawn(move || {
                item_search_hotkey_thread_linux(
                    is_running,
                    current_hotkey,
                    scanner_state,
                    app_handle,
                );
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Item-search watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

fn default_item_search_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x46,
        modifiers: 0x0001,
        display: "Alt+F".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn item_search_hotkey_thread_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    scanner_state: Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
    app_handle: AppHandle,
) {
    log_info("Item-search hotkey watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed_d2_only(&hk);
        if active && !prev_down {
            let query = scanner_state
                .read()
                .ok()
                .and_then(|guard| guard.as_ref().cloned())
                .and_then(
                    |shared| match crate::hovered_item::read_hovered_item_name(&shared) {
                        Ok(name) => name,
                        Err(e) => {
                            log_error(&format!("Hovered item lookup failed: {}", e));
                            None
                        }
                    },
                );
            if let Err(e) = app_handle.emit("open-item-search", OpenItemSearchPayload { query }) {
                log_error(&format!("Failed to emit open-item-search: {}", e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Item-search hotkey watcher thread stopped");
}

#[cfg(target_os = "linux")]
fn item_search_hotkey_thread_linux(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    scanner_state: Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
    app_handle: AppHandle,
) {
    log_info("Item-search hotkey watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed_d2_only_linux(&hk);
        if active && !prev_down {
            let query = scanner_state
                .read()
                .ok()
                .and_then(|guard| guard.as_ref().cloned())
                .and_then(
                    |shared| match crate::hovered_item::read_hovered_item_name(&shared) {
                        Ok(name) => name,
                        Err(e) => {
                            log_error(&format!("Hovered item lookup failed: {}", e));
                            None
                        }
                    },
                );
            if let Err(e) = app_handle.emit("open-item-search", OpenItemSearchPayload { query }) {
                log_error(&format!("Failed to emit open-item-search: {}", e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Item-search hotkey watcher thread stopped");
}

#[tauri::command]
pub fn update_item_search_hotkey(
    state: tauri::State<ItemSearchHotkeyState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!(
        "Updating item-search hotkey to: {}",
        hotkey.display
    ));
    state.start(app, hotkey);
    Ok(())
}

pub struct DpsMeterResetHotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
}

impl DpsMeterResetHotkeyState {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(HotkeyConfig {
                key_code: 0,
                modifiers: 0,
                display: "None".to_string(),
            })),
        }
    }

    pub fn start(&self, app_handle: AppHandle, hotkey: HotkeyConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("DPS-meter reset watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_hotkey.lock() {
            *current = hotkey.clone();
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_hotkey = self.current_hotkey.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                rising_edge_watcher_windows(
                    is_running,
                    current_hotkey,
                    app_handle,
                    "reset-dps-session",
                );
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                rising_edge_watcher_linux(
                    is_running,
                    current_hotkey,
                    app_handle,
                    "reset-dps-session",
                );
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("DPS-meter reset watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

impl Default for DpsMeterResetHotkeyState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
fn rising_edge_watcher_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
    event_name: &'static str,
) {
    log_info(&format!("'{}' hotkey watcher thread starting", event_name));

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed(&hk);
        if active && !prev_down {
            if let Err(e) = app_handle.emit(event_name, ()) {
                log_error(&format!("Failed to emit {}: {}", event_name, e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info(&format!("'{}' hotkey watcher thread stopped", event_name));
}

#[cfg(target_os = "linux")]
fn rising_edge_watcher_linux(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    app_handle: AppHandle,
    event_name: &'static str,
) {
    log_info(&format!("'{}' hotkey watcher thread starting", event_name));

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let hk = match current_hotkey.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if hk.key_code != last_key_code || hk.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let active = chord_is_pressed_linux(&hk);
        if active && !prev_down {
            if let Err(e) = app_handle.emit(event_name, ()) {
                log_error(&format!("Failed to emit {}: {}", event_name, e));
            }
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info(&format!("'{}' hotkey watcher thread stopped", event_name));
}

#[tauri::command]
pub fn update_dps_meter_reset_hotkey(
    state: tauri::State<DpsMeterResetHotkeyState>,
    app: AppHandle,
    hotkey: HotkeyConfig,
) -> Result<(), String> {
    log_info(&format!(
        "Updating DPS-meter reset hotkey to: {}",
        hotkey.display
    ));
    state.start(app, hotkey);
    Ok(())
}

/// Preset text + hotkey for the create-game autofill feature. Bundled
/// together (rather than reusing the bare `HotkeyConfig` pattern) since
/// the watcher thread needs all four values on every rising edge, not
/// just the chord to detect.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameCreateAutofillConfig {
    pub hotkey: HotkeyConfig,
    /// Combined with the (in-memory, not persisted) auto-increment
    /// counter to form the game name: `{name_prefix}{index}`.
    pub name_prefix: String,
    /// Fixed password, used when `password_use_prefix` is false.
    pub password: String,
    /// Combined with the same counter as the name
    /// (`{password_prefix}{index}`), used instead of `password` when
    /// `password_use_prefix` is true.
    pub password_prefix: String,
    pub password_use_prefix: bool,
    pub description: String,
}

pub struct GameCreateAutofillHotkeyState {
    is_running: Arc<AtomicBool>,
    current_config: Arc<std::sync::Mutex<GameCreateAutofillConfig>>,
    /// Auto-increments once per autofill trigger. Deliberately in-memory
    /// only (not part of `AppSettings`/settings.json) — resets to 1 each
    /// app launch.
    next_index: Arc<std::sync::atomic::AtomicU32>,
    /// Same `game_status` atomic the UI's in-game/menu indicator reads
    /// (`GAME_STATUS_UNKNOWN`/`_INGAME`/`_MENU` in `main.rs`) — gates
    /// firing to menu only, since the create-game screen (and thus any
    /// use for this) only exists there.
    game_status: Arc<std::sync::atomic::AtomicU8>,
}

impl GameCreateAutofillHotkeyState {
    pub fn new(game_status: Arc<std::sync::atomic::AtomicU8>) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_config: Arc::new(std::sync::Mutex::new(GameCreateAutofillConfig::default())),
            next_index: Arc::new(std::sync::atomic::AtomicU32::new(1)),
            game_status,
        }
    }

    pub fn start(&self, config: GameCreateAutofillConfig) {
        if self.is_running.load(Ordering::SeqCst) {
            log_info("Game-create autofill watcher already running, restarting with new config");
            self.stop();
            thread::sleep(std::time::Duration::from_millis(80));
        }

        if let Ok(mut current) = self.current_config.lock() {
            *current = config;
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = self.is_running.clone();
        let current_config = self.current_config.clone();
        let next_index = self.next_index.clone();
        let game_status = self.game_status.clone();

        #[cfg(target_os = "windows")]
        {
            thread::spawn(move || {
                game_create_autofill_watcher_windows(
                    is_running,
                    current_config,
                    next_index,
                    game_status,
                );
            });
        }

        #[cfg(target_os = "linux")]
        {
            thread::spawn(move || {
                game_create_autofill_watcher_linux(
                    is_running,
                    current_config,
                    next_index,
                    game_status,
                );
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Game-create autofill watcher is only supported on Windows and Linux");
            let _ = (current_config, next_index, game_status);
        }
    }

    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
fn game_create_autofill_watcher_windows(
    is_running: Arc<AtomicBool>,
    current_config: Arc<std::sync::Mutex<GameCreateAutofillConfig>>,
    next_index: Arc<std::sync::atomic::AtomicU32>,
    game_status: Arc<std::sync::atomic::AtomicU8>,
) {
    log_info("Game-create autofill watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let cfg = match current_config.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if cfg.hotkey.key_code != last_key_code || cfg.hotkey.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = cfg.hotkey.key_code;
            last_modifiers = cfg.hotkey.modifiers;
        }

        let in_menu = game_status.load(Ordering::SeqCst) == crate::GAME_STATUS_MENU;
        let active = in_menu && chord_is_pressed_d2_only(&cfg.hotkey);
        if active && !prev_down {
            run_game_create_autofill(&cfg, &next_index);
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Game-create autofill watcher thread stopped");
}

#[cfg(target_os = "linux")]
fn game_create_autofill_watcher_linux(
    is_running: Arc<AtomicBool>,
    current_config: Arc<std::sync::Mutex<GameCreateAutofillConfig>>,
    next_index: Arc<std::sync::atomic::AtomicU32>,
    game_status: Arc<std::sync::atomic::AtomicU8>,
) {
    log_info("Game-create autofill watcher thread starting");

    let mut prev_down = false;
    let mut last_key_code: u32 = 0;
    let mut last_modifiers: u32 = 0;

    while is_running.load(Ordering::SeqCst) {
        let cfg = match current_config.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };

        if cfg.hotkey.key_code != last_key_code || cfg.hotkey.modifiers != last_modifiers {
            prev_down = false;
            last_key_code = cfg.hotkey.key_code;
            last_modifiers = cfg.hotkey.modifiers;
        }

        let in_menu = game_status.load(Ordering::SeqCst) == crate::GAME_STATUS_MENU;
        let active = in_menu && chord_is_pressed_d2_only_linux(&cfg.hotkey);
        if active && !prev_down {
            run_game_create_autofill(&cfg, &next_index);
        }
        prev_down = active;

        thread::sleep(std::time::Duration::from_millis(30));
    }

    log_info("Game-create autofill watcher thread stopped");
}

/// Builds `{prefix}{index}` name/password from `cfg` and the shared
/// counter, advances the counter, and types the result. The same index
/// value is used for both name and password within one trigger.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn run_game_create_autofill(
    cfg: &GameCreateAutofillConfig,
    next_index: &std::sync::atomic::AtomicU32,
) {
    let index = next_index.fetch_add(1, Ordering::SeqCst);
    let name = format!("{}{}", cfg.name_prefix, index);
    let password = if cfg.password_use_prefix {
        format!("{}{}", cfg.password_prefix, index)
    } else {
        cfg.password.clone()
    };
    if let Err(e) = crate::keystroke_sim::autofill_create_game(&name, &password, &cfg.description) {
        log_error(&format!("Game-create autofill failed: {}", e));
    }
}

#[tauri::command]
pub fn update_game_create_autofill_hotkey(
    state: tauri::State<GameCreateAutofillHotkeyState>,
    config: GameCreateAutofillConfig,
) -> Result<(), String> {
    log_info(&format!(
        "Updating game-create autofill hotkey to: {}",
        config.hotkey.display
    ));
    state.start(config);
    Ok(())
}
