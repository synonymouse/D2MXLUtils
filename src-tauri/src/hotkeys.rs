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
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
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
