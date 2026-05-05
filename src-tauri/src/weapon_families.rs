//! On-disk catalog of every weapon record in items.txt with its family chain
//! and WSM. Built once per game attach (a single pass over items.txt while
//! the injector is alive to resolve names via D2Lang.GetStringById), cached
//! to `weapon-bases.json` so the catalog survives offline launches and
//! powers the breakpoint calculator's manual-mode dropdowns.

use std::fs;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::breakpoints::{resolve_item_type_chain, u32_to_packed_code};
use crate::injection::D2Injector;
use crate::logger::{error as log_error, info as log_info};
use crate::notifier::strip_color_codes;
use crate::offsets::{d2common, items_txt};
use crate::process::D2Context;

const CACHE_FILE: &str = "weapon-bases.json";
const SCHEMA_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponBase {
    pub file_index: u32,
    pub name: String,
    pub wclass: String,
    pub wsm: i32,
    /// Chain of 4-char ItemTypes codes from `items.txt[fidx].wType[0]` walked
    /// up via `equiv1` (most specific first, e.g.
    /// `["qaxe", "axe", "mele", "weap"]`). The frontend matches this against
    /// its WEAPON_TYPES tokens to group bases under their site-style family.
    pub family_codes: Vec<String>,
}

pub type WeaponBaseCatalog = Vec<WeaponBase>;

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    schema: String,
    bases: WeaponBaseCatalog,
    dumped_at: String,
}

pub fn load_from_cache(app: &AppHandle) -> Option<WeaponBaseCatalog> {
    let app_data = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log_error(&format!(
                "weapon-bases cache: failed to resolve app data directory: {}",
                e
            ));
            return None;
        }
    };

    let path = app_data.join(CACHE_FILE);
    if !path.exists() {
        log_info(&format!("weapon-bases cache: no file at {}", path.display()));
        return None;
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("weapon-bases cache: read failed: {}", e));
            return None;
        }
    };

    match serde_json::from_str::<CacheFile>(&content) {
        Ok(file) => {
            if file.schema != SCHEMA_VERSION {
                log_info(&format!(
                    "weapon-bases cache: schema mismatch (file={:?}, app={:?}), ignoring",
                    file.schema, SCHEMA_VERSION
                ));
                return None;
            }
            log_info(&format!(
                "weapon-bases cache: loaded {} entries (dumped at {})",
                file.bases.len(),
                file.dumped_at
            ));
            Some(file.bases)
        }
        Err(e) => {
            log_error(&format!("weapon-bases cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_to_cache(app: &AppHandle, catalog: &WeaponBaseCatalog) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    if !app_data.exists() {
        fs::create_dir_all(&app_data)
            .map_err(|e| format!("Failed to create app data directory: {}", e))?;
    }

    let path = app_data.join(CACHE_FILE);
    let payload = CacheFile {
        schema: SCHEMA_VERSION.to_string(),
        bases: catalog.clone(),
        dumped_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize weapon-bases cache: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write weapon-bases.json: {}", e))?;
    log_info(&format!(
        "weapon-bases cache: wrote {} entries to {}",
        catalog.len(),
        path.display()
    ));
    Ok(())
}

/// Walks all items.txt records, keeps weapons (`wclass != 0`), resolves
/// each one's family chain, WSM, and base name (via injected
/// `D2Lang.GetStringById`). Single pass; takes a few seconds depending on
/// how many name lookups MXL needs (~500-1500 weapons typically).
pub fn build_catalog(ctx: &D2Context, injector: &D2Injector) -> Result<WeaponBaseCatalog, String> {
    let count_addr = ctx.d2_common + d2common::ITEMS_TXT_COUNT;
    let ptr_addr = ctx.d2_common + d2common::ITEMS_TXT;

    let count = ctx
        .process
        .read_memory::<u32>(count_addr)
        .map_err(|e| format!("read items.txt count: {}", e))? as usize;
    let base_ptr = ctx
        .process
        .read_memory::<u32>(ptr_addr)
        .map_err(|e| format!("read items.txt ptr: {}", e))? as usize;

    if count == 0 || base_ptr == 0 {
        return Err(format!(
            "items.txt not available (count={}, ptr=0x{:X})",
            count, base_ptr
        ));
    }

    let mut catalog = Vec::with_capacity(512);

    for fidx in 0..count {
        let record = base_ptr + fidx * items_txt::RECORD_SIZE;

        // Skip non-weapon records cheaply (no remote-thread call needed).
        let wclass_raw = ctx
            .process
            .read_memory::<u32>(record + items_txt::WCLASS)
            .unwrap_or(0);
        if wclass_raw == 0 {
            continue;
        }

        let wclass = u32_to_packed_code(wclass_raw);
        let wsm = ctx
            .process
            .read_memory::<i32>(record + items_txt::SPEED)
            .unwrap_or(0);
        let type0 = ctx
            .process
            .read_memory::<u16>(record + items_txt::TYPE_0)
            .unwrap_or(0);
        let family_codes = resolve_item_type_chain(ctx, type0);

        let name_id = ctx
            .process
            .read_memory::<u16>(record + items_txt::NAME_ID)
            .unwrap_or(0);
        let raw_name = if name_id != 0 {
            injector
                .get_string(&ctx.process, name_id, 100)
                .map(|s| strip_color_codes(&s))
                .unwrap_or_default()
        } else {
            String::new()
        };
        // Items.txt name strings often span multiple lines (category prefix
        // + base name); the last non-empty line is the actual base name.
        let name = raw_name
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .last()
            .map(String::from)
            .unwrap_or_default();

        catalog.push(WeaponBase {
            file_index: fidx as u32,
            name,
            wclass,
            wsm,
            family_codes,
        });
    }

    log_info(&format!(
        "weapon-bases: built catalog with {} entries from {} items.txt records",
        catalog.len(),
        count
    ));
    Ok(catalog)
}
