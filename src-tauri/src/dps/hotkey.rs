//! Complete DPS-session reset watcher and configuration command.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use crate::hotkeys::chord_is_pressed;
#[cfg(target_os = "linux")]
use crate::hotkeys::chord_is_pressed_linux;
use crate::hotkeys::HotkeyConfig;
use crate::logger::{error as log_error, info as log_info};

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
