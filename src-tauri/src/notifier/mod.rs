//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use crate::{rules, AppState};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod catalog;
mod dictionary_cache;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod discovery;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod event;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod pickup;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod processing;
mod visibility;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use catalog::{load_matching_cache, save_matching_cache};
pub(crate) use dictionary_cache::{load_items_cache, save_items_cache};
pub(crate) use visibility::{
    update_reveal_hidden_hotkey, RevealHiddenState, __cmd__update_reveal_hidden_hotkey,
};

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
use discovery::{
    capped_item_scan_path_count, item_scan_unit_index_in_bounds, should_enrich_bfs_candidate,
    MAX_ITEM_SCAN_PATHS, MAX_ITEM_SCAN_UNITS_PER_PATH,
};

#[cfg(all(test, target_os = "windows"))]
#[path = "stat_acquisition_tests.rs"]
mod stat_acquisition_tests;

use std::sync::atomic::Ordering;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use self::visibility::LootFilterHook;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use self::visibility::{HookBitTracker, HookCleanupFailureLogThrottle, PendingVisibilityMaskOps};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::logger::error as log_error;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::offsets::d2client;
#[cfg(all(test, target_os = "windows"))]
use crate::offsets::{item_data, paths, stat_list, unit, unit_type};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::process::D2Context;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::rules::FilterConfig;
#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
use crate::rules::Visibility;
use crate::rules::{ItemTier, Notification};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::scanner_state::SharedScannerState;

#[derive(Debug, Clone, serde::Serialize)]
pub struct GoblinDetectedEvent {
    pub unit_id: u32,
    pub class: u32,
}

