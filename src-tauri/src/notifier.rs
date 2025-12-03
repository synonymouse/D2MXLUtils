//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use std::collections::HashSet;

#[cfg(target_os = "windows")]
use crate::d2types::{ItemData, ScannedItem, UnitAny};
#[cfg(target_os = "windows")]
use crate::injection::D2Injector;
#[cfg(target_os = "windows")]
use crate::logger::{error as log_error, info as log_info};
#[cfg(target_os = "windows")]
use crate::offsets::{d2client, paths, unit_type};
#[cfg(target_os = "windows")]
use crate::process::D2Context;

/// Payload sent to frontend when an item is found
#[derive(Debug, Clone, serde::Serialize)]
pub struct ItemDropEvent {
    pub unit_id: u32,
    pub class: u32,
    pub quality: String,
    pub name: String,
    pub stats: String,
    pub is_ethereal: bool,
    pub is_identified: bool,
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
    /// One-shot debug flag to log pointer chain / counts once per game session
    debug_logged_paths: bool,
}

#[cfg(target_os = "windows")]
impl DropScanner {
    /// Create a new scanner attached to the D2 process
    pub fn new() -> Result<Self, String> {
        let ctx = D2Context::new()?;
        let injector = D2Injector::new(&ctx.process, ctx.d2_client, ctx.d2_common)?;
        
        log_info(&format!(
            "DropScanner initialized: D2Client=0x{:08X}, D2Common=0x{:08X}, StringBuffer=0x{:08X}",
            ctx.d2_client, ctx.d2_common, injector.string_buffer.address
        ));
        
        Ok(Self {
            ctx,
            injector,
            seen_items: HashSet::new(),
            debug_logged_paths: false,
        })
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
        
        let ptr2 = match self.ctx.process.read_memory::<u32>(ptr1 + paths::TO_PATHS_PTR[1]) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };
        
        let ptr3 = match self.ctx.process.read_memory::<u32>(ptr2 + paths::TO_PATHS_PTR[2]) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };
        
        let p_paths = match self.ctx.process.read_memory::<u32>(ptr3 + paths::TO_PATHS_PTR[3]) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };
        
        let i_paths = match self.ctx.process.read_memory::<u32>(ptr3 + paths::TO_PATHS_COUNT[3]) {
            Ok(p) => p as usize,
            _ => return events,
        };

        // Log pointer chain once to help debug cases when no items are detected.
        if !self.debug_logged_paths {
            let msg = format!(
                "Notifier debug: ptr1=0x{:08X}, ptr2=0x{:08X}, ptr3=0x{:08X}, p_paths=0x{:08X}, i_paths={}",
                ptr1, ptr2, ptr3, p_paths, i_paths
            );
            log_info(&msg);
            self.debug_logged_paths = true;
        }
        
        // Iterate through each path/room
        for i in 0..i_paths {
            let p_path = match self.ctx.process.read_memory::<u32>(p_paths + 4 * i) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };
            
            let mut p_unit = match self.ctx.process.read_memory::<u32>(p_path + paths::PATH_TO_UNIT) {
                Ok(p) if p != 0 => p,
                _ => continue,
            };
            
            // Iterate through units in this room
            while p_unit != 0 {
                if let Some(scanned) = self.scan_unit(p_unit) {
                    events.push(Self::to_event(scanned));
                }

                // Move to next unit (use struct layout for safety instead of hardcoded offset)
                let unit: UnitAny = match self.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(e) => {
                        let msg = format!(
                            "Notifier debug: failed to read UnitAny at 0x{:08X}: {}",
                            p_unit, e
                        );
                        log_error(&msg);
                        break;
                    }
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
        
        let item_data: ItemData = self.ctx.process.read_memory(unit.p_unit_data as usize).ok()?;
        
        // Create scanned item and try to enrich it using injected game functions.
        let mut scanned = ScannedItem::from_unit(&unit, &item_data, p_unit);

        // Try to resolve item name via injected GetItemName.
        match self
            .injector
            .get_item_name(&self.ctx.process, p_unit)
        {
            Ok(raw_name) => {
                let cleaned = strip_color_codes(&raw_name);

                // Use the last non-empty line as the display name (matches D2Stats behavior).
                if let Some(last_line) = cleaned
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                {
                    scanned.name = Some(last_line.to_string());
                } else if !cleaned.trim().is_empty() {
                    scanned.name = Some(cleaned.trim().to_string());
                } else {
                    let msg = format!(
                        "Notifier debug: empty item name after injection for unit {} (class {}), raw='{}'",
                        unit.unit_id, unit.class, raw_name
                    );
                    log_info(&msg);
                }
            }
            Err(e) => {
                let msg = format!(
                    "Notifier debug: get_item_name failed for unit {} (class {}): {}",
                    unit.unit_id, unit.class, e
                );
                log_error(&msg);
            }
        }

        // Try to resolve item stats text via injected GetItemStats.
        match self
            .injector
            .get_item_stats(&self.ctx.process, p_unit)
        {
            Ok(raw_stats) => {
                let cleaned = strip_color_codes(&raw_stats);
                if !cleaned.trim().is_empty() {
                    scanned.stats = Some(cleaned);
                }
            }
            Err(e) => {
                let msg = format!(
                    "Notifier debug: get_item_stats failed for unit {} (class {}): {}",
                    unit.unit_id, unit.class, e
                );
                log_error(&msg);
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

// --- Stub for Non-Windows ---

#[cfg(not(target_os = "windows"))]
pub struct DropScanner;

#[cfg(not(target_os = "windows"))]
impl DropScanner {
    pub fn new() -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }
    
    pub fn is_ingame(&self) -> bool {
        false
    }
    
    pub fn clear_cache(&mut self) {}
    
    pub fn tick(&mut self) -> Vec<ItemDropEvent> {
        Vec::new()
    }
}

