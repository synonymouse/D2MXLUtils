//! Drop Notifier - scans ground items and emits events for matching items
//!
//! This module implements the core NotifierMain logic from D2Stats.au3

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

#[cfg(any(target_os = "windows", target_os = "linux"))]
use std::sync::atomic::Ordering;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::d2types::{ItemData, ScannedItem, UnitAny};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::hook_bit_tracker::{
    HookBitTracker, HookCleanupFailureLogThrottle, PendingVisibilityMaskOps,
};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::injection::D2Injector;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::logger::{error as log_error, info as log_info};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::loot_filter_hook::{visibility_mask_ops, LootFilterHook, VisibilityMaskOp};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::offsets::{
    d2client, d2common, d2sigma, data_tables, inventory, item_data, item_quality, items_txt, paths,
    set_items_txt, stat_list, unique_items_txt, unit, unit_type,
};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::process::D2Context;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::rules::{FilterConfig, MatchContext, PartialFilterDecision, Visibility};
use crate::rules::{ItemTier, Notification};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::scanner_state::{BfsItemCandidate, CachedFilterDecision, SharedScannerState};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use tauri::{AppHandle, Manager};

/// MonStats.txt class IDs that count as "goblins" for the alert sound.
/// Ported verbatim from `D2Stats.au3:$g_goblinIds`.
#[cfg(any(target_os = "windows", target_os = "linux"))]
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
#[cfg(any(target_os = "windows", target_os = "linux"))]
const MAX_ITEM_SCAN_PATHS: usize = 1024;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const MAX_ITEM_SCAN_UNITS_PER_PATH: usize = 4096;

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn capped_item_scan_path_count(i_paths: usize) -> (usize, bool) {
    let capped = i_paths.min(MAX_ITEM_SCAN_PATHS);
    (capped, i_paths > capped)
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn item_scan_unit_index_in_bounds(index: usize) -> bool {
    index < MAX_ITEM_SCAN_UNITS_PER_PATH
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn should_enrich_bfs_candidate(
    candidate: &BfsItemCandidate,
    current_item_ids: &HashSet<u32>,
    recent_filter_decisions: &HashMap<u32, CachedFilterDecision>,
    current_generation: u64,
) -> bool {
    if current_item_ids.contains(&candidate.unit_id) {
        return false;
    }
    !matches!(
        recent_filter_decisions.get(&candidate.unit_id),
        Some(decision) if decision.generation == current_generation
    )
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn visibility_mask_op_description(op: VisibilityMaskOp) -> &'static str {
    match op {
        VisibilityMaskOp::SetShow => "force-show",
        VisibilityMaskOp::SetHide => "hide",
        VisibilityMaskOp::ClearShow => "clear force-show bit for",
        VisibilityMaskOp::ClearHide => "clear hide bit for",
    }
}

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
/// through the ptrace-hijack machinery in `process.rs`, which is far
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

#[cfg(any(target_os = "windows", target_os = "linux"))]
const MATCHING_CACHE_FILE: &str = "matching-cache.json";
#[cfg(any(target_os = "windows", target_os = "linux"))]
const MATCHING_CACHE_SCHEMA_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(any(target_os = "windows", target_os = "linux"))]
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MatchingCacheFile {
    schema: String,
    cache: MatchingCache,
    dumped_at: String,
}

/// Mirrors `weapon_families::load_from_cache`'s pattern (schema-versioned
/// JSON in the app data dir, `None` on any miss/mismatch so the caller
/// falls back to a live rebuild).
#[cfg(any(target_os = "windows", target_os = "linux"))]
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

#[cfg(any(target_os = "windows", target_os = "linux"))]
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

    pub fn set_no_pickup(&self, on: bool) -> Result<(), String> {
        let (addr, bytes) = no_pickup_flag_write(self.state.ctx.d2_client, on);
        self.state.ctx.process.write_buffer(addr, &bytes)
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

    fn reset_departed_mask_collision(
        &mut self,
        unit_id: u32,
        current_item_ids: &HashSet<u32>,
    ) -> bool {
        let colliding_departed_ids = self
            .hook_bits
            .departed_mask_collisions(unit_id, current_item_ids);
        if colliding_departed_ids.is_empty() {
            return true;
        }

        match self
            .loot_hook
            .clear_unit_id_bits(&self.state.ctx, &[unit_id])
        {
            Ok(()) => {
                self.hook_bits.confirm_cleared(&colliding_departed_ids);
                true
            }
            Err(e) => {
                log_error(&format!(
                    "Failed to reset stale hook bits for fresh colliding item {}: {}",
                    unit_id, e
                ));
                false
            }
        }
    }

    fn retry_pending_visibility_ops(&mut self, unit_id: u32) {
        if !self.loot_hook.is_injected() {
            return;
        }

        let ops = self.pending_visibility_ops.take(unit_id);
        if ops.is_empty() {
            return;
        }

        let failed_ops = self.apply_visibility_mask_ops(unit_id, &ops);
        self.pending_visibility_ops
            .record_failed(unit_id, failed_ops);
        self.hook_bits.mark_written(unit_id);
    }

    fn apply_visibility_mask_ops(
        &self,
        unit_id: u32,
        ops: &[VisibilityMaskOp],
    ) -> Vec<VisibilityMaskOp> {
        let mut failed_ops = Vec::new();
        for &op in ops {
            if let Err(e) = self.apply_visibility_mask_op(unit_id, op) {
                log_error(&format!(
                    "Failed to {} item {}: {}",
                    visibility_mask_op_description(op),
                    unit_id,
                    e
                ));
                failed_ops.push(op);
            }
        }
        failed_ops
    }

    fn apply_visibility_mask_op(&self, unit_id: u32, op: VisibilityMaskOp) -> Result<(), String> {
        match op {
            VisibilityMaskOp::SetShow => self.loot_hook.add_shown_unit_id(&self.state.ctx, unit_id),
            VisibilityMaskOp::SetHide => {
                self.loot_hook.add_hidden_unit_id(&self.state.ctx, unit_id)
            }
            VisibilityMaskOp::ClearShow => {
                self.loot_hook.clear_shown_unit_id(&self.state.ctx, unit_id)
            }
            VisibilityMaskOp::ClearHide => self
                .loot_hook
                .clear_hidden_unit_id(&self.state.ctx, unit_id),
        }
    }

    /// Get a reference to the D2Context
    pub fn context(&self) -> &D2Context {
        &self.state.ctx
    }

    fn process_scanned_item(&mut self, scanned: ScannedItem, events: &mut Vec<ItemDropEvent>) {
        let p_unit = scanned.p_unit;
        let mut event = self.to_event(scanned);
        let unit_id = event.unit_id;

        let mut should_emit = true;
        let mut hook_bits_may_exist = false;
        let mut cached_filter_decision = None;
        let filter_snapshot = {
            let guard = self.state.filter_config.read().unwrap();
            guard.as_ref().map(|filter_arc| {
                (
                    filter_arc.clone(),
                    self.state.filter_generation.load(Ordering::SeqCst),
                )
            })
        };
        if let Some((filter_arc, filter_generation)) = filter_snapshot {
            if let Ok(filter) = filter_arc.read() {
                let decision = loop {
                    let ctx = MatchContext::new(&event);
                    match filter.decide_partial(&ctx) {
                        PartialFilterDecision::Ready(decision) => break decision,
                        PartialFilterDecision::Needs(needs) => {
                            let before_stats = event.runtime_stats_loaded;

                            if needs.runtime_stats {
                                self.enrich_event_stats(&mut event, p_unit);
                            }

                            if before_stats == event.runtime_stats_loaded {
                                let ctx = MatchContext::new(&event);
                                break filter.decide(&ctx);
                            }
                        }
                    }
                };

                if self.live_match_highlight {
                    if let Some(line) = decision.matched_line {
                        self.pending_matched_lines.push(line);
                    }
                }

                cached_filter_decision = Some(CachedFilterDecision::from_decision(
                    filter_generation,
                    &decision,
                ));

                if self.verbose_filter_logging {
                    let ctx = MatchContext::new(&event);
                    let winner = filter.rules.iter().rev().find(|r| ctx.matches(r));
                    let reason = match winner {
                        Some(r) => {
                            format!("winner={}", r.name_pattern.as_deref().unwrap_or("<any>"))
                        }
                        None => {
                            format!("no rule matched (hide_all={})", filter.hide_all)
                        }
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

                if decision
                    .notification
                    .as_ref()
                    .map(|n| n.display_stats)
                    .unwrap_or(false)
                {
                    self.enrich_event_stats(&mut event, p_unit);
                }

                if self.loot_hook.is_injected() {
                    hook_bits_may_exist = true;
                    let failed_ops = self.apply_visibility_mask_ops(
                        event.unit_id,
                        visibility_mask_ops(decision.visibility),
                    );
                    self.pending_visibility_ops
                        .record_failed(event.unit_id, failed_ops);
                }

                match decision.notification {
                    Some(n) => event.filter = Some(n),
                    None => should_emit = false,
                }
            }
        }

        // Cache enriched event for the map-marker pass.
        self.state
            .recent_events
            .write()
            .unwrap()
            .insert(event.unit_id, event.clone());
        if let Some(decision) = cached_filter_decision {
            self.state
                .recent_filter_decisions
                .write()
                .unwrap()
                .insert(event.unit_id, decision);
        } else {
            self.state
                .recent_filter_decisions
                .write()
                .unwrap()
                .remove(&event.unit_id);
        }

        if should_emit {
            // Push to session history (only filter-matched items — same gate
            // as overlay notifications).
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
                // Only fresh inserts emit `loot-history-entry`; dedup-merges
                // silently update the existing row (frontend keys by `seed`).
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

        // Keep after show/hide writes: inspected releases the trampoline gate.
        if self.loot_hook.is_injected() {
            hook_bits_may_exist = true;
            if let Err(e) = self
                .loot_hook
                .add_inspected_unit_id(&self.state.ctx, unit_id)
            {
                log_error(&format!("Failed to mark item {} inspected: {}", unit_id, e));
            }
        }
        if hook_bits_may_exist {
            self.hook_bits.mark_written(unit_id);
        }
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

        // Sampled once per tick (not per item) — clvl/class don't change
        // between items in the same scan pass.
        {
            let injector = self.state.injector.lock().unwrap();
            if let Ok(n) = injector.get_unit_stat(
                &self.state.ctx.process,
                ptr1 as u32,
                stat_list::STAT_LEVEL as u32,
            ) {
                self.char_level = n;
            }
        }
        if let Ok(class) = self
            .state
            .ctx
            .process
            .read_memory::<u32>(ptr1 + unit::CLASS)
        {
            self.player_class = class;
        }

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
        let (path_count, path_count_capped) = capped_item_scan_path_count(i_paths);
        if path_count_capped {
            log_error(&format!(
                "item scan path count cap hit; scanning first {} paths",
                MAX_ITEM_SCAN_PATHS
            ));
        }

        let mut current_item_ids: HashSet<u32> = HashSet::new();

        // Two passes keep cleanup aware of current ids without storing pUnit snapshots.
        for i in 0..path_count {
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

            let mut units_visited = 0;
            let mut unit_cap_hit = false;
            while p_unit != 0 {
                if !item_scan_unit_index_in_bounds(units_visited) {
                    unit_cap_hit = true;
                    break;
                }
                units_visited += 1;
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

                p_unit = unit.p_next_unit;
            }
            if unit_cap_hit {
                log_error(&format!(
                    "item scan unit cap hit during current-id pass; max={} units per path",
                    MAX_ITEM_SCAN_UNITS_PER_PATH
                ));
            }
        }

        for i in 0..path_count {
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

            let mut units_visited = 0;
            let mut unit_cap_hit = false;
            while p_unit != 0 {
                if !item_scan_unit_index_in_bounds(units_visited) {
                    unit_cap_hit = true;
                    break;
                }
                units_visited += 1;
                let unit: UnitAny = match self.state.ctx.process.read_memory(p_unit as usize) {
                    Ok(u) => u,
                    Err(_) => break,
                };
                let next_unit = unit.p_next_unit;

                if unit.unit_type != unit_type::ITEM {
                    p_unit = next_unit;
                    continue;
                }
                current_item_ids.insert(unit.unit_id);

                if self.loot_hook.is_injected() {
                    let already_seen = self.seen_items.contains(&unit.unit_id);
                    if already_seen {
                        self.retry_pending_visibility_ops(unit.unit_id);
                    } else if !self.reset_departed_mask_collision(unit.unit_id, &current_item_ids) {
                        p_unit = next_unit;
                        continue;
                    }
                }

                if let Some(scanned) = self.scan_unit(p_unit, &unit) {
                    self.process_scanned_item(scanned, &mut events);
                }
                p_unit = next_unit;
            }
            if unit_cap_hit {
                log_error(&format!(
                    "item scan unit cap hit during scan pass; max={} units per path",
                    MAX_ITEM_SCAN_UNITS_PER_PATH
                ));
            }
        }

        let mut bfs_candidates: Vec<BfsItemCandidate> = self
            .state
            .recent_bfs_items
            .read()
            .map(|items| items.values().copied().collect())
            .unwrap_or_default();
        bfs_candidates.sort_by_key(|candidate| candidate.unit_id);
        let current_generation = self.state.filter_generation.load(Ordering::SeqCst);
        let decision_snapshot: HashMap<u32, CachedFilterDecision> = self
            .state
            .recent_filter_decisions
            .read()
            .map(|decisions| decisions.clone())
            .unwrap_or_default();
        for candidate in bfs_candidates {
            let needs_enrichment = should_enrich_bfs_candidate(
                &candidate,
                &current_item_ids,
                &decision_snapshot,
                current_generation,
            );
            if current_item_ids.contains(&candidate.unit_id) {
                continue;
            }

            let unit: UnitAny = match self
                .state
                .ctx
                .process
                .read_memory(candidate.p_unit as usize)
            {
                Ok(unit) => unit,
                Err(_) => continue,
            };
            if unit.unit_type != unit_type::ITEM || unit.unit_id != candidate.unit_id {
                continue;
            }
            current_item_ids.insert(unit.unit_id);

            if self.loot_hook.is_injected() {
                let already_seen = self.seen_items.contains(&unit.unit_id);
                if already_seen {
                    self.retry_pending_visibility_ops(unit.unit_id);
                } else if !self.reset_departed_mask_collision(unit.unit_id, &current_item_ids) {
                    continue;
                }
            }

            if !needs_enrichment {
                continue;
            }
            if let Some(scanned) = self.scan_unit(candidate.p_unit, &unit) {
                self.process_scanned_item(scanned, &mut events);
            }
        }

        // Keep hook-mask cleanup independent from `seen_items` pruning.
        let to_clear = self.hook_bits.plan_clears(&current_item_ids);
        if !to_clear.is_empty() && self.loot_hook.is_injected() {
            match self
                .loot_hook
                .clear_unit_id_bits(&self.state.ctx, &to_clear)
            {
                Ok(()) => {
                    self.hook_bits.confirm_cleared(&to_clear);
                    self.hook_cleanup_failure_logs.reset();
                }
                Err(e) => {
                    if let Some(suppressed) = self.hook_cleanup_failure_logs.record_failure() {
                        let suppressed = if suppressed > 0 {
                            format!(" (suppressed {} repeated cleanup failures)", suppressed)
                        } else {
                            String::new()
                        };
                        log_error(&format!(
                            "Failed to clear hook bits for {} departed items ({} overdue, {} tracked): {}{}",
                            to_clear.len(),
                            self.hook_bits.overdue_len(),
                            self.hook_bits.tracked_len(),
                            e,
                            suppressed
                        ));
                    }
                }
            }
        } else {
            self.hook_cleanup_failure_logs.reset();
        }

        // dwUnitId stays stable when an item moves between ground and
        // inventory, so without pruning a re-dropped item would never notify.
        self.seen_items.retain(|id| current_item_ids.contains(id));
        self.pending_visibility_ops
            .retain_current(&current_item_ids);
        self.state
            .recent_events
            .write()
            .unwrap()
            .retain(|id, _| current_item_ids.contains(id));
        self.state
            .recent_filter_decisions
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

        if self.debug_get_item_stats_calls > 0 {
            if self.verbose_filter_logging {
                log_info(&format!(
                    "[Filter] runtime enrichment calls: GetItemStats={}",
                    self.debug_get_item_stats_calls
                ));
            }
            self.debug_get_item_stats_calls = 0;
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

        // Create scanned item and keep the existing socket-count enrichment only.
        let mut scanned = ScannedItem::from_unit(unit, &item_data, p_unit);

        {
            let injector = self.state.injector.lock().unwrap();
            if item_data.is_socketed() {
                if let Ok(n) = injector.get_unit_stat(&self.state.ctx.process, p_unit, 0xC2) {
                    scanned.sockets = n.min(6) as u8;
                }
            }
        }

        // Mark as seen
        self.seen_items.insert(unit.unit_id);

        Some(scanned)
    }

    fn enrich_event_stats(&mut self, event: &mut ItemDropEvent, p_unit: u32) {
        if event.runtime_stats_loaded {
            return;
        }

        self.debug_get_item_stats_calls += 1;

        let injector = self.state.injector.lock().unwrap();
        match injector.get_item_stats(&self.state.ctx.process, p_unit) {
            Ok(raw_stats) => {
                let cleaned = strip_color_codes(&raw_stats);
                if !cleaned.trim().is_empty() {
                    let reversed: Vec<&str> = cleaned.lines().rev().collect();
                    let mut stats = Self::format_event_stats(event.sockets, reversed.join("\n"));
                    if event.quality == "Unique" || event.quality == "Set" {
                        stats = crate::unique_stats_db::annotate_with_roll_ranges(
                            &self.state.unique_stats_db,
                            &event.name,
                            event.tier,
                            &stats,
                        );
                    }
                    event.stats = stats;
                    event.runtime_stats_loaded = true;
                }
            }
            Err(e) => {
                if self.verbose_filter_logging {
                    log_error(&format!("get_item_stats failed for unit {}: {}", p_unit, e));
                }
            }
        }

        if !event.runtime_stats_loaded {
            if let Some(text) = self.read_item_desc_from_txt(&injector, event.class) {
                event.stats = Self::format_event_stats(event.sockets, text);
                event.runtime_stats_loaded = true;
            }
        }
    }

    /// Convert a scanned item into an event payload for the frontend.
    fn to_event(&self, scanned: ScannedItem) -> ItemDropEvent {
        let class = scanned.class;
        let quality = scanned.quality_name().to_string();
        let base_name = self.class_base_name(class);
        let mut name = self.static_display_name(&scanned, &base_name);
        let runtime_stats_loaded = scanned.stats.is_some();
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
        let stats = Self::format_event_stats(scanned.sockets, raw_stats);
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
            base_name,
            category: self.class_category(class),
            name,
            stats,
            name_is_runtime: false,
            runtime_stats_loaded,
            is_ethereal: scanned.is_ethereal,
            is_identified: scanned.is_identified,
            p_unit_data: scanned.p_unit_data,
            seed,
            history_pushed: false,
            tier: self.class_tier(class),
            unique_kind,
            sockets: scanned.sockets,
            clvl: self.char_level,
            ilvl: scanned.item_level,
            player_class: self.player_class,
            filter: None,
        }
    }

    fn format_event_stats(sockets: u8, raw_stats: String) -> String {
        if sockets > 0 {
            if raw_stats.is_empty() {
                format!("Socketed ({})", sockets)
            } else {
                format!("Socketed ({})\n{}", sockets, raw_stats)
            }
        } else {
            raw_stats
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

    fn static_display_name(&self, scanned: &ScannedItem, base_name: &str) -> String {
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
    fn read_item_desc_from_txt(&self, injector: &D2Injector, class: u32) -> Option<String> {
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
mod no_pickup_tests {
    use super::*;

    #[test]
    fn no_pickup_write_targets_d2client_flag_byte() {
        let (addr, bytes) = no_pickup_flag_write(0x1000_0000, true);

        assert_eq!(addr, 0x1000_0000 + d2client::NO_PICKUP_FLAG);
        assert_eq!(bytes, [1]);
    }

    #[test]
    fn no_pickup_write_can_disable_flag() {
        let (_, bytes) = no_pickup_flag_write(0x1000_0000, false);

        assert_eq!(bytes, [0]);
    }
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    use crate::scanner_state::{BfsItemCandidate, CachedFilterDecision};

    #[test]
    fn bfs_candidate_enrichment_skips_current_scan_and_current_cached_decision() {
        let candidate = BfsItemCandidate {
            unit_id: 42,
            p_unit: 0x1000,
            sub_x: 10,
            sub_y: 20,
        };
        let mut current_item_ids = HashSet::new();
        let mut decisions = HashMap::new();

        assert!(should_enrich_bfs_candidate(
            &candidate,
            &current_item_ids,
            &decisions,
            7
        ));

        current_item_ids.insert(42);
        assert!(!should_enrich_bfs_candidate(
            &candidate,
            &current_item_ids,
            &decisions,
            7
        ));

        current_item_ids.clear();
        decisions.insert(
            42,
            CachedFilterDecision {
                generation: 7,
                visibility: Visibility::Show,
                place_on_map: true,
            },
        );
        assert!(!should_enrich_bfs_candidate(
            &candidate,
            &current_item_ids,
            &decisions,
            7
        ));

        decisions.get_mut(&42).unwrap().generation = 6;
        assert!(should_enrich_bfs_candidate(
            &candidate,
            &current_item_ids,
            &decisions,
            7
        ));
    }

    #[test]
    fn item_scan_path_count_is_capped() {
        assert_eq!(capped_item_scan_path_count(0), (0, false));
        assert_eq!(capped_item_scan_path_count(12), (12, false));
        assert_eq!(
            capped_item_scan_path_count(MAX_ITEM_SCAN_PATHS + 1),
            (MAX_ITEM_SCAN_PATHS, true)
        );
    }

    #[test]
    fn item_scan_unit_walk_stops_at_marker_bfs_cap() {
        assert!(item_scan_unit_index_in_bounds(0));
        assert!(item_scan_unit_index_in_bounds(
            MAX_ITEM_SCAN_UNITS_PER_PATH - 1
        ));
        assert!(!item_scan_unit_index_in_bounds(
            MAX_ITEM_SCAN_UNITS_PER_PATH
        ));
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
