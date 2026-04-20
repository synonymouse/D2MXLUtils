//! Disk mirror of the items dictionary so autocomplete works before D2 attach.

use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};

const CACHE_FILE: &str = "items-cache.json";

const SCHEMA_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
struct ItemsCacheFile {
    #[serde(default)]
    schema: String,
    base_types: Vec<String>,
    dumped_at: String,
}

pub fn load_items_cache(app: &AppHandle) -> Option<Vec<String>> {
    let app_data = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log_error(&format!(
                "items cache: failed to resolve app data directory: {}",
                e
            ));
            return None;
        }
    };

    let path = app_data.join(CACHE_FILE);
    if !path.exists() {
        log_info(&format!("items cache: no file at {}", path.display()));
        return None;
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("items cache: read failed: {}", e));
            return None;
        }
    };

    match serde_json::from_str::<ItemsCacheFile>(&content) {
        Ok(cache) => {
            if cache.schema != SCHEMA_VERSION {
                log_info(&format!(
                    "items cache: schema mismatch (file={:?}, app={:?}), ignoring",
                    cache.schema, SCHEMA_VERSION
                ));
                return None;
            }
            log_info(&format!(
                "items cache: loaded {} entries (dumped at {})",
                cache.base_types.len(),
                cache.dumped_at
            ));
            Some(cache.base_types)
        }
        Err(e) => {
            log_error(&format!("items cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_items_cache(app: &AppHandle, items: &[String]) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    if !app_data.exists() {
        fs::create_dir_all(&app_data)
            .map_err(|e| format!("Failed to create app data directory: {}", e))?;
    }

    let path = app_data.join(CACHE_FILE);
    let payload = ItemsCacheFile {
        schema: SCHEMA_VERSION.to_string(),
        base_types: items.to_vec(),
        dumped_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize items cache: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write items-cache.json: {}", e))?;
    log_info(&format!(
        "items cache: wrote {} entries to {}",
        items.len(),
        path.display()
    ));
    Ok(())
}
