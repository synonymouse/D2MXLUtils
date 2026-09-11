//! Complete main-window and overlay edit-mode watchers.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "windows")]
use crate::hotkeys::{chord_is_pressed, chord_keys_are_pressed};
#[cfg(target_os = "linux")]
use crate::hotkeys::{chord_is_pressed_linux, chord_keys_are_pressed_linux};
use crate::hotkeys::{is_mouse_hotkey_key, HotkeyConfig};
use crate::logger::{error as log_error, info as log_info};

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
};

/// Hotkey ID for toggle main window
const HOTKEY_ID_TOGGLE_MAIN: i32 = 1;

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
