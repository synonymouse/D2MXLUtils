//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;

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
    d2client, d2common, d2sigma, data_tables, inventory, item_data, item_quality, items_txt, paths,
    set_items_txt, unique_items_txt, unit, unit_type,
};
#[cfg(target_os = "windows")]
use crate::process::D2Context;
#[cfg(target_os = "windows")]
use crate::rules::{FilterConfig, MatchContext, Visibility};
use crate::rules::{ItemTier, Notification};
#[cfg(target_os = "windows")]
use crate::scanner_state::SharedScannerState;

/// MonStats.txt class IDs that count as "goblins" for the alert sound.
/// Ported verbatim from `D2Stats.au3:$g_goblinIds`.
#[cfg(target_os = "windows")]
const GOBLIN_CLASS_IDS: &[u32] = &[
    2774, 2775, 2776, 2779, 2780, 2781, 2784, 2785, 2786, 2787, 2788, 2789, 2790, 2791, 2792, 2793,
    2794, 2795, 2799, 2802, 2803, 2805,
];

#[derive(Debug, Clone, serde::Serialize)]
pub struct GoblinDetectedEvent {
    pub unit_id: u32,
    pub class: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ItemDropEvent {
    pub unit_id: u32,
    pub class: u32,
    pub quality: String,
    pub name: String,
    #[serde(default)]
    pub base_name: String,
    /// Prefix lines from items.txt's multi-line name (e.g. `"Great Rune"`
    /// for Rhal Rune). Matched alongside `name`/`base_name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub stats: String,
    pub is_ethereal: bool,
    pub is_identified: bool,
    pub p_unit_data: u32,
    /// `dwSeed` — random seed identifying this physical item. Stable
    /// across area unload/reload, so used by loot-history to dedupe
    /// the same item after a teleport-away/return cycle (the engine
    /// assigns a fresh `unit_id` but the seed survives).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub seed: u32,
    /// True iff this scan inserted a *new* row in `LootHistory`
    /// (vs. merged into an existing entry by `seed`). Drives whether
    /// the main loop fires `loot-history-entry` to the frontend —
    /// dedup-merges shouldn't render twice. Skipped from serialization
    /// (internal flag).
    #[serde(default, skip)]
    pub history_pushed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ItemTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_kind: Option<UniqueKind>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub sockets: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Notification>,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// Drop scanner that iterates through ground items
#[cfg(target_os = "windows")]
pub struct DropScanner {
    /// Shared state bundle (ctx, injector, filter_config, filter_enabled,
    /// recent_events). Owned by this thread; Arc cloned to marker thread in
    /// Task 5.
    state: Arc<SharedScannerState>,
    /// Cache of already-seen item IDs (to avoid duplicate notifications)
    seen_items: HashSet<u32>,
    /// When true, log per-item filter decisions (opt-in; noisy).
    verbose_filter_logging: bool,
    /// Loot filter hook for D2Sigma.dll
    loot_hook: LootFilterHook,
    /// Indexed by `UnitAny.class`. Built lazily on first in-game tick.
    class_cache: Option<Vec<ClassInfo>>,
    unique_cache: Option<Vec<UniqueInfo>>,
    set_cache: Option<Vec<String>>,
    /// Session loot history. Shared with main thread so Tauri commands can
    /// snapshot it. Updated each tick.
    loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
    /// Pickup-state transitions produced by the latest `tick_items` call.
    /// Drained by main loop into `loot-history-update` events. Each tuple
    /// is `(unit_id, seed, new_state)`; `seed` is the stable key the
    /// frontend uses to find the row.
    last_pickup_updates: Vec<(u32, u32, crate::loot_history::PickupState)>,
    /// Consecutive ticks each tracked unit_id was missing from the scan.
    /// Hook-mask bits cleared once the count hits
    /// `MISSED_TICKS_BEFORE_BIT_CLEAR`. The grace period absorbs transient
    /// `read_memory` failures so we don't re-trigger the `bff0c0d` flicker.
    missed_ticks: HashMap<u32, u8>,
    /// Monster `unit_id`s already announced via `goblin-detected`. Not
    /// pruned by current-scan presence — same `unit_id` only fires once
    /// per scanner lifetime. Cleared by `clear_cache()` (filter swap /
    /// game-entry transitions).
    seen_goblins: HashSet<u32>,
    /// Goblins detected in the latest `tick_items` pass; drained by main
    /// loop into `goblin-detected` events. Same pattern as `last_pickup_updates`.
    last_goblin_events: Vec<GoblinDetectedEvent>,
}

#[cfg(target_os = "windows")]
const MISSED_TICKS_BEFORE_BIT_CLEAR: u8 = 2;

#[derive(Debug, Clone)]
struct ClassInfo {
    base_name: String,
    category: Option<String>,
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
    /// Create a new scanner using the provided shared state.
    /// `ctx` and `injector` are constructed by the caller (main.rs) and
    /// passed in via `Arc<SharedScannerState>`.
    pub fn new(
        state: Arc<SharedScannerState>,
        loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
    ) -> Result<Self, String> {
        // Initialize and inject the loot filter hook (uses ctx from shared state).
        let mut loot_hook = LootFilterHook::new();
        if state.ctx.d2_sigma != 0 {
            if let Err(e) = loot_hook.inject(&state.ctx) {
                log_error(&format!("Failed to inject LootFilterHook: {}", e));
            }
        }

        Ok(Self {
            state,
            seen_items: HashSet::new(),
            verbose_filter_logging: false,
            loot_hook,
            class_cache: None,
            unique_cache: None,
            set_cache: None,
            loot_history,
            last_pickup_updates: Vec::new(),
            missed_ticks: HashMap::new(),
            seen_goblins: HashSet::new(),
            last_goblin_events: Vec::new(),
        })
    }

