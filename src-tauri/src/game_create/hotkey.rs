//! Complete menu-gated game-create autofill watcher and configuration command.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

#[cfg(target_os = "windows")]
use crate::hotkeys::chord_is_pressed_d2_only;
#[cfg(target_os = "linux")]
use crate::hotkeys::chord_is_pressed_d2_only_linux;
use crate::hotkeys::HotkeyConfig;
use crate::logger::{error as log_error, info as log_info};

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
    if let Err(e) = super::input::autofill_create_game(&name, &password, &cfg.description) {
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
