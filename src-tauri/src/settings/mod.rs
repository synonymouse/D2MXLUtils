//! Settings persistence module for D2MXLUtils
//!
//! Handles loading and saving application settings using tauri-plugin-store.
//! Settings are stored in a JSON file in the app's data directory.

mod model;

// Preserve the existing settings type paths, including types without direct consumers.
#[allow(unused_imports)]
pub use model::{
    AppSettings, DpsMeterSettings, SoundSlot, SoundSource, WidgetPosition, WindowState,
};

use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;

use crate::logger::{error as log_error, info as log_info};

const SETTINGS_FILE: &str = "settings.json";

/// Load application settings from the store
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let store = app
        .store(SETTINGS_FILE)
        .map_err(|e| format!("Failed to open settings store: {}", e))?;

    let Some(raw) = store.get("settings") else {
        log_info("No settings found, using defaults");
        return Ok(AppSettings::default());
    };

    let mut settings: AppSettings = serde_json::from_value(raw.clone()).unwrap_or_else(|e| {
        log_error(&format!("Failed to parse settings, using defaults: {}", e));
        AppSettings::default()
    });

    if crate::migrations::migrate(&raw, &mut settings) {
        let value = serde_json::to_value(&settings)
            .map_err(|e| format!("Failed to serialize migrated settings: {}", e))?;
        store.set("settings", value);
        store
            .save()
            .map_err(|e| format!("Failed to save migrated settings: {}", e))?;
        log_info("Settings migrated and re-saved");
    }

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
mod tests;
