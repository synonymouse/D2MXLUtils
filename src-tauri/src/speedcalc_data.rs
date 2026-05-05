use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::logger::{error as log_error, info as log_info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimEntry {
    pub frames: u16,
    pub anim_speed: u16,
}

pub type SpeedcalcTable = HashMap<String, AnimEntry>;

const SPEEDCALC_URL: &str = "https://dev.median-xl.com/speedcalc/SpeedcalcData.txt";
const CACHE_FILE: &str = "speedcalc-data.json";

pub fn parse_tsv(raw: &str) -> SpeedcalcTable {
    let mut table = HashMap::new();
    for line in raw.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let cof_name = parts[0].to_string();
        let frames = match parts[1].parse::<u16>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let anim_speed = match parts[2].parse::<u16>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        table.insert(cof_name, AnimEntry { frames, anim_speed });
    }
    table
}

pub fn fetch_from_site() -> Result<String, String> {
    let response = ureq::get(SPEEDCALC_URL)
        .call()
        .map_err(|e| format!("Failed to fetch SpeedcalcData.txt: {}", e))?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    Ok(body)
}

pub fn load_from_cache(app_data_dir: &PathBuf) -> Option<SpeedcalcTable> {
    let path = app_data_dir.join(CACHE_FILE);
    if !path.exists() {
        return None;
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("speedcalc cache: read failed: {}", e));
            return None;
        }
    };
    match serde_json::from_str::<SpeedcalcTable>(&content) {
        Ok(table) => {
            log_info(&format!(
                "speedcalc cache: loaded {} entries from {}",
                table.len(),
                path.display()
            ));
            Some(table)
        }
        Err(e) => {
            log_error(&format!("speedcalc cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_to_cache(app_data_dir: &PathBuf, table: &SpeedcalcTable) -> Result<(), String> {
    if !app_data_dir.exists() {
        fs::create_dir_all(app_data_dir)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    let path = app_data_dir.join(CACHE_FILE);
    let json = serde_json::to_string(table)
        .map_err(|e| format!("Failed to serialize speedcalc data: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write speedcalc cache: {}", e))?;
    log_info(&format!(
        "speedcalc cache: wrote {} entries to {}",
        table.len(),
        path.display()
    ));
    Ok(())
}

pub fn fetch_and_cache(app_data_dir: &PathBuf) -> Result<SpeedcalcTable, String> {
    let raw = fetch_from_site()?;
    let table = parse_tsv(&raw);
    if table.is_empty() {
        return Err("Parsed SpeedcalcData.txt but got 0 entries".to_string());
    }
    if let Err(e) = save_to_cache(app_data_dir, &table) {
        log_error(&format!("speedcalc: cache save failed (non-fatal): {}", e));
    }
    Ok(table)
}
