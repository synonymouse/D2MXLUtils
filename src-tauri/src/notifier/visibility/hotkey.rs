//! Complete hold-to-reveal watcher and configuration command.

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