    pub fn set_filter_config(&mut self, config: Arc<RwLock<FilterConfig>>) {
        *self.state.filter_config.write().unwrap() = Some(config);
    }

    pub fn on_filter_config_changed(&mut self) {
        self.clear_cache();
    }

    /// Enable or disable automatic filtering
    pub fn set_filter_enabled(&mut self, enabled: bool) {
        if self.state.filter_enabled.load(Ordering::Relaxed) == enabled {
            return; // No change
        }

        self.state.filter_enabled.store(enabled, Ordering::Relaxed);

        // Sync with the loot filter hook
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.set_filter_enabled(&self.state.ctx, enabled) {
                log_error(&format!("Failed to set hook filter_enabled: {}", e));
            }
        }
    }

    /// Check if filtering is enabled
    pub fn is_filter_enabled(&self) -> bool {
        self.state.filter_enabled.load(Ordering::Relaxed)
            && self.state.filter_config.read().unwrap().is_some()
    }

    pub fn set_verbose_filter_logging(&mut self, enabled: bool) {
        self.verbose_filter_logging = enabled;
    }

    pub fn set_force_show_all(&self, value: bool) -> Result<(), String> {
        if !self.loot_hook.is_injected() {
            return Ok(());
        }
        self.loot_hook.set_force_show_all(&self.state.ctx, value)
    }

    /// Check if filter config is set
    pub fn has_filter_config(&self) -> bool {
        self.state.filter_config.read().unwrap().is_some()
    }

    /// Check if player is in game
    pub fn is_ingame(&self) -> bool {
        let player_unit_ptr = self.state.ctx.d2_client + d2client::PLAYER_UNIT;
        match self.state.ctx.process.read_memory::<u32>(player_unit_ptr) {
            Ok(ptr) => ptr != 0,
            Err(_) => false,
        }
    }