#[tauri::command]
pub(crate) fn get_items_dictionary(state: tauri::State<AppState>) -> ItemsDictionary {
    state
        .items_dictionary
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

/// Set the filter configuration for the scanner
#[tauri::command]
pub(crate) fn set_filter_config(
    config: rules::FilterConfig,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    {
        let mut guard = state
            .filter_config
            .write()
            .map_err(|e| format!("Failed to acquire lock: {}", e))?;
        *guard = Some(config);
    }
    // Bump generation so the scanner thread re-evaluates all ground items
    // (clears hide mask, re-runs rule matching) on the next tick.
    state
        .filter_config_generation
        .fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Enable or disable the per-item `[Filter] ...` log line.
#[tauri::command]
pub(crate) fn set_verbose_filter_logging(enabled: bool, state: tauri::State<AppState>) {
    state
        .verbose_filter_logging
        .store(enabled, Ordering::SeqCst);
}

/// Enable or disable the Loot Filter tab's live "show matches" highlight mode.
#[tauri::command]
pub(crate) fn set_live_match_highlight(enabled: bool, state: tauri::State<AppState>) {
    state.live_match_highlight.store(enabled, Ordering::SeqCst);
}

/// Enable or disable auto-toggling of MXL's "always show items" on game entry.
#[tauri::command]
pub(crate) fn set_auto_always_show_items(enabled: bool, state: tauri::State<AppState>) {
    state
        .auto_always_show_items
        .store(enabled, Ordering::SeqCst);
}

/// Enable or disable auto-enabling Diablo II's no-pickup flag on game entry.
#[tauri::command]
pub(crate) fn set_auto_no_pickup(enabled: bool, state: tauri::State<AppState>) {
    state.auto_no_pickup.store(enabled, Ordering::SeqCst);
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
    /// True only when `name` came from D2Client.GetItemName. Ground-item
    /// scanning should normally keep this false and use table-derived names.
    #[serde(default, skip)]
    pub name_is_runtime: bool,
    /// True when `stats` came from D2Client.GetItemStats or a table fallback.
    /// False means stat-pattern rules cannot be fully decided yet.
    #[serde(default, skip)]
    pub runtime_stats_loaded: bool,
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
    /// Character level of the player at the moment this item dropped.
    /// Sampled once per scan tick (`STAT_LEVEL`), not per item.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub clvl: u32,
    /// Item level (`dwItemLevel`), read directly from `ItemData`.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub ilvl: u32,
    /// Player's character class id (`UnitAny.class`, 0=Amazon..6=Assassin).
    /// Sampled once per scan tick, same cadence as `clvl`.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub player_class: u32,
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
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub struct DropScanner {
    /// Shared state bundle (ctx, injector, filter_config, recent_events).
    /// Owned by this thread; Arc cloned to marker thread in Task 5.
    state: Arc<SharedScannerState>,
    /// Cache of already-seen item IDs (to avoid duplicate notifications)
    seen_items: HashSet<u32>,
    /// When true, log per-item filter decisions (opt-in; noisy).
    verbose_filter_logging: bool,
    /// When true, record the source line of every rule that decides an
    /// item's outcome so the editor can flash it (opt-in; the Loot Filter
    /// tab's "show matches" mode).
    live_match_highlight: bool,
    /// Rule source lines matched since the last drain, deduped in-order.
    /// Drained by the main loop into `filter-rule-matched` events.
    pending_matched_lines: Vec<usize>,
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
    /// Remote hook-mask bits need cleanup even after `seen_items` is pruned.
    hook_bits: HookBitTracker,
    hook_cleanup_failure_logs: HookCleanupFailureLogThrottle,
    pending_visibility_ops: PendingVisibilityMaskOps,
    /// Monster `unit_id`s already announced via `goblin-detected`. Not
    /// pruned by current-scan presence — same `unit_id` only fires once
    /// per scanner lifetime. Cleared by `clear_cache()` (filter swap /
    /// game-entry transitions).
    seen_goblins: HashSet<u32>,
    /// Goblins detected in the latest `tick_items` pass; drained by main
    /// loop into `goblin-detected` events. Same pattern as `last_pickup_updates`.
    last_goblin_events: Vec<GoblinDetectedEvent>,
    debug_get_item_stats_calls: u64,
    /// Player's character level (`STAT_LEVEL`), refreshed once per
    /// `tick_items` call rather than per item — see that fn.
    char_level: u32,
    /// Player's character class id (`UnitAny.class`), refreshed alongside
    /// `char_level`.
    player_class: u32,
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
const MISSED_TICKS_BEFORE_BIT_CLEAR: u8 = 2;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const HOOK_CLEANUP_FAILURE_LOG_SUPPRESSED_TICKS: u32 = 166;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Parse a loot-filter DSL rarity keyword (`tu`/`su`/`ssu`/`sssu`).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tu" => Some(Self::Tu),
            "su" => Some(Self::Su),
            "ssu" => Some(Self::Ssu),
            "sssu" => Some(Self::Sssu),
            _ => None,
        }
    }

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct UniqueInfo {
    display_name: String,
    kind: Option<UniqueKind>,
}

/// The three live matching caches `DropScanner` builds by walking
/// items.txt/UniqueItems.txt/SetItems.txt and resolving each record's name
/// via a `D2Lang.GetStringById` remote call. On Linux those remote calls go
/// through the ptrace-hijack machinery in `process/linux_ptrace.rs`, which is far
/// slower per-call than Windows' `CreateRemoteThread` — building all three
/// caches from scratch (~2500 + ~1800 + ~330 calls) dominated the 5-10s
/// startup delay after launching the game. The underlying game data
/// (item/unique/set names) is static per D2/MXL install, so this is cached
/// to disk (`matching-cache.json`, see `load_matching_cache`/
/// `save_matching_cache`) and reused across attaches instead of being
/// rebuilt from live memory every single time.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MatchingCache {
    class_cache: Vec<ClassInfo>,
    unique_cache: Vec<UniqueInfo>,
    set_cache: Vec<String>,
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

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl DropScanner {
    /// Create a new scanner using the provided shared state.
    /// `ctx` and `injector` are constructed by the caller (app/scanner_runtime/worker.rs) and
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
            live_match_highlight: false,
            pending_matched_lines: Vec::new(),
            loot_hook,
            class_cache: None,
            unique_cache: None,
            set_cache: None,
            loot_history,
            last_pickup_updates: Vec::new(),
            hook_bits: HookBitTracker::new(MISSED_TICKS_BEFORE_BIT_CLEAR),
            hook_cleanup_failure_logs: HookCleanupFailureLogThrottle::new(
                HOOK_CLEANUP_FAILURE_LOG_SUPPRESSED_TICKS,
            ),
            pending_visibility_ops: PendingVisibilityMaskOps::new(),
            seen_goblins: HashSet::new(),
            last_goblin_events: Vec::new(),
            debug_get_item_stats_calls: 0,
            char_level: 0,
            player_class: 0,
        })
    }

    pub fn set_filter_config(&mut self, config: Arc<RwLock<FilterConfig>>) {
        if let Ok(mut guard) = config.write() {
            guard.prepare_for_matching();
        }
        let mut guard = self.state.filter_config.write().unwrap();
        self.state.filter_generation.fetch_add(1, Ordering::SeqCst);
        self.state.recent_filter_decisions.write().unwrap().clear();
        *guard = Some(config);
    }

    pub fn on_filter_config_changed(&mut self) {
        self.clear_cache();
    }

    pub fn set_verbose_filter_logging(&mut self, enabled: bool) {
        self.verbose_filter_logging = enabled;
    }

    pub fn set_live_match_highlight(&mut self, enabled: bool) {
        self.live_match_highlight = enabled;
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

    pub fn clear_cache(&mut self) {
        self.seen_items.clear();
        self.seen_goblins.clear();
        self.pending_visibility_ops.clear();
        self.state.recent_events.write().unwrap().clear();
        self.state.recent_filter_decisions.write().unwrap().clear();
        self.state.recent_bfs_items.write().unwrap().clear();
        let mut hook_masks_cleared = !self.loot_hook.is_injected();
        if self.loot_hook.is_injected() {
            hook_masks_cleared = true;
            if let Err(e) = self.loot_hook.clear_hidden_items(&self.state.ctx) {
                log_error(&format!("Failed to clear hide mask: {}", e));
                hook_masks_cleared = false;
            }
            if let Err(e) = self.loot_hook.clear_shown_items(&self.state.ctx) {
                log_error(&format!("Failed to clear show mask: {}", e));
                hook_masks_cleared = false;
            }
            if let Err(e) = self.loot_hook.clear_inspected_mask(&self.state.ctx) {
                log_error(&format!("Failed to clear inspected mask: {}", e));
                hook_masks_cleared = false;
            }
        }
        if hook_masks_cleared {
            self.hook_bits.clear();
            self.hook_cleanup_failure_logs.reset();
        }
    }

    /// Get a reference to the D2Context
    pub fn context(&self) -> &D2Context {
        &self.state.ctx
    }

    /// Take the pickup updates produced by the latest `tick_items` call.
    pub fn drain_pickup_updates(&mut self) -> Vec<(u32, u32, crate::loot_history::PickupState)> {
        std::mem::take(&mut self.last_pickup_updates)
    }

    /// Take the goblin-detection events produced by the latest `tick_items` call.
    pub fn drain_goblin_events(&mut self) -> Vec<GoblinDetectedEvent> {
        std::mem::take(&mut self.last_goblin_events)
    }

    /// Take the rule source lines matched since the last drain (only
    /// populated while `live_match_highlight` is enabled).
    pub fn drain_matched_lines(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.pending_matched_lines)
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn no_pickup_flag_write(d2_client: usize, on: bool) -> (usize, [u8; 1]) {
    (d2_client + d2client::NO_PICKUP_FLAG, [u8::from(on)])
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod no_pickup_tests;

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests;

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

#[cfg(any(target_os = "windows", target_os = "linux"))]
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

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
use crate::rules::FilterConfig;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub struct DropScanner;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
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

    pub fn drain_matched_lines(&mut self) -> Vec<usize> {
        Vec::new()
    }

    pub fn set_filter_config(&mut self, _config: Arc<RwLock<FilterConfig>>) {}

    pub fn on_filter_config_changed(&mut self) {}

    pub fn set_verbose_filter_logging(&mut self, _enabled: bool) {}

    pub fn set_live_match_highlight(&mut self, _enabled: bool) {}

    pub fn set_force_show_all(&self, _value: bool) -> Result<(), String> {
        Ok(())
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

    pub fn set_no_pickup(&self, _on: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn clear_cache(&mut self) {}

    pub fn context(&self) -> ! {
        panic!("Not supported on this OS")
    }

    pub fn tick_items(&mut self) -> Vec<ItemDropEvent> {
        Vec::new()
    }
}
