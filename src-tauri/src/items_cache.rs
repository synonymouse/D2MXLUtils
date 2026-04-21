//! Disk mirror of the items dictionary so autocomplete works before D2 attach.

use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};
use crate::notifier::ItemsDictionary;

const CACHE_FILE: &str = "items-cache.json";

const SCHEMA_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
struct ItemsCacheFile {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    base_types: Vec<String>,
    #[serde(default)]
    uniques_tu: Vec<String>,
    #[serde(default)]
    uniques_su: Vec<String>,
    #[serde(default)]
    uniques_ssu: Vec<String>,
    #[serde(default)]
    uniques_sssu: Vec<String>,
    #[serde(default)]
    set_items: Vec<String>,
    dumped_at: String,
}

pub fn load_items_cache(app: &AppHandle) -> Option<ItemsDictionary> {
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
                "items cache: loaded {} base + {} TU + {} SU + {} SSU + {} SSSU + {} set (dumped at {})",
                cache.base_types.len(),
                cache.uniques_tu.len(),
                cache.uniques_su.len(),
                cache.uniques_ssu.len(),
                cache.uniques_sssu.len(),
                cache.set_items.len(),
                cache.dumped_at
            ));
            Some(ItemsDictionary {
                base_types: cache.base_types,
                uniques_tu: cache.uniques_tu,
                uniques_su: cache.uniques_su,
                uniques_ssu: cache.uniques_ssu,
                uniques_sssu: cache.uniques_sssu,
                set_items: cache.set_items,
            })
        }
        Err(e) => {
            log_error(&format!("items cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_items_cache(app: &AppHandle, dict: &ItemsDictionary) -> Result<(), String> {
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
        base_types: dict.base_types.clone(),
        uniques_tu: dict.uniques_tu.clone(),
        uniques_su: dict.uniques_su.clone(),
        uniques_ssu: dict.uniques_ssu.clone(),
        uniques_sssu: dict.uniques_sssu.clone(),
        set_items: dict.set_items.clone(),
        dumped_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize items cache: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write items-cache.json: {}", e))?;
    log_info(&format!(
        "items cache: wrote {} base + {} TU + {} SU + {} SSU + {} SSSU + {} set entries to {}",
        dict.base_types.len(),
        dict.uniques_tu.len(),
        dict.uniques_su.len(),
        dict.uniques_ssu.len(),
        dict.uniques_sssu.len(),
        dict.set_items.len(),
        path.display()
    ));
    Ok(())
}