    fn always_show_items_addr(&self) -> Result<Option<usize>, String> {
        if self.state.ctx.d2_sigma == 0 {
            return Ok(None);
        }
        let Some(rva) = self.state.ctx.always_show_items_ptr_rva else {
            return Ok(None);
        };
        let base = self.state.ctx.d2_sigma + rva;
        let struct_ptr = self.state.ctx.process.read_memory::<u32>(base)?;
        if struct_ptr == 0 {
            return Ok(None);
        }
        Ok(Some(struct_ptr as usize + d2sigma::ALWAYS_SHOW_ITEMS_FLAG))
    }

    /// Ok(false) = base ptr NULL (caller should retry next tick).
    pub fn set_always_show_items(&self, on: bool) -> Result<bool, String> {
        let Some(addr) = self.always_show_items_addr()? else {
            return Ok(false);
        };
        let value: u32 = if on { 1 } else { 0 };
        self.state
            .ctx
            .process
            .write_buffer(addr, &value.to_le_bytes())?;
        Ok(true)
    }

    /// Ok(None) = struct not allocated yet.
    pub fn read_always_show_items(&self) -> Result<Option<bool>, String> {
        let Some(addr) = self.always_show_items_addr()? else {
            return Ok(None);
        };
        let value = self.state.ctx.process.read_memory::<u32>(addr)?;
        Ok(Some(value != 0))
    }

    pub fn clear_cache(&mut self) {
        self.seen_items.clear();
        self.missed_ticks.clear();
        self.seen_goblins.clear();
        self.state.recent_events.write().unwrap().clear();
        if self.loot_hook.is_injected() {
            if let Err(e) = self.loot_hook.clear_hidden_items(&self.state.ctx) {
                log_error(&format!("Failed to clear hide mask: {}", e));
            }
            if let Err(e) = self.loot_hook.clear_shown_items(&self.state.ctx) {
                log_error(&format!("Failed to clear show mask: {}", e));
            }
            if let Err(e) = self.loot_hook.clear_inspected_mask(&self.state.ctx) {
                log_error(&format!("Failed to clear inspected mask: {}", e));
            }
        }
    }

    /// Get a reference to the D2Context
    pub fn context(&self) -> &D2Context {
        &self.state.ctx
    }

    /// Scan ground items (pPaths pass) and return fresh notification events.
    ///
    /// Intentionally excludes the map-marker BFS pass so callers can emit
    /// `item-drop` events before the (potentially expensive) marker
    /// reconciliation runs. The marker pass is handled by `MarkerScanner::tick`.
    pub fn tick_items(&mut self) -> Vec<ItemDropEvent> {
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
        let base_ptr = self.state.ctx.d2_client + d2client::PLAYER_UNIT;

        // Follow pointer chain: [base] -> [+0x2C] -> [+0x1C] -> pPaths (at +0x0) and iPaths (at +0x24)
        let ptr1 = match self.state.ctx.process.read_memory::<u32>(base_ptr) {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let ptr2 = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr1 + paths::TO_PATHS_PTR[1])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let ptr3 = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr2 + paths::TO_PATHS_PTR[2])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let p_paths = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_PTR[3])
        {
            Ok(p) if p != 0 => p as usize,
            _ => return events,
        };

