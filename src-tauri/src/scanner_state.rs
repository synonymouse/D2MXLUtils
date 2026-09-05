//! State shared by the items and marker scanner threads. Wrap-once at
//! startup, clone the outer `Arc` per thread. `injector` and
//! `recent_events` locks must never be held simultaneously.
//!
//! `hovered_item_hook` and `dps_hook` are inline-code-hooking subsystems,
//! both ported to Linux (see `dps_hook/mod.rs`'s and `hovered_item.rs`'s
//! `ProcessRef`-based splits). Everything else here is OS-agnostic once
//! `process.rs`/`injection.rs` provide a `D2Context`/`D2Injector` for the
//! current OS.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::dps_hook::DpsHook;
use crate::dps_meter::DpsMeter;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::hovered_item::HoveredItemHook;
use crate::injection::D2Injector;
use crate::notifier::ItemDropEvent;
use crate::offsets::d2client;
use crate::process::D2Context;
use crate::rules::{FilterConfig, FilterDecision, Visibility};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedFilterDecision {
    pub generation: u64,
    pub visibility: Visibility,
    pub place_on_map: bool,
}

impl CachedFilterDecision {
    pub fn from_decision(generation: u64, decision: &FilterDecision) -> Self {
        Self {
            generation,
            visibility: decision.visibility,
            place_on_map: decision.place_on_map,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BfsItemCandidate {
    pub unit_id: u32,
    pub p_unit: u32,
    pub sub_x: i32,
    pub sub_y: i32,
}

pub struct SharedScannerState {
    pub ctx: Arc<D2Context>,
    pub injector: Arc<Mutex<D2Injector>>,
    pub filter_config: RwLock<Option<Arc<RwLock<FilterConfig>>>>,
    pub filter_generation: AtomicU64,
    /// Enriched events keyed by `dwUnitId`; pruned to currently visible items.
    pub recent_events: RwLock<HashMap<u32, ItemDropEvent>>,
    /// Runtime-only filter decisions keyed by `dwUnitId`; marker thread uses
    /// these instead of re-running the full filter.
    pub recent_filter_decisions: RwLock<HashMap<u32, CachedFilterDecision>>,
    /// Raw item candidates found by marker BFS. The item scanner consumes this
    /// as a secondary discovery source and remains the only enrichment path.
    pub recent_bfs_items: RwLock<HashMap<u32, BfsItemCandidate>>,
    /// Items thread sets on game-entry; marker thread swap-clears at top
    /// of tick.
    pub clear_markers: AtomicBool,
    pub stop: AtomicBool,
    /// Set by the `refresh_game_data_caches` command; the items thread
    /// swap-clears it at top of tick and rebuilds class/unique/set caches
    /// plus the weapon-base catalog from current game memory. Lets a
    /// stale-cache recovery (e.g. after an MXL content patch) happen
    /// without an app restart.
    pub refresh_requested: AtomicBool,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub dps_hook: Arc<DpsHook>,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub hovered_item_hook: Arc<HoveredItemHook>,
    pub dps_meter: Arc<RwLock<DpsMeter>>,
    /// Last observed `*pAutomapLayer`. Sentinel `-1` = uninitialised
    /// (first read records, doesn't reset). `i64` so any 32-bit pointer
    /// value (incl. 0) round-trips losslessly.
    pub last_area_token: AtomicI64,
    /// Local unique/set roll-range template DB — see `unique_stats_db.rs`.
    /// Empty (not an `Option`) when no local DB file was found, so lookups
    /// are just always-miss rather than needing an extra `is_some` check
    /// at every call site.
    pub unique_stats_db: crate::unique_stats_db::UniqueStatsDb,
}

impl SharedScannerState {
    pub fn new(
        ctx: D2Context,
        injector: D2Injector,
        unique_stats_db: crate::unique_stats_db::UniqueStatsDb,
    ) -> Self {
        Self {
            ctx: Arc::new(ctx),
            injector: Arc::new(Mutex::new(injector)),
            filter_config: RwLock::new(None),
            filter_generation: AtomicU64::new(0),
            recent_events: RwLock::new(HashMap::new()),
            recent_filter_decisions: RwLock::new(HashMap::new()),
            recent_bfs_items: RwLock::new(HashMap::new()),
            clear_markers: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            refresh_requested: AtomicBool::new(false),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            dps_hook: Arc::new(DpsHook::new()),
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            hovered_item_hook: Arc::new(HoveredItemHook::new()),
            dps_meter: Arc::new(RwLock::new(DpsMeter::new())),
            last_area_token: AtomicI64::new(-1),
            unique_stats_db,
        }
    }

    /// `*pAutomapLayer` is stable inside one area and rotated by the
    /// engine on every transition — used as the DPS-meter auto-reset
    /// trigger. `None` outside gameplay (main menu, loading screen).
    pub fn read_current_area_token(&self) -> Option<u32> {
        let layer = self
            .ctx
            .process
            .read_memory::<u32>(self.ctx.d2_client + d2client::AUTOMAP_LAYER)
            .ok()?;
        if layer == 0 {
            None
        } else {
            Some(layer)
        }
    }
}
