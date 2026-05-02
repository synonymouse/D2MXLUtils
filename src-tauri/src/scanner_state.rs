//! State shared by the items and marker scanner threads. Wrap-once at
//! startup, clone the outer `Arc` per thread. `injector` and
//! `recent_events` locks must never be held simultaneously.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use crate::injection::D2Injector;
use crate::notifier::ItemDropEvent;
use crate::process::D2Context;
use crate::rules::FilterConfig;

pub struct SharedScannerState {
    pub ctx: Arc<D2Context>,
    pub injector: Arc<Mutex<D2Injector>>,
    pub filter_config: RwLock<Option<Arc<RwLock<FilterConfig>>>>,
    pub filter_enabled: AtomicBool,
    /// Enriched events keyed by `dwUnitId`; marker thread snapshots for
    /// regex matching.
    pub recent_events: RwLock<HashMap<u32, ItemDropEvent>>,
    /// Items thread sets on game-entry; marker thread swap-clears at top
    /// of tick.
    pub clear_markers: AtomicBool,
    pub stop: AtomicBool,
}

impl SharedScannerState {
    pub fn new(ctx: D2Context, injector: D2Injector) -> Self {
        Self {
            ctx: Arc::new(ctx),
            injector: Arc::new(Mutex::new(injector)),
            filter_config: RwLock::new(None),
            filter_enabled: AtomicBool::new(false),
            recent_events: RwLock::new(HashMap::new()),
            clear_markers: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        }
    }
}
