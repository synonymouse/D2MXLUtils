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

/// One configurable drop-sound slot. Index in `AppSettings.sounds` + 1
/// equals the DSL keyword index (e.g. element 0 -> `sound1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundSlot {
    pub label: String,
    pub volume: f32,
    pub source: SoundSource,
}

/// What plays for a given slot.
/// - `Default`: bundled `public/sounds/{N}.mp3` (slots 1..=7 only).
/// - `Custom`: user-imported file in `app_data_dir/sounds/`.
/// - `Empty`: silence; only for slots >= 8 after deletion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SoundSource {
    Default,
    Custom { file_name: String },
    Empty,
}

fn default_sounds() -> Vec<SoundSlot> {
    (1..=7)
        .map(|n| SoundSlot {
            label: format!("Sound {}", n),
            volume: 0.8,
            source: SoundSource::Default,
        })
        .collect()
}

/// Application settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// UI theme: "dark" or "light"
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Master multiplier for drop notification sounds (0.0 - 1.0). Final played gain = `sound_volume * slot.volume`.
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

    /// Per-slot drop sounds. Slot index = element position + 1.
    /// Final played gain = `sound_volume * slot.volume`.
    #[serde(default = "default_sounds")]
    pub sounds: Vec<SoundSlot>,
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
            sounds: default_sounds(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_without_sounds_field_seeds_seven_defaults() {
        // settings.json that predates the Sounds tab — no `sounds` field.
        let json = r#"{
            "theme": "dark",
            "soundVolume": 0.5,
            "activeProfile": null,
            "notificationDuration": 5000,
            "notificationStackDirection": "up",
            "notificationFontSize": 14,
            "notificationOpacity": 0.9,
            "notificationX": 1.0,
            "notificationY": 1.0,
            "compactName": false,
            "toggleWindowHotkey": {"keyCode": 0, "modifiers": 0, "display": "None"},
            "editOverlayHotkey": {"keyCode": 0, "modifiers": 3, "display": "Ctrl+Alt"},
            "revealHiddenHotkey": {"keyCode": 90, "modifiers": 0, "display": "Z"},
            "lootHistoryHotkey": {"keyCode": 78, "modifiers": 0, "display": "N"},
            "verboseFilterLogging": false,
            "autoAlwaysShowItems": true
        }"#;
        let settings: AppSettings = serde_json::from_str(json).expect("valid legacy json");
        assert_eq!(settings.sounds.len(), 7);
        for (i, slot) in settings.sounds.iter().enumerate() {
            assert_eq!(slot.label, format!("Sound {}", i + 1));
            assert_eq!(slot.volume, 0.8);
            assert!(matches!(slot.source, SoundSource::Default));
        }
        assert_eq!(settings.sound_volume, 0.5);
    }

    #[test]
    fn sound_source_round_trips_each_variant() {
        let slots = vec![
            SoundSlot { label: "Default".into(), volume: 0.8, source: SoundSource::Default },
            SoundSlot {
                label: "Custom".into(),
                volume: 0.5,
                source: SoundSource::Custom { file_name: "slot-8.mp3".into() },
            },
            SoundSlot { label: "Empty".into(), volume: 0.0, source: SoundSource::Empty },
        ];
        let json = serde_json::to_string(&slots).unwrap();
        let back: Vec<SoundSlot> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 3);
        assert!(matches!(back[0].source, SoundSource::Default));
        match &back[1].source {
            SoundSource::Custom { file_name } => assert_eq!(file_name, "slot-8.mp3"),
            other => panic!("expected Custom, got {:?}", other),
        }
        assert!(matches!(back[2].source, SoundSource::Empty));
    }

    #[test]
    fn sound_source_custom_uses_camel_case_on_wire() {
        let slot = SoundSlot {
            label: "Custom".into(),
            volume: 0.5,
            source: SoundSource::Custom { file_name: "slot-8.mp3".into() },
        };
        let json = serde_json::to_string(&slot).unwrap();
        // The wire format MUST use camelCase `fileName`, otherwise the JS
        // frontend (which sends `fileName`) cannot round-trip through Tauri.
        assert!(
            json.contains("\"fileName\":\"slot-8.mp3\""),
            "expected camelCase fileName on the wire, got {}",
            json
        );
        assert!(
            !json.contains("file_name"),
            "snake_case file_name should not be on the wire, got {}",
            json
        );
    }

    #[test]
    fn sound_source_deserialises_camel_case_payload_from_frontend() {
        // Exact JSON shape that `SoundsTab.svelte` sends through Tauri's
        // `save_settings` command. If this fails, the Tauri command rejects
        // the args before the Rust handler runs, and the slot's Custom state
        // never makes it to disk.
        let json = r#"{
            "label": "Custom",
            "volume": 0.5,
            "source": { "kind": "custom", "fileName": "slot-8.mp3" }
        }"#;
        let slot: SoundSlot = serde_json::from_str(json).expect("frontend payload must deserialise");
        match slot.source {
            SoundSource::Custom { file_name } => assert_eq!(file_name, "slot-8.mp3"),
            other => panic!("expected Custom, got {:?}", other),
        }
    }
}
