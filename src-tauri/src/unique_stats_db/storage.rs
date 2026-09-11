use std::fs;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};

use super::UniqueStatsDb;

const DB_FILE: &str = "unique-stats-db.json";

#[derive(Debug, Deserialize)]
struct UniqueStatsEntry {
    name: String,
    stats: String,
}

#[derive(Debug, Deserialize)]
struct UniqueStatsDbFile {
    entries: Vec<UniqueStatsEntry>,
}

pub fn load_unique_stats_db(app: &AppHandle) -> Option<UniqueStatsDb> {
    let app_data = app.path().app_data_dir().ok()?;
    let path = app_data.join(DB_FILE);
    if !path.exists() {
        log_info(&format!("unique stats db: no file at {}", path.display()));
        return None;
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("unique stats db: read failed: {}", e));
            return None;
        }
    };
    match serde_json::from_str::<UniqueStatsDbFile>(&content) {
        Ok(file) => {
            let map: UniqueStatsDb = file
                .entries
                .into_iter()
                .map(|e| (e.name, e.stats))
                .collect();
            log_info(&format!("unique stats db: loaded {} entries", map.len()));
            Some(map)
        }
        Err(e) => {
            log_error(&format!("unique stats db: parse failed: {}", e));
            None
        }
    }
}
