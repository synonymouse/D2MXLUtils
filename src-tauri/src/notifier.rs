//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

#[cfg(target_os = "windows")]
use crate::d2types::{ItemData, ScannedItem, UnitAny};
#[cfg(target_os = "windows")]
use crate::injection::D2Injector;
#[cfg(target_os = "windows")]
use crate::logger::{error as log_error, info as log_info};
#[cfg(target_os = "windows")]
use crate::loot_filter_hook::LootFilterHook;
#[cfg(target_os = "windows")]
use crate::offsets::{
    d2client, d2common, data_tables, item_quality, items_txt, paths, set_items_txt,
    unique_items_txt, unit_type,
};
#[cfg(target_os = "windows")]
use crate::process::D2Context;
#[cfg(target_os = "windows")]
use crate::rules::{FilterConfig, MatchContext, Visibility};
use crate::rules::{ItemTier, Notification};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ItemDropEvent {
    pub unit_id: u32,
    pub class: u32,
    pub quality: String,
    pub name: String,
    #[serde(default)]
    pub base_name: String,
    pub stats: String,
    pub is_ethereal: bool,
    pub is_identified: bool,
    pub p_unit_data: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ItemTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_kind: Option<UniqueKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Notification>,
}

/// Drop scanner that iterates through ground items
#[cfg(target_os = "windows")]
pub struct DropScanner {
    /// D2 context with process handle and DLL bases
    ctx: D2Context,
    /// Injector for calling game functions
    injector: D2Injector,
    /// Cache of already-seen item IDs (to avoid duplicate notifications)
    seen_items: HashSet<u32>,
    /// Optional filter config for automatic item filtering
    filter_config: Option<Arc<RwLock<FilterConfig>>>,
    /// Whether automatic filtering is enabled
    filter_enabled: bool,
    /// Loot filter hook for D2Sigma.dll
    loot_hook: LootFilterHook,
    /// Indexed by `UnitAny.class`. Built lazily on first in-game tick.
    class_cache: Option<Vec<ClassInfo>>,
    unique_cache: Option<Vec<UniqueInfo>>,
    set_cache: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct ClassInfo {
    base_name: String,
    tier: ItemTier,
}

/// Sacred unique tier buckets, classified by UniqueItems.txt `wLvl`.
/// Bands below match D2Stats.au3:1181-1191 except the `Sssu` upper
/// bound is removed — MXL has SSSU items up to at least wLvl 139
/// (e.g. amulets), and D2Stats' `<= 130` cap mislabeled them.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum UniqueKind {
    Tu = 0,   // wLvl 2..=100
    Su = 1,   // wLvl 101..=115
    Ssu = 2,  // wLvl 116..=120
    Sssu = 3, // wLvl 121..
}

impl UniqueKind {
    fn from_wlvl(wlvl: u16) -> Option<Self> {
        match wlvl {
            2..=100 => Some(UniqueKind::Tu),
            101..=115 => Some(UniqueKind::Su),
            116..=120 => Some(UniqueKind::Ssu),
            121.. => Some(UniqueKind::Sssu),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UniqueKind::Tu => "TU",
            UniqueKind::Su => "SU",
            UniqueKind::Ssu => "SSU",
            UniqueKind::Sssu => "SSSU",
        }
    }
}

/// Resolve a unique's tier label combining wLvl banding and base-item tier.
///
/// MXL stores `wLvl = 1` for many low-tier uniques (e.g. Razordisk on a
/// Tier1 Buckler). When wLvl alone yields no band, fall back to the base
/// item tier: a normal-tier base (Tier1-4) means TU.
fn classify_unique_kind(
    from_wlvl: Option<UniqueKind>,
    base_tier: Option<ItemTier>,
) -> Option<UniqueKind> {
    if from_wlvl.is_some() {
        return from_wlvl;
    }
    match base_tier? {
        ItemTier::Tier1 | ItemTier::Tier2 | ItemTier::Tier3 | ItemTier::Tier4 => {
            Some(UniqueKind::Tu)
        }
        _ => None,
    }
}

