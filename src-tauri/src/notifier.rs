//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use std::collections::{HashMap, HashSet};
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
use crate::offsets::{d2client, item_data, paths, unit_type};
#[cfg(target_os = "windows")]
use crate::process::D2Context;
#[cfg(target_os = "windows")]
use crate::rules::{FilterConfig, MatchContext};

/// Payload sent to frontend when an item is found
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ItemDropEvent {
    pub unit_id: u32,
    pub class: u32,
    pub quality: String,
    pub name: String,
    pub stats: String,
    pub is_ethereal: bool,
    pub is_identified: bool,
    /// Pointer to ItemData structure (for set_item_visibility)
    pub p_unit_data: u32,
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
    /// Resolved items by unit_id; reused on config change to avoid re-injection.
    item_cache: HashMap<u32, ItemDropEvent>,
}

#[cfg(target_os = "windows")]
impl DropScanner {
    /// Create a new scanner attached to the D2 process
    pub fn new() -> Result<Self, String> {
        let ctx = D2Context::new()?;
        let injector = D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common)?;

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
            item_cache: HashMap::new(),
        })
    }

    pub fn set_filter_config(&mut self, config: Arc<RwLock<FilterConfig>>) {
        self.filter_config = Some(config);
    }

    pub fn on_filter_config_changed(&mut self) {
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.clear_hidden_items(&self.ctx) {
                log_error(&format!(
                    "Failed to clear hide mask on config change: {}",
                    e
                ));
            }
            if let Err(e) = self.loot_hook.clear_shown_items(&self.ctx) {
                log_error(&format!(
                    "Failed to clear show mask on config change: {}",
                    e
                ));
            }
        }
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

    /// Clear the seen items cache (call when entering a new game)
    pub fn clear_cache(&mut self) {
        self.seen_items.clear();
        self.item_cache.clear();
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.clear_hidden_items(&self.ctx) {
                log_error(&format!("Failed to clear hide mask: {}", e));
            }
            if let Err(e) = self.loot_hook.clear_shown_items(&self.ctx) {
                log_error(&format!("Failed to clear show mask: {}", e));
            }
        }
    }

    /// Get a reference to the D2Context
    pub fn context(&self) -> &D2Context {
        &self.ctx
    }

    /// Set item visibility by writing to iEarLevel field in ItemData
    /// value: 0 = not processed (default), 1 = show, 2 = hide
    pub fn set_item_visibility(&self, p_unit_data: u32, visible: bool) -> Result<(), String> {
        if p_unit_data == 0 {
            return Err("p_unit_data is null".to_string());
        }

        let value: u8 = if visible { 1 } else { 2 };
        let addr = p_unit_data as usize + item_data::EAR_LEVEL;

        // Write the value
        self.ctx.process.write_buffer(addr, &[value])?;

        // Verify the write
        let mut verify = [0u8; 1];
        if let Ok(()) = self.ctx.process.read_buffer_into(addr, &mut verify) {
            if verify[0] != value {
                log_error(&format!(
                    "set_item_visibility: verify mismatch at 0x{:08X}: wrote {} but read back {}",
                    addr, value, verify[0]
                ));
            }
        }

        Ok(())
    }

    /// Scan for ground items and return new items found
    pub fn tick(&mut self) -> Vec<ItemDropEvent> {
        let mut events = Vec::new();

        if !self.is_ingame() {
            return events;
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
                    let event = Self::to_event(scanned);

                    // Cache the resolved event so later sweeps (e.g. after a
                    // config change) can match name/stat rules against real
                    // text instead of synthetic "Item#<class>" fallbacks.
                    self.item_cache.insert(event.unit_id, event.clone());

                    // Apply filter if enabled
                    if self.filter_enabled {
                        if let Some(ref filter_arc) = self.filter_config {
                            if let Ok(filter) = filter_arc.read() {
                                let ctx = MatchContext::new(&event);

                                let matched: Vec<String> = filter
                                    .rules
                                    .iter()
                                    .filter(|r| r.active && ctx.matches(r))
                                    .map(|r| match &r.name_pattern {
                                        Some(p) => format!("\"{}\"", p),
                                        None => "<any>".to_string(),
                                    })
                                    .collect();

                                let action = filter.get_action(&ctx);
                                let decision = if action.color.as_deref() == Some("show") {
                                    "FORCE-SHOW"
                                } else if !action.show_item {
                                    "HIDE"
                                } else {
                                    "SHOW"
                                };

                                let reason = if matched.is_empty() {
                                    format!(
                                        "no rules matched, default={}",
                                        if filter.default_show_items {
                                            "Show All"
                                        } else {
                                            "Hide All"
                                        }
                                    )
                                } else {
                                    format!(
                                        "matched {}/{}: {}",
                                        matched.len(),
                                        filter.rules.len(),
                                        matched.join(", ")
                                    )
                                };

                                log_info(&format!(
                                    "[Filter] \"{}\" ({}, class={}) -> {} | {}",
                                    event.name, event.quality, event.class, decision, reason
                                ));

                                if self.loot_hook.is_injected() {
                                    if action.color.as_deref() == Some("show") {
                                        if let Err(e) = self
                                            .loot_hook
                                            .add_shown_unit_id(&self.ctx, event.unit_id)
                                        {
                                            log_error(&format!(
                                                "Failed to force-show item {}: {}",
                                                event.unit_id, e
                                            ));
                                        }
                                    } else if !action.show_item {
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
                                }
                            }
                        }
                    }

                    events.push(event);
                }

                // Move to next unit (use struct layout for safety instead of hardcoded offset)
                let unit: UnitAny = match self.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };
                p_unit = unit.p_next_unit;
            }
        }

        // Apply filter to ALL ground items (including already seen ones).
        // This ensures newly spawned items are added to the hide mask and,
        // after a config change, already-cached items are re-evaluated.
        // We clone the FilterConfig out of the lock so the sweep can take
        // `&mut self` (needed to write into item_cache / log_next_sweep).
        let filter_snapshot = if self.filter_enabled {
            self.filter_config
                .as_ref()
                .and_then(|arc| arc.read().ok().map(|g| g.clone()))
        } else {
            None
        };
        if let Some(filter) = filter_snapshot {
            self.apply_filter_to_all_items(&filter, p_paths, i_paths);
        }

        events
    }

    /// Apply filter rules to all ground items (not just new ones).
    /// This ensures items that should be hidden end up in the bitmask,
    /// including items already on the ground at config-change time.
    fn apply_filter_to_all_items(
        &mut self,
        filter: &FilterConfig,
        p_paths: usize,
        i_paths: usize,
    ) {
        if !self.loot_hook.is_injected() {
            return;
        }

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

            while p_unit != 0 {
                // Read unit
                let unit: UnitAny = match self.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };

                // Only process items
                if unit.unit_type == unit_type::ITEM && unit.p_unit_data != 0 {
                    // Prefer the cached event (has real name/stats from injection).
                    // Fall back to a synthetic event for items we haven't scanned yet.
                    let event = if let Some(cached) = self.item_cache.get(&unit.unit_id) {
                        cached.clone()
                    } else if let Ok(idata) = self
                        .ctx
                        .process
                        .read_memory::<ItemData>(unit.p_unit_data as usize)
                    {
                        let scanned = ScannedItem::from_unit(&unit, &idata, p_unit);
                        ItemDropEvent {
                            unit_id: unit.unit_id,
                            class: unit.class,
                            quality: scanned.quality_name().to_string(),
                            name: scanned
                                .name
                                .clone()
                                .unwrap_or_else(|| format!("Item#{}", unit.class)),
                            stats: scanned.stats.clone().unwrap_or_default(),
                            is_ethereal: scanned.is_ethereal,
                            is_identified: scanned.is_identified,
                            p_unit_data: unit.p_unit_data,
                        }
                    } else {
                        p_unit = unit.p_next_unit;
                        continue;
                    };

                    let ctx = MatchContext::new(&event);
                    let action = filter.get_action(&ctx);

                    if action.color.as_deref() == Some("show") {
                        let _ = self.loot_hook.add_shown_unit_id(&self.ctx, unit.unit_id);
                    } else if !action.show_item {
                        let _ = self.loot_hook.add_hidden_unit_id(&self.ctx, unit.unit_id);
                    }
                }

                p_unit = unit.p_next_unit;
            }
        }
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
    fn to_event(scanned: ScannedItem) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: scanned.unit_id,
            class: scanned.class,
            quality: scanned.quality_name().to_string(),
            // Fallback to a generic name if injection failed or returned empty text.
            name: scanned
                .name
                .unwrap_or_else(|| format!("Item #{}", scanned.class)),
            stats: scanned.stats.unwrap_or_default(),
            is_ethereal: scanned.is_ethereal,
            is_identified: scanned.is_identified,
            p_unit_data: scanned.p_unit_data,
        }
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

    pub fn set_item_visibility(&self, _p_unit_data: u32, _visible: bool) -> Result<(), String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn tick(&mut self) -> Vec<ItemDropEvent> {
        Vec::new()
    }
}