        let i_paths = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr3 + paths::TO_PATHS_COUNT[3])
        {
            Ok(p) => p as usize,
            _ => return events,
        };

        let mut current_item_ids: HashSet<u32> = HashSet::new();

        // Iterate through each path/room
        for i in 0..i_paths {
            let p_path = match self.state.ctx.process.read_memory::<u32>(p_paths + 4 * i) {
                Ok(p) if p != 0 => p as usize,
                _ => continue,
            };

            let mut p_unit = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_path + paths::PATH_TO_UNIT)
            {
                Ok(p) if p != 0 => p,
                _ => continue,
            };

            // Iterate through units in this room
            while p_unit != 0 {
                let unit: UnitAny = match self.state.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };

                if unit.unit_type == unit_type::ITEM {
                    current_item_ids.insert(unit.unit_id);
                } else if unit.unit_type == unit_type::MONSTER
                    && GOBLIN_CLASS_IDS.contains(&unit.class)
                    && self.seen_goblins.insert(unit.unit_id)
                {
                    self.last_goblin_events.push(GoblinDetectedEvent {
                        unit_id: unit.unit_id,
                        class: unit.class,
                    });
                }

                if let Some(scanned) = self.scan_unit(p_unit, &unit) {
                    let event = self.to_event(scanned);
                    let unit_id = event.unit_id;

                    // Apply filter if enabled
                    let mut event = event;
                    let mut should_emit = true;
                    if self.state.filter_enabled.load(Ordering::Relaxed) {
                        let filter_arc = self.state.filter_config.read().unwrap().clone();
                        if let Some(ref filter_arc) = filter_arc {
                            if let Ok(filter) = filter_arc.read() {
                                let ctx = MatchContext::new(&event);
                                let decision = filter.decide(&ctx);

                                if self.verbose_filter_logging {
                                    let winner = filter.rules.iter().rev().find(|r| ctx.matches(r));
                                    let reason = match winner {
                                        Some(r) => format!(
                                            "winner={}",
                                            r.name_pattern.as_deref().unwrap_or("<any>")
                                        ),
                                        None => format!(
                                            "no rule matched (hide_all={})",
                                            filter.hide_all
                                        ),
                                    };
                                    let vis_label = match decision.visibility {
                                        Visibility::Show => "SHOW",
                                        Visibility::Hide => "HIDE",
                                        Visibility::Default => "DEFAULT",
                                    };
                                    let category_label = event
                                        .category
                                        .as_deref()
                                        .map(|c| format!(" [{}]", c.replace('\n', "|")))
                                        .unwrap_or_default();
                                    log_info(&format!(
                                        "[Filter] \"{} {}\"{} ({}, class={}) -> {} notify={} | {}",
                                        event.name,
                                        event.base_name,
                                        category_label,
                                        event.quality,
                                        event.class,
                                        vis_label,
                                        decision.notification.is_some(),
                                        reason
                                    ));
                                }

                                if self.loot_hook.is_injected() {
                                    match decision.visibility {
                                        Visibility::Show => {
                                            if let Err(e) = self
                                                .loot_hook
                                                .add_shown_unit_id(&self.state.ctx, event.unit_id)
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
                                                .add_hidden_unit_id(&self.state.ctx, event.unit_id)
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

                    // Cache enriched event for the map-marker pass.
                    self.state
                        .recent_events
                        .write()
                        .unwrap()
                        .insert(event.unit_id, event.clone());

                    if should_emit {
                        // Push to session history (only filter-matched items
                        // — same gate as overlay notifications).
                        if event.filter.is_some() {
                            let color = event
                                .filter
                                .as_ref()
                                .and_then(|n| n.color.as_ref())
                                .map(|c| c.lowercase_name().to_string());
                            let entry = crate::loot_history::LootEntry {
                                unit_id: event.unit_id,
                                timestamp_ms: crate::loot_history::now_ms(),
                                name: event.name.clone(),
                                quality: event.quality.clone(),
                                color,
                                pickup: crate::loot_history::PickupState::Pending,
                                seed: event.seed,
                            };
                            // Only fresh inserts emit `loot-history-entry`;
                            // dedup-merges silently update the existing row
                            // (frontend keys by `seed`, so no notification
                            // needed when only `unit_id` changes underneath).
                            let outcome = if let Ok(mut hist) = self.loot_history.write() {
                                hist.push(entry)
                            } else {
                                crate::loot_history::PushOutcome::Duplicate
                            };
                            event.history_pushed =
                                matches!(outcome, crate::loot_history::PushOutcome::Inserted);
                        }
                        events.push(event);
                    }

                    // Must run AFTER show/hide bits: otherwise the game thread
                    // could see inspected=1 with no decision yet and fall through
                    // to MXL's default (= flash the label).
                    if self.loot_hook.is_injected() {
                        if let Err(e) = self
                            .loot_hook
                            .add_inspected_unit_id(&self.state.ctx, unit_id)
                        {
                            log_error(&format!("Failed to mark item {} inspected: {}", unit_id, e));
                        }
                    }
                }

                p_unit = unit.p_next_unit;
            }
        }

        // dwUnitId stays stable when an item moves between ground and
        // inventory, so without pruning a re-dropped item would never notify.
        // Age missing-tick counters before the retain — items absent for
        // `MISSED_TICKS_BEFORE_BIT_CLEAR` consecutive ticks have their
        // hook-mask bits batch-cleared so old decisions don't outlive the
        // physical item and alias new drops via `unit_id & MASK_INDEX_BITS`.
        let mut to_clear: Vec<u32> = Vec::new();
        for &id in self.seen_items.iter() {
            if current_item_ids.contains(&id) {
                self.missed_ticks.remove(&id);
            } else {
                let count = self.missed_ticks.entry(id).or_insert(0);
                *count = count.saturating_add(1);
                if *count >= MISSED_TICKS_BEFORE_BIT_CLEAR {
                    to_clear.push(id);
                }
            }
        }
        for id in &to_clear {
            self.missed_ticks.remove(id);
        }
        if !to_clear.is_empty() && self.loot_hook.is_injected() {
            if let Err(e) = self
                .loot_hook
                .clear_unit_id_bits(&self.state.ctx, &to_clear)
            {
                log_error(&format!(
                    "Failed to clear hook bits for {} departed items: {}",
                    to_clear.len(),
                    e
                ));
            }
        }

        self.seen_items.retain(|id| current_item_ids.contains(id));
        self.state
            .recent_events
            .write()
            .unwrap()
            .retain(|id, _| current_item_ids.contains(id));

        // Pickup resolution: walk the local hero's inventory once and
        // promote any matching Pending entries to PickedUp. Skip when no
        // entry is Pending — saves the inventory walk.
        let has_pending = self
            .loot_history
            .read()
            .map(|h| h.has_pending())
            .unwrap_or(false);
        if has_pending {
            let our_ids = self.read_player_inventory_ids();
            if let Ok(mut hist) = self.loot_history.write() {
                let resolved = hist.resolve_pending(&our_ids);
                self.last_pickup_updates.extend(resolved);
            }
        }

        events
    }

    /// Process a single unit, returning a fully scanned item if it's a new item.
    fn scan_unit(&mut self, p_unit: u32, unit: &UnitAny) -> Option<ScannedItem> {
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
            .state
            .ctx
            .process
            .read_memory(unit.p_unit_data as usize)
            .ok()?;

        // Create scanned item and try to enrich it using injected game functions.
        let mut scanned = ScannedItem::from_unit(unit, &item_data, p_unit);

        {
            let injector = self.state.injector.lock().unwrap();
            if item_data.is_socketed() {
                if let Ok(n) = injector.get_unit_stat(&self.state.ctx.process, p_unit, 0xC2) {
                    scanned.sockets = n.min(6) as u8;
                }
            }

            // Try to resolve item name via injected GetItemName.
            if let Ok(raw_name) = injector.get_item_name(&self.state.ctx.process, p_unit) {
                let cleaned = strip_color_codes(&raw_name);

                // Use the last non-empty line as the display name (matches D2Stats behavior).
                if let Some(last_line) = cleaned.lines().rev().find(|line| !line.trim().is_empty())
                {
                    scanned.name = Some(last_line.to_string());
                } else if !cleaned.trim().is_empty() {
                    scanned.name = Some(cleaned.trim().to_string());
                }
            }

            // Try to resolve item stats text via injected GetItemStats.
            if let Ok(raw_stats) = injector.get_item_stats(&self.state.ctx.process, p_unit) {
                let cleaned = strip_color_codes(&raw_stats);
                if !cleaned.trim().is_empty() {
                    let reversed: Vec<&str> = cleaned.lines().rev().collect();
                    scanned.stats = Some(reversed.join("\n"));
                }
            }

            // Fallback: for items whose stats come from data tables rather
            // than the unit's stat list (e.g. Cycles), read the bonus
            // description from the items.txt string-table ID at +0xB6.
            if scanned.stats.is_none() {
                if let Some(text) = self.read_item_desc_from_txt(
                    &injector,
                    scanned.class,
                ) {
                    scanned.stats = Some(text);
                }
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
        let raw_stats = scanned.stats.unwrap_or_default();
        let stats = if scanned.sockets > 0 {
            if raw_stats.is_empty() {
                format!("Socketed ({})", scanned.sockets)
            } else {
                format!("Socketed ({})\n{}", scanned.sockets, raw_stats)
            }
        } else {
            raw_stats
        };
        // Read dwSeed at item_data + 0x14 — stable per-item across area
        // unload/reload, used by loot-history dedup.
        let seed = if scanned.p_unit_data != 0 {
            self.state
                .ctx
                .process
                .read_memory::<u32>(scanned.p_unit_data as usize + item_data::SEED)
                .unwrap_or(0)
        } else {
            0
        };
        ItemDropEvent {
            unit_id: scanned.unit_id,
            class,
            quality,
            base_name: self.class_base_name(class),
            category: self.class_category(class),
            name,
            stats,
            is_ethereal: scanned.is_ethereal,
            is_identified: scanned.is_identified,
            p_unit_data: scanned.p_unit_data,
            seed,
            history_pushed: false,
            tier: self.class_tier(class),
            unique_kind,
            sockets: scanned.sockets,
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

    fn class_category(&self, class: u32) -> Option<String> {
        self.class_cache
            .as_ref()
            .and_then(|cache| cache.get(class as usize))
            .and_then(|info| info.category.clone())
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

    fn build_unique_items_cache(&self) -> Result<Vec<UniqueInfo>, String> {
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

    fn build_set_items_cache(&self) -> Result<Vec<String>, String> {
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
            let name_id = match self
                .state
                .ctx
                .process
                .read_memory::<u16>(record + set_items_txt::NAME_ID)
            {
                Ok(v) => v,
                Err(_) => continue,
            };
            let name = match injector.get_string(&self.state.ctx.process, name_id, 200) {
                Ok(s) => strip_color_codes(&s).trim().to_string(),
                Err(_) => continue,
            };
            if !name.is_empty() {
                cache.push(name);
            }
        }

        Ok(cache)
    }

    /// Walk the local player's inventory and return every item `unit_id`
    /// linked off `pFirstItem`. Robust against stale `p_unit_data` caches:
    /// the walk uses live pointers from the player struct outward.
    ///
    /// Chain: `PLAYER_UNIT` → `UnitAny + 0x60 (Inventory*)` →
    ///        `Inventory + 0x0C (pFirstItem)` → walk via item
    ///        `pUnitData + 0x64 (NEXT_ITEM)`.
    ///
    /// Capped at 256 iterations to defend against pointer cycles.
    fn read_player_inventory_ids(&self) -> HashSet<u32> {
        let mut ids = HashSet::new();

        let player_unit_ptr_addr = self.state.ctx.d2_client + d2client::PLAYER_UNIT;
        let player_ptr = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(player_unit_ptr_addr)
        {
            Ok(p) if p != 0 => p as usize,
            _ => return ids,
        };

        let inv_ptr = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(player_ptr + unit::INVENTORY)
        {
            Ok(p) if p != 0 => p as usize,
            _ => return ids,
        };

        let mut p_item = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(inv_ptr + inventory::FIRST_ITEM)
        {
            Ok(p) => p,
            Err(_) => return ids,
        };

        for _ in 0..256 {
            if p_item == 0 {
                break;
            }
            // UnitAny.unit_id at +0x0C
            if let Ok(uid) = self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_item as usize + unit::UNIT_ID)
            {
                ids.insert(uid);
            }
            // UnitAny.pUnitData at +0x14 → ItemData; ItemData + 0x64 = next.
            let p_unit_data = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_item as usize + unit::UNIT_DATA)
            {
                Ok(p) if p != 0 => p as usize,
                _ => break,
            };
            p_item = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_unit_data + item_data::NEXT_ITEM)
            {
                Ok(p) => p,
                Err(_) => break,
            };
        }

        ids
    }

    /// Read item bonus description from the items.txt string table.
    ///
    /// Items like Median XL Cycles store their property description as a
    /// string-table ID in items.txt at record offset +0xB6 (u16).  The
    /// string contains the full tooltip in bottom-to-top line order.
    fn read_item_desc_from_txt(
        &self,
        injector: &crate::injection::D2Injector,
        class: u32,
    ) -> Option<String> {
        let count: u32 = self
            .state
            .ctx
            .process
            .read_memory(self.state.ctx.d2_common + d2common::ITEMS_TXT_COUNT)
            .ok()?;
        let base_ptr: u32 = self
            .state
            .ctx
            .process
            .read_memory(self.state.ctx.d2_common + d2common::ITEMS_TXT)
            .ok()?;
        if class >= count || base_ptr == 0 {
            return None;
        }
        let record = base_ptr as usize + class as usize * items_txt::RECORD_SIZE;
        let sid: u16 = self
            .state
            .ctx
            .process
            .read_memory(record + items_txt::DESC_STR_ID)
            .ok()?;
        if sid == 0 || sid == 0xFFFF {
            return None;
        }
        let raw = injector
            .get_string(&self.state.ctx.process, sid, 500)
            .ok()?;
        let clean = strip_color_codes(&raw);
        if clean.trim().is_empty() {
            return None;
        }

        let stat_section = clean.splitn(2, "\n\n").next().unwrap_or(&clean);
        let lines: Vec<&str> = stat_section
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("Cube ")
            })
            .rev()
            .collect();
        if lines.is_empty() {
            return None;
        }
        Some(lines.join("\n"))
    }

    /// Take the pickup updates produced by the latest `tick_items` call.
    pub fn drain_pickup_updates(&mut self) -> Vec<(u32, u32, crate::loot_history::PickupState)> {
        std::mem::take(&mut self.last_pickup_updates)
    }

    /// Take the goblin-detection events produced by the latest `tick_items` call.
    pub fn drain_goblin_events(&mut self) -> Vec<GoblinDetectedEvent> {
        std::mem::take(&mut self.last_goblin_events)
    }
}

/// Strip D2 color codes from string (ÿc followed by color char)
pub(crate) fn strip_color_codes(s: &str) -> String {
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
            if let Err(e) = self.loot_hook.eject(&self.state.ctx) {
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
    pub fn new(
        _loot_history: Arc<RwLock<crate::loot_history::LootHistory>>,
    ) -> Result<Self, String> {
        Err("Not supported on this OS".to_string())
    }

    pub fn drain_pickup_updates(&mut self) -> Vec<(u32, u32, crate::loot_history::PickupState)> {
        Vec::new()
    }

    pub fn drain_goblin_events(&mut self) -> Vec<GoblinDetectedEvent> {
        Vec::new()
    }

    pub fn set_filter_config(&mut self, _config: Arc<RwLock<FilterConfig>>) {}

    pub fn on_filter_config_changed(&mut self) {}

    pub fn set_filter_enabled(&mut self, _enabled: bool) {}

    pub fn set_verbose_filter_logging(&mut self, _enabled: bool) {}

    pub fn set_force_show_all(&self, _value: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn is_filter_enabled(&self) -> bool {
        false
    }

    pub fn is_ingame(&self) -> bool {
        false
    }

    pub fn set_always_show_items(&self, _on: bool) -> Result<bool, String> {
        Ok(false)
    }

    pub fn read_always_show_items(&self) -> Result<Option<bool>, String> {
        Ok(None)
    }

    pub fn clear_cache(&mut self) {}

    pub fn context(&self) -> ! {
        panic!("Not supported on this OS")
    }

    pub fn tick_items(&mut self) -> Vec<ItemDropEvent> {
        Vec::new()
    }
}
