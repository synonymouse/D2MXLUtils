//! Complete loot-history toggle watcher and configuration command.

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
