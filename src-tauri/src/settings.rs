//! Settings persistence module for D2MXLUtils
//!
//! Handles loading and saving application settings using tauri-plugin-store.
//! Settings are stored in a JSON file in the app's data directory.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;

use crate::hotkeys::HotkeyConfig;
use crate::logger::{error as log_error, info as log_info};

const SETTINGS_FILE: &str = "settings.json";

/// Application settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// UI theme: "dark" or "light"
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Master volume for drop notification sounds (0.0 - 1.0, 0 = silent)
    #[serde(default = "default_volume")]
    pub sound_volume: f32,

    /// Active loot filter profile name
    #[serde(default)]
    pub active_profile: Option<String>,

    /// Notification display duration in milliseconds
    #[serde(default = "default_notification_duration")]
    pub notification_duration: u32,

    /// Notification stack direction: "up" or "down"
    #[serde(default = "default_stack_direction")]
    pub notification_stack_direction: String,

    /// Notification font size in pixels
    #[serde(default = "default_notification_font_size")]
    pub notification_font_size: u32,

    /// Notification background opacity (0.0 - 1.0)
    #[serde(default = "default_notification_opacity")]
    pub notification_opacity: f32,

    /// Notification position X offset from edge (percentage 0-100)
    #[serde(default = "default_notification_x")]
    pub notification_x: f32,

    /// Notification position Y offset from edge (percentage 0-100)
    #[serde(default = "default_notification_y")]
    pub notification_y: f32,

    /// When true, show only base name for Set/TU/SU/SSU/SSSU drops
    /// (single-line layout). Stat-flagged rules ignore this.
    #[serde(default)]
    pub compact_name: bool,

    /// Hotkey configuration for toggling main window
    #[serde(default)]
    pub toggle_window_hotkey: HotkeyConfig,

    /// Hotkey held to enter overlay edit mode (drag notification anchor)
    #[serde(default = "default_edit_overlay_hotkey")]
    pub edit_overlay_hotkey: HotkeyConfig,

    /// Hotkey held to reveal every item on the ground, bypassing `hide` rules
    #[serde(default = "default_reveal_hidden_hotkey")]
    pub reveal_hidden_hotkey: HotkeyConfig,

    /// Hotkey to toggle the in-game loot history overlay panel.
    #[serde(default = "default_loot_history_hotkey")]
    pub loot_history_hotkey: HotkeyConfig,

    /// When true, scanner logs per-item filter decisions (noisy; opt-in for debugging).
    #[serde(default)]
    pub verbose_filter_logging: bool,

    #[serde(default = "default_auto_always_show_items")]
    pub auto_always_show_items: bool,
}

/// Window state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

// Default value functions
fn default_theme() -> String {
    "dark".to_string()
}

fn default_volume() -> f32 {
    0.8
}

fn default_notification_duration() -> u32 {
    5000
}

fn default_stack_direction() -> String {
    "up".to_string()
}

fn default_notification_font_size() -> u32 {
    14
}

fn default_notification_opacity() -> f32 {
    0.9
}

fn default_notification_x() -> f32 {
    1.0
}

fn default_notification_y() -> f32 {
    1.0
}

fn default_auto_always_show_items() -> bool {
    true
}

fn default_edit_overlay_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0,
        modifiers: 0x0001 | 0x0002, // MOD_ALT | MOD_CONTROL
        display: "Ctrl+Alt".to_string(),
    }
}

fn default_reveal_hidden_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x5A, // 'Z'
        modifiers: 0,
        display: "Z".to_string(),
    }
}

fn default_loot_history_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x4E, // 'N'
        modifiers: 0,
        display: "N".to_string(),
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            sound_volume: default_volume(),
            active_profile: None,
            notification_duration: default_notification_duration(),
            notification_stack_direction: default_stack_direction(),
            notification_font_size: default_notification_font_size(),
            notification_opacity: default_notification_opacity(),
            notification_x: default_notification_x(),
            notification_y: default_notification_y(),
            compact_name: false,
            toggle_window_hotkey: HotkeyConfig::default(),
            edit_overlay_hotkey: default_edit_overlay_hotkey(),
            reveal_hidden_hotkey: default_reveal_hidden_hotkey(),
            loot_history_hotkey: default_loot_history_hotkey(),
            verbose_filter_logging: false,
            auto_always_show_items: default_auto_always_show_items(),
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 1024,
            height: 640,
            maximized: false,
        }
    }
}

/// Load application settings from the store
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| format!("Failed to open settings store: {}", e))?;

    // Try to get settings from store, use defaults if not found
    let settings: AppSettings = match store.get("settings") {
        Some(value) => serde_json::from_value(value.clone()).unwrap_or_else(|e| {
            log_error(&format!("Failed to parse settings, using defaults: {}", e));
            AppSettings::default()
        }),
        None => {
            log_info("No settings found, using defaults");
            AppSettings::default()
        }
    };

    Ok(settings)
}

/// Save application settings to the store
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| format!("Failed to open settings store: {}", e))?;

    let value = serde_json::to_value(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    store.set("settings", value);

    store
        .save()
        .map_err(|e| format!("Failed to save settings to disk: {}", e))?;

    if let Err(e) = app.emit("settings-updated", &settings) {
        log_error(&format!("Failed to emit settings-updated: {}", e));
    }

    Ok(())
}

/// Load window state from the store
#[tauri::command]
pub fn get_window_state(
    app: AppHandle,
    window_label: String,
) -> Result<Option<WindowState>, String> {
    log_info(&format!("Loading window state for: {}", window_label));

    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| format!("Failed to open settings store: {}", e))?;

    let key = format!("window_{}", window_label);

    let state: Option<WindowState> = match store.get(&key) {
        Some(value) => serde_json::from_value(value.clone()).ok(),
        None => None,
    };

    Ok(state)
}

/// Save window state to the store
#[tauri::command]
pub fn save_window_state(
    app: AppHandle,
    window_label: String,
    state: WindowState,
) -> Result<(), String> {
    log_info(&format!(
        "Saving window state for {}: {}x{} at ({}, {})",
        window_label, state.width, state.height, state.x, state.y
    ));

    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| format!("Failed to open settings store: {}", e))?;

    let key = format!("window_{}", window_label);
    let value = serde_json::to_value(&state)
        .map_err(|e| format!("Failed to serialize window state: {}", e))?;

    store.set(key, value);

    store
        .save()
        .map_err(|e| format!("Failed to save window state to disk: {}", e))?;

    Ok(())
}