/// One entry per UniqueItems.txt record (aligned 1:1 with `file_index`
/// read from `ItemData`). `kind = None` marks records with wLvl ∈ {0, 1};
/// at drop time `classify_unique_kind` falls back to base item tier so
/// low-tier TUs (e.g. Razordisk on Tier1 Buckler) still get the TU label.
/// `display_name.is_empty()` marks failed `GetStringById` resolution;
/// such records are skipped in the autocomplete snapshot.
#[derive(Debug, Clone)]
struct UniqueInfo {
    display_name: String,
    kind: Option<UniqueKind>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ItemsDictionary {
    pub base_types: Vec<String>,
    pub uniques_tu: Vec<String>,
    pub uniques_su: Vec<String>,
    pub uniques_ssu: Vec<String>,
    pub uniques_sssu: Vec<String>,
    pub set_items: Vec<String>,
}

#[cfg(target_os = "windows")]
impl DropScanner {
    /// Create a new scanner attached to the D2 process
    pub fn new() -> Result<Self, String> {
        let ctx = D2Context::new()?;
        let injector = D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common, ctx.d2_lang)?;

        // Initialize and inject the loot filter hook
        let mut loot_hook = LootFilterHook::new();
        if ctx.d2_sigma != 0 {
            if let Err(e) = loot_hook.inject(&ctx) {
                log_error(&format!("Failed to inject LootFilterHook: {}", e));
            }
        }

        Ok(Self {
            ctx,
            injector,
            seen_items: HashSet::new(),
            filter_config: None,
            filter_enabled: false,
            loot_hook,
            class_cache: None,
            unique_cache: None,
            set_cache: None,
        })
    }

    pub fn set_filter_config(&mut self, config: Arc<RwLock<FilterConfig>>) {
        self.filter_config = Some(config);
    }

    pub fn on_filter_config_changed(&mut self) {
        self.clear_cache();
    }

