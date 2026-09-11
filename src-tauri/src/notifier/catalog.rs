//! Matching-cache persistence, live item catalogs and dictionary snapshots.

use super::{
    classify_unique_kind, strip_color_codes, ClassInfo, DropScanner, ItemsDictionary,
    MatchingCache, UniqueInfo, UniqueKind,
};
use crate::d2types::ScannedItem;
use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{
    d2common, data_tables, item_quality, items_txt, set_items_txt, unique_items_txt,
};
use crate::rules::ItemTier;
use std::collections::HashSet;
use tauri::{AppHandle, Manager};

const MATCHING_CACHE_FILE: &str = "matching-cache.json";
const MATCHING_CACHE_SCHEMA_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MatchingCacheFile {
    schema: String,
    cache: MatchingCache,
    dumped_at: String,
}

/// Mirrors `breakpoints::load_weapon_base_cache`'s pattern (schema-versioned
/// JSON in the app data dir, `None` on any miss/mismatch so the caller
/// falls back to a live rebuild).
pub fn load_matching_cache(app: &AppHandle) -> Option<MatchingCache> {
    let app_data = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log_error(&format!(
                "matching cache: failed to resolve app data directory: {}",
                e
            ));
            return None;
        }
    };

    let path = app_data.join(MATCHING_CACHE_FILE);
    if !path.exists() {
        log_info(&format!("matching cache: no file at {}", path.display()));
        return None;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("matching cache: read failed: {}", e));
            return None;
        }
    };

    match serde_json::from_str::<MatchingCacheFile>(&content) {
        Ok(file) => {
            if file.schema != MATCHING_CACHE_SCHEMA_VERSION {
                log_info(&format!(
                    "matching cache: schema mismatch (file={:?}, app={:?}), ignoring",
                    file.schema, MATCHING_CACHE_SCHEMA_VERSION
                ));
                return None;
            }
            log_info(&format!(
                "matching cache: loaded {} classes + {} uniques + {} set items (dumped at {})",
                file.cache.class_cache.len(),
                file.cache.unique_cache.len(),
                file.cache.set_cache.len(),
                file.dumped_at
            ));
            Some(file.cache)
        }
        Err(e) => {
            log_error(&format!("matching cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_matching_cache(app: &AppHandle, cache: &MatchingCache) -> Result<(), String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    if !app_data.exists() {
        std::fs::create_dir_all(&app_data)
            .map_err(|e| format!("Failed to create app data directory: {}", e))?;
    }

    let path = app_data.join(MATCHING_CACHE_FILE);
    let payload = MatchingCacheFile {
        schema: MATCHING_CACHE_SCHEMA_VERSION.to_string(),
        cache: cache.clone(),
        dumped_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("Failed to serialize matching cache: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write matching-cache.json: {}", e))?;
    log_info(&format!(
        "matching cache: wrote {} classes + {} uniques + {} set items to {}",
        cache.class_cache.len(),
        cache.unique_cache.len(),
        cache.set_cache.len(),
        path.display()
    ));
    Ok(())
}

impl DropScanner {
    pub(super) fn unique_kind(&self, file_index: u32, class: u32) -> Option<UniqueKind> {
        let from_wlvl = self
            .unique_cache
            .as_ref()
            .and_then(|cache| cache.get(file_index as usize))
            .and_then(|info| info.kind);
        classify_unique_kind(from_wlvl, self.class_tier(class))
    }

    fn unique_display_name(&self, file_index: u32) -> Option<String> {
        self.unique_cache
            .as_ref()
            .and_then(|cache| cache.get(file_index as usize))
            .map(|info| info.display_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }

    fn set_display_name(&self, file_index: u32) -> Option<String> {
        self.set_cache
            .as_ref()
            .and_then(|cache| cache.get(file_index as usize))
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }

    pub(super) fn static_display_name(&self, scanned: &ScannedItem, base_name: &str) -> String {
        match scanned.quality {
            item_quality::UNIQUE => self.unique_display_name(scanned.file_index),
            item_quality::SET => self.set_display_name(scanned.file_index),
            _ => None,
        }
        .or_else(|| {
            if base_name.is_empty() {
                None
            } else {
                Some(base_name.to_string())
            }
        })
        .unwrap_or_else(|| format!("Item #{}", scanned.class))
    }

    pub(super) fn class_tier(&self, class: u32) -> Option<ItemTier> {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .map(|info| info.tier)
    }

    pub(super) fn class_base_name(&self, class: u32) -> String {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .map(|info| info.base_name.clone())
            .unwrap_or_default()
    }

    pub(super) fn class_category(&self, class: u32) -> Option<String> {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .and_then(|info| info.category.clone())
    }

    /// Seed the live matching caches from a previously-saved
    /// `MatchingCache` (see `load_matching_cache`) so `tick_items`'s
    /// lazy-build-on-first-tick logic (`if self.class_cache.is_none()`)
    /// skips the expensive live rebuild entirely. Only takes effect right
    /// after construction — `tick_items` never re-checks once populated.
    pub fn seed_matching_cache(&mut self, cache: MatchingCache) {
        self.class_cache = Some(cache.class_cache);
        self.unique_cache = Some(cache.unique_cache);
        self.set_cache = Some(cache.set_cache);
    }

    /// Drop the live matching caches so the next `tick_items` call rebuilds
    /// them from current game memory (see `if self.class_cache.is_none()`).
    /// Used by the manual "refresh game data" command to recover from a
    /// stale on-disk cache (e.g. after an MXL content patch) without
    /// restarting the app.
    pub fn clear_matching_cache(&mut self) {
        self.class_cache = None;
        self.unique_cache = None;
        self.set_cache = None;
    }

    /// Snapshot of the live matching caches for persistence, once all
    /// three have been populated (either seeded from disk or freshly
    /// built). `None` while any is still missing.
    pub fn matching_cache_snapshot(&self) -> Option<MatchingCache> {
        Some(MatchingCache {
            class_cache: self.class_cache.clone()?,
            unique_cache: self.unique_cache.clone()?,
            set_cache: self.set_cache.clone()?,
        })
    }

    pub fn items_dictionary_snapshot(&self) -> Option<ItemsDictionary> {
        let class_cache = self.class_cache.as_ref()?;
        let unique_cache = self.unique_cache.as_ref()?;
        let set_cache = self.set_cache.as_ref()?;

        let word_tier =
            regex::Regex::new(r"(?i)\s*\((?:Sacred|Angelic|Mastercrafted)\)\s*$").ok()?;
        let count_suffix = regex::Regex::new(r"\s*\(\d+\)\s*$").ok()?;
        // Keep "X Container (NN)" intact — the number identifies the rune.
        let rune_container = regex::Regex::new(r"(?i)\bContainer\s*\(\d+\)\s*$").ok()?;
        let mut base_types: Vec<String> = class_cache
            .iter()
            .map(|info| {
                let n = word_tier.replace(&info.base_name, "");
                if rune_container.is_match(&n) {
                    n.into_owned()
                } else {
                    count_suffix.replace(&n, "").into_owned()
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
        base_types.sort();
        base_types.dedup();

        // On name collision keep the highest kind (Sssu > Ssu > Su > Tu)
        // so the strongest tier of a multi-record unique survives dedup.
        let mut kind_by_name: std::collections::HashMap<String, UniqueKind> =
            std::collections::HashMap::new();
        for info in unique_cache {
            let kind = match info.kind {
                Some(k) => k,
                None => continue,
            };
            if info.display_name.is_empty() {
                continue;
            }
            kind_by_name
                .entry(info.display_name.clone())
                .and_modify(|k| *k = (*k).max(kind))
                .or_insert(kind);
        }

        // Drop uniques that also live in base_types — MXL charms
        // (e.g. "The Butcher's Tooth", "Azmodan's Heart") are indexed
        // in both tables; keep them on the base side only.
        let base_set: HashSet<&str> = base_types.iter().map(String::as_str).collect();
        let mut uniques_tu: Vec<String> = Vec::new();
        let mut uniques_su: Vec<String> = Vec::new();
        let mut uniques_ssu: Vec<String> = Vec::new();
        let mut uniques_sssu: Vec<String> = Vec::new();
        for (name, kind) in kind_by_name {
            if base_set.contains(name.as_str()) {
                continue;
            }
            match kind {
                UniqueKind::Tu => uniques_tu.push(name),
                UniqueKind::Su => uniques_su.push(name),
                UniqueKind::Ssu => uniques_ssu.push(name),
                UniqueKind::Sssu => uniques_sssu.push(name),
            }
        }
        uniques_tu.sort();
        uniques_su.sort();
        uniques_ssu.sort();
        uniques_sssu.sort();

        let mut set_items: Vec<String> = set_cache
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect();
        set_items.sort();
        set_items.dedup();

        Some(ItemsDictionary {
            base_types,
            uniques_tu,
            uniques_su,
            uniques_ssu,
            uniques_sssu,
            set_items,
        })
    }

    /// Port of `NotifierCache` in D2Stats.au3 (lines 697-750).
    pub(super) fn build_class_cache(&self) -> Result<Vec<ClassInfo>, String> {
        let count_addr = self.state.ctx.d2_common + d2common::ITEMS_TXT_COUNT;
        let ptr_addr = self.state.ctx.d2_common + d2common::ITEMS_TXT;

        let count = self.state.ctx.process.read_memory::<u32>(count_addr)? as usize;
        let base_ptr = self.state.ctx.process.read_memory::<u32>(ptr_addr)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "items.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let re = regex::Regex::new(r"(?i)\(Sacred\)|\(Angelic\)|\(Mastercrafted\)|[1-4]")
            .map_err(|e| format!("tier regex compile failed: {}", e))?;

        let mut cache = Vec::with_capacity(count);
        let injector = self.state.injector.lock().unwrap();

        for class in 0..count {
            let record = base_ptr + class * items_txt::RECORD_SIZE;

            // MISC != 0 → weapon or armor (tier-eligible).
            let misc = self
                .state
                .ctx
                .process
                .read_memory::<u32>(record + items_txt::MISC)
                .unwrap_or(0);

            let name_id = self
                .state
                .ctx
                .process
                .read_memory::<u16>(record + items_txt::NAME_ID)
                .unwrap_or(0);

            let raw_name = match injector.get_string(&self.state.ctx.process, name_id, 100) {
                Ok(s) => strip_color_codes(&s),
                Err(_) => {
                    cache.push(ClassInfo {
                        base_name: String::new(),
                        category: None,
                        tier: ItemTier::Tier0,
                    });
                    continue;
                }
            };

            let mut non_empty_lines: Vec<&str> = raw_name
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let base_name = non_empty_lines
                .pop()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let category = if non_empty_lines.is_empty() {
                None
            } else {
                Some(non_empty_lines.join("\n"))
            };

            let tier = if misc == 0 {
                ItemTier::Tier0
            } else {
                match re.find(&raw_name) {
                    Some(m) => match m.as_str().to_ascii_lowercase().as_str() {
                        "(sacred)" => ItemTier::Sacred,
                        "(angelic)" => ItemTier::Angelic,
                        "(mastercrafted)" => ItemTier::Master,
                        "1" => ItemTier::Tier1,
                        "2" => ItemTier::Tier2,
                        "3" => ItemTier::Tier3,
                        "4" => ItemTier::Tier4,
                        _ => ItemTier::Tier0,
                    },
                    None => ItemTier::Tier0,
                }
            };

            cache.push(ClassInfo {
                base_name,
                category,
                tier,
            });
        }

        Ok(cache)
    }

    pub(super) fn build_unique_items_cache(&self) -> Result<Vec<UniqueInfo>, String> {
        let sgpt = self
            .state
            .ctx
            .process
            .read_memory::<u32>(self.state.ctx.d2_common + d2common::SGPT_DATA_TABLES)?
            as usize;
        if sgpt == 0 {
            return Err("sgptDataTables is NULL".into());
        }

        let count = self
            .state
            .ctx
            .process
            .read_memory::<u32>(sgpt + data_tables::UNIQUE_ITEMS_TXT_COUNT)?
            as usize;
        let base_ptr =
            self.state
                .ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::UNIQUE_ITEMS_TXT_PTR)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "UniqueItems.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let mut cache = Vec::with_capacity(count);
        let injector = self.state.injector.lock().unwrap();

        // Push exactly one UniqueInfo per UniqueItems.txt record so that
        // runtime lookup by `ItemData.file_index` stays O(1).
        for i in 0..count {
            let record = base_ptr + i * unique_items_txt::RECORD_SIZE;

            let name_id = self
                .state
                .ctx
                .process
                .read_memory::<u16>(record + unique_items_txt::NAME_ID)
                .unwrap_or(0);
            let wlvl = self
                .state
                .ctx
                .process
                .read_memory::<u16>(record + unique_items_txt::LEVEL)
                .unwrap_or(0);

            let display_name = injector
                .get_string(&self.state.ctx.process, name_id, 200)
                .map(|s| strip_color_codes(&s).trim().to_string())
                .unwrap_or_default();

            cache.push(UniqueInfo {
                display_name,
                kind: UniqueKind::from_wlvl(wlvl),
            });
        }

        Ok(cache)
    }

    pub(super) fn build_set_items_cache(&self) -> Result<Vec<String>, String> {
        let sgpt = self
            .state
            .ctx
            .process
            .read_memory::<u32>(self.state.ctx.d2_common + d2common::SGPT_DATA_TABLES)?
            as usize;
        if sgpt == 0 {
            return Err("sgptDataTables is NULL".into());
        }

        let count =
            self.state
                .ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::SET_ITEMS_TXT_COUNT)? as usize;
        let base_ptr =
            self.state
                .ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::SET_ITEMS_TXT_PTR)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "SetItems.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let injector = self.state.injector.lock().unwrap();
        let mut cache = Vec::with_capacity(count);
        for i in 0..count {
            let record = base_ptr + i * set_items_txt::RECORD_SIZE;
            let name = self
                .state
                .ctx
                .process
                .read_memory::<u16>(record + set_items_txt::NAME_ID)
                .ok()
                .and_then(|name_id| {
                    injector
                        .get_string(&self.state.ctx.process, name_id, 200)
                        .ok()
                })
                .map(|s| strip_color_codes(&s).trim().to_string())
                .unwrap_or_default();
            cache.push(name);
        }

        Ok(cache)
    }
}
