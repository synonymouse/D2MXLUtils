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

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key_code: 0x4B,    // 'K' key
            modifiers: 0x0002, // MOD_CONTROL
            display: "Ctrl+K".to_string(),
        }
    }
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

        #[cfg(not(target_os = "windows"))]
        {
            log_info("Global hotkeys are only supported on Windows");
        }
    }

    /// Stop the hotkey listener thread
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        log_info("Hotkey listener stop requested");
    }
}

#[cfg(target_os = "windows")]
fn hotkey_thread_windows(is_running: Arc<AtomicBool>, app_handle: AppHandle, hotkey: HotkeyConfig) {
    log_info(&format!(
        "Hotkey thread starting with: {} (key={:#x}, mods={:#x})",
        hotkey.display, hotkey.key_code, hotkey.modifiers
    ));

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

        #[cfg(not(target_os = "windows"))]
        {
            log_info("Edit-mode watcher is only supported on Windows");
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

        #[cfg(not(target_os = "windows"))]
        {
            log_info("Reveal-hidden watcher is only supported on Windows");
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
                let _ = app_handle
                    .emit("reveal-hidden-state", serde_json::json!({ "active": false }));
            }
            last_active = false;
            last_key_code = hk.key_code;
            last_modifiers = hk.modifiers;
        }

        let pressed = reveal_chord_is_pressed(&hk);

        if pressed != last_active {
            active.store(pressed, Ordering::SeqCst);
            if let Err(e) = app_handle
                .emit("reveal-hidden-state", serde_json::json!({ "active": pressed }))
            {
                log_error(&format!("Failed to emit reveal-hidden-state event: {}", e));
            }
            last_active = pressed;
        }

        thread::sleep(std::time::Duration::from_millis(30));
    }

    if last_active {
        active.store(false, Ordering::SeqCst);
        let _ = app_handle.emit("reveal-hidden-state", serde_json::json!({ "active": false }));
    }
    log_info("Reveal-hidden watcher thread stopped");
}

// Like chord_is_pressed but allows a bare key with no modifier (e.g. 'Z').
#[cfg(target_os = "windows")]
fn reveal_chord_is_pressed(hk: &HotkeyConfig) -> bool {
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