    /// Enable or disable automatic filtering
    pub fn set_filter_enabled(&mut self, enabled: bool) {
        if self.filter_enabled == enabled {
            return; // No change
        }

        self.filter_enabled = enabled;

        // Sync with the loot filter hook
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.set_filter_enabled(&self.ctx, enabled) {
                log_error(&format!("Failed to set hook filter_enabled: {}", e));
            }
        }
    }

    /// Check if filtering is enabled
    pub fn is_filter_enabled(&self) -> bool {
        self.filter_enabled && self.filter_config.is_some()
    }

    /// Check if filter config is set
    pub fn has_filter_config(&self) -> bool {
        self.filter_config.is_some()
    }

    /// Check if player is in game
    pub fn is_ingame(&self) -> bool {
        let player_unit_ptr = self.ctx.d2_client + d2client::PLAYER_UNIT;
        match self.ctx.process.read_memory::<u32>(player_unit_ptr) {
            Ok(ptr) => ptr != 0,
            Err(_) => false,
        }
    }

    pub fn clear_cache(&mut self) {
        self.seen_items.clear();
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.clear_hidden_items(&self.ctx) {
                log_error(&format!("Failed to clear hide mask: {}", e));
            }
            if let Err(e) = self.loot_hook.clear_shown_items(&self.ctx) {
                log_error(&format!("Failed to clear show mask: {}", e));
            }
            if let Err(e) = self.loot_hook.clear_inspected_mask(&self.ctx) {
                log_error(&format!("Failed to clear inspected mask: {}", e));
            }
        }
    }

    /// Get a reference to the D2Context
    pub fn context(&self) -> &D2Context {
        &self.ctx
    }

    /// Scan for ground items and return new items found
    pub fn tick(&mut self) -> Vec<ItemDropEvent> {
        let mut events = Vec::new();

        if !self.is_ingame() {
            return events;
        }

        if self.class_cache.is_none() {
            match self.build_class_cache() {
                Ok(cache) => {
                    log_info(&format!("Class cache built: {} classes", cache.len()));
                    self.class_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build class cache: {}", e));
                    // Install an empty cache so we don't keep retrying every tick.
                    self.class_cache = Some(Vec::new());
                }
            }
        }

        if self.unique_cache.is_none() {
            match self.build_unique_items_cache() {
                Ok(cache) => {
                    log_info(&format!("Unique cache built: {} records", cache.len()));
                    self.unique_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build unique cache: {}", e));
                    self.unique_cache = Some(Vec::new());
                }
            }
        }

        if self.set_cache.is_none() {
            match self.build_set_items_cache() {
                Ok(cache) => {
                    log_info(&format!("Set cache built: {} records", cache.len()));
                    self.set_cache = Some(cache);
                }
                Err(e) => {
                    log_error(&format!("Failed to build set cache: {}", e));
                    self.set_cache = Some(Vec::new());
                }
            }
        }

        // Read paths structure to iterate through rooms/units
        let base_ptr = self.ctx.d2_client + d2client::PLAYER_UNIT;

        // Follow pointer chain: [base] -> [+0x2C] -> [+0x1C] -> pPaths (at +0x0) and iPaths (at +0x24)
        let ptr1 = match self.ctx.process.read_memory::<u32>(base_ptr) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let ptr2 = match self
            .ctx
            .process
            .read_memory::<u32>(ptr1 + paths::TO_PATHS_PTR[1])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let ptr3 = match self
            .ctx
            .process
            .read_memory::<u32>(ptr2 + paths::TO_PATHS_PTR[2])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let p_paths = match self
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_PTR[3])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let i_paths = match self
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_COUNT[3])
        {
            Ok(p) => p as usize,
            _ => return events,
        };

        // Iterate through each path/room
        for i in 0..i_paths {
            let p_path = match self.ctx.process.read_memory::<u32>(p_paths + 4 * i) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };

            let mut p_unit = match self
                .ctx
                .process
                .read_memory::<u32>(p_path + paths::PATH_TO_UNIT)
            {
                Ok(p) if p != 0 => p,
                _ => continue,
            };

            // Iterate through units in this room
            while p_unit != 0 {
                if let Some(scanned) = self.scan_unit(p_unit) {
                    let event = self.to_event(scanned);
                    let unit_id = event.unit_id;

                    // Apply filter if enabled
                    let mut event = event;
                    let mut should_emit = true;
                    if self.filter_enabled {
                        if let Some(ref filter_arc) = self.filter_config {
                            if let Ok(filter) = filter_arc.read() {
                                let ctx = MatchContext::new(&event);
                                let decision = filter.decide(&ctx);

                                let winner = filter.rules.iter().rev().find(|r| ctx.matches(r));
                                let reason = match winner {
                                    Some(r) => format!(
                                        "winner={}",
                                        r.name_pattern.as_deref().unwrap_or("<any>")
                                    ),
                                    None => {
                                        format!("no rule matched (hide_all={})", filter.hide_all)
                                    }
                                };
                                let vis_label = match decision.visibility {
                                    Visibility::Show => "SHOW",
                                    Visibility::Hide => "HIDE",
                                    Visibility::Default => "DEFAULT",
                                };
                                log_info(&format!(
                                    "[Filter] \"{} {}\" ({}, class={}) -> {} notify={} | {}",
                                    event.name,
                                    event.base_name,
                                    event.quality,
                                    event.class,
                                    vis_label,
                                    decision.notification.is_some(),
                                    reason
                                ));

                                if self.loot_hook.is_injected() {
                                    match decision.visibility {
                                        Visibility::Show => {
                                            if let Err(e) = self
                                                .loot_hook
                                                .add_shown_unit_id(&self.ctx, event.unit_id)
                                            {
                                                log_error(&format!(
                                                    "Failed to force-show item {}: {}",
                                                    event.unit_id, e
                                                ));
                                            }
                                        }
                                        Visibility::Hide => {
                                            if let Err(e) = self
                                                .loot_hook
                                                .add_hidden_unit_id(&self.ctx, event.unit_id)
                                            {
                                                log_error(&format!(
                                                    "Failed to hide item {}: {}",
                                                    event.unit_id, e
                                                ));
                                            }
                                        }
                                        Visibility::Default => {}
                                    }
                                }

                                match decision.notification {
                                    Some(n) => event.filter = Some(n),
                                    None => should_emit = false,
                                }
                            }
                        }
                    }

                    if should_emit {
                        events.push(event);
                    }

                    // Must run AFTER show/hide bits: otherwise the game thread
                    // could see inspected=1 with no decision yet and fall through
                    // to MXL's default (= flash the label).
                    if self.loot_hook.is_injected() {
                        if let Err(e) = self.loot_hook.add_inspected_unit_id(&self.ctx, unit_id) {
                            log_error(&format!("Failed to mark item {} inspected: {}", unit_id, e));
                        }
                    }
                }

                // Move to next unit (use struct layout for safety instead of hardcoded offset)
                let unit: UnitAny = match self.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };
                p_unit = unit.p_next_unit;
            }
        }

        events
    }

    /// Process a single unit, returning a fully scanned item if it's a new item.
    fn scan_unit(&mut self, p_unit: u32) -> Option<ScannedItem> {
        // Read UnitAny structure
        let unit: UnitAny = self.ctx.process.read_memory(p_unit as usize).ok()?;

        // Only process items (unit_type == 4)
        if unit.unit_type != unit_type::ITEM {
            return None;
        }

        // Skip if we've already seen this item
        if self.seen_items.contains(&unit.unit_id) {
            return None;
        }

        // Read ItemData
        if unit.p_unit_data == 0 {
            return None;
        }

        let item_data: ItemData = self
            .ctx
            .process
            .read_memory(unit.p_unit_data as usize)
            .ok()?;

        // Create scanned item and try to enrich it using injected game functions.
        let mut scanned = ScannedItem::from_unit(&unit, &item_data, p_unit);

        // Try to resolve item name via injected GetItemName.
        if let Ok(raw_name) = self.injector.get_item_name(&self.ctx.process, p_unit) {
            let cleaned = strip_color_codes(&raw_name);

            // Use the last non-empty line as the display name (matches D2Stats behavior).
            if let Some(last_line) = cleaned.lines().rev().find(|line| !line.trim().is_empty()) {
                scanned.name = Some(last_line.to_string());
            } else if !cleaned.trim().is_empty() {
                scanned.name = Some(cleaned.trim().to_string());
            }
        }

        // Try to resolve item stats text via injected GetItemStats.
        if let Ok(raw_stats) = self.injector.get_item_stats(&self.ctx.process, p_unit) {
            let cleaned = strip_color_codes(&raw_stats);
            if !cleaned.trim().is_empty() {
                scanned.stats = Some(cleaned);
            }
        }

        // Mark as seen
        self.seen_items.insert(unit.unit_id);

        Some(scanned)
    }

    /// Convert a scanned item into an event payload for the frontend.
    fn to_event(&self, scanned: ScannedItem) -> ItemDropEvent {
        let class = scanned.class;
        let quality = scanned.quality_name().to_string();
        let mut name = scanned
            .name
            .unwrap_or_else(|| format!("Item #{}", scanned.class));
        let unique_kind = if scanned.quality == item_quality::UNIQUE {
            self.unique_kind(scanned.file_index, class)
        } else {
            None
        };
        if let Some(kind) = unique_kind {
            name.push(' ');
            name.push_str(kind.label());
        }
        ItemDropEvent {
            unit_id: scanned.unit_id,
            class,
            quality,
            base_name: self.class_base_name(class),
            name,
            stats: scanned.stats.unwrap_or_default(),
            is_ethereal: scanned.is_ethereal,
            is_identified: scanned.is_identified,
            p_unit_data: scanned.p_unit_data,
            tier: self.class_tier(class),
            unique_kind,
            filter: None,
        }
    }

    fn unique_kind(&self, file_index: u32, class: u32) -> Option<UniqueKind> {
        let from_wlvl = self
            .unique_cache
            .as_ref()
            .and_then(|cache| cache.get(file_index as usize))
            .and_then(|info| info.kind);
        classify_unique_kind(from_wlvl, self.class_tier(class))
    }

    fn class_tier(&self, class: u32) -> Option<ItemTier> {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .map(|info| info.tier)
    }

    fn class_base_name(&self, class: u32) -> String {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .map(|info| info.base_name.clone())
            .unwrap_or_default()
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
    fn build_class_cache(&self) -> Result<Vec<ClassInfo>, String> {
        let count_addr = self.ctx.d2_common + d2common::ITEMS_TXT_COUNT;
        let ptr_addr = self.ctx.d2_common + d2common::ITEMS_TXT;

        let count = self.ctx.process.read_memory::<u32>(count_addr)? as usize;
        let base_ptr = self.ctx.process.read_memory::<u32>(ptr_addr)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "items.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let re = regex::Regex::new(r"(?i)\(Sacred\)|\(Angelic\)|\(Mastercrafted\)|[1-4]")
            .map_err(|e| format!("tier regex compile failed: {}", e))?;

        let mut cache = Vec::with_capacity(count);

        for class in 0..count {
            let record = base_ptr + class * items_txt::RECORD_SIZE;

            // MISC != 0 → weapon or armor (tier-eligible).
            let misc = self
                .ctx
                .process
                .read_memory::<u32>(record + items_txt::MISC)
                .unwrap_or(0);

            let name_id = self
                .ctx
                .process
                .read_memory::<u16>(record + items_txt::NAME_ID)
                .unwrap_or(0);

            let raw_name = match self.injector.get_string(&self.ctx.process, name_id, 100) {
                Ok(s) => strip_color_codes(&s),
                Err(_) => {
                    cache.push(ClassInfo {
                        base_name: String::new(),
                        tier: ItemTier::Tier0,
                    });
                    continue;
                }
            };

            let base_name = raw_name
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

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

            cache.push(ClassInfo { base_name, tier });
        }

        Ok(cache)
    }

    fn build_unique_items_cache(&self) -> Result<Vec<UniqueInfo>, String> {
        let sgpt = self
            .ctx
            .process
            .read_memory::<u32>(self.ctx.d2_common + d2common::SGPT_DATA_TABLES)?
            as usize;
        if sgpt == 0 {
            return Err("sgptDataTables is NULL".into());
        }

        let count = self
            .ctx
            .process
            .read_memory::<u32>(sgpt + data_tables::UNIQUE_ITEMS_TXT_COUNT)?
            as usize;
        let base_ptr =
            self.ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::UNIQUE_ITEMS_TXT_PTR)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "UniqueItems.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let mut cache = Vec::with_capacity(count);

        // Push exactly one UniqueInfo per UniqueItems.txt record so that
        // runtime lookup by `ItemData.file_index` stays O(1).
        for i in 0..count {
            let record = base_ptr + i * unique_items_txt::RECORD_SIZE;

            let name_id = self
                .ctx
                .process
                .read_memory::<u16>(record + unique_items_txt::NAME_ID)
                .unwrap_or(0);
            let wlvl = self
                .ctx
                .process
                .read_memory::<u16>(record + unique_items_txt::LEVEL)
                .unwrap_or(0);

            let display_name = self
                .injector
                .get_string(&self.ctx.process, name_id, 200)
                .map(|s| strip_color_codes(&s).trim().to_string())
                .unwrap_or_default();

            cache.push(UniqueInfo {
                display_name,
                kind: UniqueKind::from_wlvl(wlvl),
            });
        }

        Ok(cache)
    }

    fn build_set_items_cache(&self) -> Result<Vec<String>, String> {
        let sgpt = self
            .ctx
            .process
            .read_memory::<u32>(self.ctx.d2_common + d2common::SGPT_DATA_TABLES)?
            as usize;
        if sgpt == 0 {
            return Err("sgptDataTables is NULL".into());
        }

        let count =
            self.ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::SET_ITEMS_TXT_COUNT)? as usize;
        let base_ptr =
            self.ctx
                .process
                .read_memory::<u32>(sgpt + data_tables::SET_ITEMS_TXT_PTR)? as usize;

        if count == 0 || base_ptr == 0 {
            return Err(format!(
                "SetItems.txt not available (count={}, ptr=0x{:X})",
                count, base_ptr
            ));
        }

        let mut cache = Vec::with_capacity(count);
        for i in 0..count {
            let record = base_ptr + i * set_items_txt::RECORD_SIZE;
            let name_id = match self
                .ctx
                .process
                .read_memory::<u16>(record + set_items_txt::NAME_ID)
            {
                Ok(v) => v,
                Err(_) => continue,
            };
            let name = match self.injector.get_string(&self.ctx.process, name_id, 200) {
                Ok(s) => strip_color_codes(&s).trim().to_string(),
                Err(_) => continue,
            };
            if !name.is_empty() {
                cache.push(name);
            }
        }

        Ok(cache)
    }
}

