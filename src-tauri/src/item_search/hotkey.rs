//! Complete hovered-item search watcher and configuration command.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use crate::hotkeys::chord_is_pressed_d2_only;
#[cfg(target_os = "linux")]
use crate::hotkeys::chord_is_pressed_d2_only_linux;
use crate::hotkeys::HotkeyConfig;
use crate::logger::{error as log_error, info as log_info};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenItemSearchPayload {
    query: Option<String>,
}

pub struct ItemSearchHotkeyState {
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    verbose_logging: Arc<AtomicBool>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    scanner_state: Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
}

impl ItemSearchHotkeyState {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fn new(
        scanner_state: Arc<
            std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
        >,
        verbose_logging: Arc<AtomicBool>,
    ) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(default_item_search_hotkey())),
            verbose_logging,
            scanner_state,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    pub fn new(verbose_logging: Arc<AtomicBool>) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_hotkey: Arc::new(std::sync::Mutex::new(default_item_search_hotkey())),
            verbose_logging,
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
        let verbose_logging = self.verbose_logging.clone();

        #[cfg(target_os = "windows")]
        {
            let scanner_state = self.scanner_state.clone();
            thread::spawn(move || {
                item_search_hotkey_thread_windows(
                    is_running,
                    current_hotkey,
                    scanner_state,
                    verbose_logging,
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
                    verbose_logging,
                    app_handle,
                );
            });
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            log_info("Item-search watcher is only supported on Windows and Linux");
            let _ = (app_handle, current_hotkey, verbose_logging);
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

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn open_item_search_from_hover(
    scanner_state: &Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
    verbose: bool,
    hotkey_display: &str,
    app_handle: &AppHandle,
) {
    if verbose {
        log_info(&format!("[ItemSearch] hotkey {} pressed", hotkey_display));
    }

    let shared = scanner_state
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());
    let has_scanner = shared.is_some();
    let query =
        shared.and_then(
            |shared| match super::capture::read_hovered_item_name(&shared, verbose) {
                Ok(name) => name,
                Err(e) => {
                    log_error(&format!("Hovered item lookup failed: {}", e));
                    None
                }
            },
        );

    if verbose {
        log_info(&format!(
            "[ItemSearch] scanner_attached={} hovered_item={:?}",
            has_scanner, query
        ));
    }
    match app_handle.emit("open-item-search", OpenItemSearchPayload { query }) {
        Ok(()) => {
            if verbose {
                log_info("[ItemSearch] emitted open-item-search");
            }
        }
        Err(e) => log_error(&format!("Failed to emit open-item-search: {}", e)),
    }
}

#[cfg(target_os = "windows")]
fn item_search_hotkey_thread_windows(
    is_running: Arc<AtomicBool>,
    current_hotkey: Arc<std::sync::Mutex<HotkeyConfig>>,
    scanner_state: Arc<std::sync::RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
    verbose_logging: Arc<AtomicBool>,
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

        let verbose = verbose_logging.load(Ordering::SeqCst);
        let active = chord_is_pressed_d2_only(&hk);
        if active && !prev_down {
            open_item_search_from_hover(&scanner_state, verbose, &hk.display, &app_handle);
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
    verbose_logging: Arc<AtomicBool>,
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

        let verbose = verbose_logging.load(Ordering::SeqCst);
        let active = chord_is_pressed_d2_only_linux(&hk);
        if active && !prev_down {
            open_item_search_from_hover(&scanner_state, verbose, &hk.display, &app_handle);
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