/// Strip D2 color codes from string (ÿc followed by color char)
fn strip_color_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == 'ÿ' {
            // Skip 'c' and the color character
            if chars.peek() == Some(&'c') {
                chars.next(); // skip 'c'
                chars.next(); // skip color char
                continue;
            }
        }
        result.push(c);
    }

    result
}

#[cfg(target_os = "windows")]
impl Drop for DropScanner {
    fn drop(&mut self) {
        // Eject the loot filter hook when scanner is destroyed
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.eject(&self.ctx) {
                log_error(&format!("Failed to eject loot filter hook: {}", e));
            }
        }
    }
}

// --- Stub for Non-Windows ---

#[cfg(not(target_os = "windows"))]
use crate::rules::FilterConfig;

#[cfg(not(target_os = "windows"))]
pub struct DropScanner;

#[cfg(not(target_os = "windows"))]
impl DropScanner {
    pub fn new() -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn set_filter_config(&mut self, _config: Arc<RwLock<FilterConfig>>) {}

    pub fn on_filter_config_changed(&mut self) {}

    pub fn set_filter_enabled(&mut self, _enabled: bool) {}

    pub fn is_filter_enabled(&self) -> bool {
        false
    }

    pub fn is_ingame(&self) -> bool {
        false
    }

    pub fn clear_cache(&mut self) {}

    pub fn context(&self) -> ! {
        panic!("Not supported on this OS")
    }

    pub fn tick(&mut self) -> Vec<ItemDropEvent> {
        Vec::new()
    }
}
