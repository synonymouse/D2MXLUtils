//! Attach/bootstrap, item/marker coordination and ordered shutdown.

use super::is_diablo2_running;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use super::readouts;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::logger::{error as log_error, info as log_info};
use crate::loot_history::{LootHistory, PickupState};
use crate::{breakpoints, damage_stats, notifier, rules, stat_telemetry, stats_panel};
use crate::{GAME_STATUS_INGAME, GAME_STATUS_MENU, GAME_STATUS_UNKNOWN};

use notifier::{DropScanner, ItemsDictionary};

const MARKER_SCAN_INTERVAL_MS: u64 = 100;

/// Spawn the marker-scanner thread. Cancellation is checked at the top of
/// each iteration, so a `stop`-then-join may block until the in-flight BFS
/// finishes (~700 ms worst case in release).
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn spawn_marker_thread(
    state: Arc<crate::scanner_state::SharedScannerState>,
) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("marker-scanner".into())
        .spawn(move || {
            let mut marker_scanner = crate::map_markers::MarkerScanner::new(state.clone());
            loop {
                if state.stop.load(Ordering::Relaxed) {
                    break;
                }
                if state.clear_markers.swap(false, Ordering::Relaxed) {
                    marker_scanner.clear();
                }
                marker_scanner.tick();
                thread::sleep(Duration::from_millis(MARKER_SCAN_INTERVAL_MS));
            }
            marker_scanner.shutdown();
        })
        .map_err(|e| {
            log_error(&format!("Failed to spawn marker-scanner thread: {}", e));
            e
        })
        .ok()
}

/// Start the scanner (internal function used by auto-start and manual start)
pub(super) fn start_scanner_internal(
    is_scanning: Arc<AtomicBool>,
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    verbose_filter_logging: Arc<AtomicBool>,
    live_match_highlight: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    auto_no_pickup: Arc<AtomicBool>,
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    loot_history: Arc<RwLock<LootHistory>>,
    breakpoints_polling: Arc<AtomicBool>,
    stats_polling: Arc<AtomicBool>,
    weapon_base_catalog: Arc<RwLock<Option<breakpoints::WeaponBaseCatalog>>>,
    dps_reset_pending: Arc<AtomicBool>,
    #[cfg(any(target_os = "windows", target_os = "linux"))] scanner_shared_state: Arc<
        RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
    >,
    #[cfg(any(target_os = "windows", target_os = "linux"))] attach_failure_streak: Arc<AtomicU32>,
    app_handle: AppHandle,
) {
    // Check if already running
    if is_scanning.load(Ordering::SeqCst) {
        return;
    }

    if let Some(prev) = scanner_thread.lock().unwrap().take() {
        let _ = prev.join();
    }

    // Set scanning flag
    is_scanning.store(true, Ordering::SeqCst);

    // Emit status to frontend
    if let Err(e) = app_handle.emit("scanner-status", "starting") {
        log_error(&format!("Failed to emit event (starting): {}", e));
    }

    // Spawn background scanning thread
    let handle = thread::Builder::new()
        .name("drop-scanner".into())
        .spawn(move || {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let (shared_state, mut scanner) = {
                let ctx = match crate::process::D2Context::new() {
                    Ok(c) => c,
                    Err(e) => {
                        log_error(&format!("Failed to attach to Diablo II: {}", e));
                        if let Err(e) = app_handle.emit("scanner-status", "error") {
                            log_error(&format!("Failed to emit event (error): {}", e));
                        }
                        if let Some(overlay) = app_handle.get_webview_window("overlay") {
                            if let Err(e) = overlay.hide() {
                                log_error(&format!(
                                    "Failed to hide overlay window after scanner attach error: {}",
                                    e
                                ));
                            }
                        }
                        is_scanning.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                let injector = match crate::injection::D2Injector::new(
                    &ctx.process,
                    ctx.d2_client,
                    ctx.d2_common,
                    ctx.d2_lang,
                ) {
                    Ok(i) => {
                        attach_failure_streak.store(0, Ordering::Relaxed);
                        i
                    }
                    Err(e) => {
                        attach_failure_streak.fetch_add(1, Ordering::Relaxed);
                        log_error(&format!("Failed to create D2Injector: {}", e));
                        if let Err(e) = app_handle.emit("scanner-status", "error") {
                            log_error(&format!("Failed to emit event (error): {}", e));
                        }
                        if let Some(overlay) = app_handle.get_webview_window("overlay") {
                            if let Err(e) = overlay.hide() {
                                log_error(&format!(
                                    "Failed to hide overlay window after scanner attach error: {}",
                                    e
                                ));
                            }
                        }
                        is_scanning.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                let unique_stats_db = crate::unique_stats_db::load_unique_stats_db(&app_handle)
                    .unwrap_or_default();
                let shared_state = Arc::new(crate::scanner_state::SharedScannerState::new(
                    ctx,
                    injector,
                    unique_stats_db,
                ));
                let scanner = match DropScanner::new(shared_state.clone(), loot_history.clone()) {
                    Ok(s) => {
                        log_info("Scanner attached to Diablo II");
                        if let Err(e) = app_handle.emit("scanner-status", "running") {
                            log_error(&format!("Failed to emit event (running): {}", e));
                        }
                        s
                    }
                    Err(e) => {
                        log_error(&format!("Failed to attach to Diablo II: {}", e));
                        if let Err(e) = app_handle.emit("scanner-status", "error") {
                            log_error(&format!("Failed to emit event (error): {}", e));
                        }
                        if let Some(overlay) = app_handle.get_webview_window("overlay") {
                            if let Err(e) = overlay.hide() {
                                log_error(&format!(
                                    "Failed to hide overlay window after scanner attach error: {}",
                                    e
                                ));
                            }
                        }
                        is_scanning.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                if let Ok(mut guard) = scanner_shared_state.write() {
                    *guard = Some(shared_state.clone());
                }
                // Non-fatal: loot/notification path keeps working without
                // DPS readings if the hook fails to install.
                #[cfg(target_os = "windows")]
                if let Err(e) = shared_state.dps_hook.install(
                    shared_state.ctx.process.handle,
                    shared_state.ctx.d2_common,
                    shared_state.ctx.d2_client,
                ) {
                    log_error(&format!("DPS hook install failed: {}", e));
                }
                #[cfg(target_os = "linux")]
                if let Err(e) = shared_state.dps_hook.install(
                    shared_state.ctx.process.pid,
                    shared_state.ctx.d2_common,
                    shared_state.ctx.d2_client,
                ) {
                    log_error(&format!("DPS hook install failed: {}", e));
                }
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                if let Err(e) = shared_state.hovered_item_hook.install(&shared_state.ctx) {
                    log_error(&format!("Hovered-item hook install failed: {}", e));
                }

                (shared_state, scanner)
            };

            // Seed the live class/unique/set-item matching caches from
            // disk before the first scan tick, so `DropScanner::tick_items`
            // (which lazily rebuilds any cache that's still `None`) skips
            // the live rebuild entirely on a warm start. That rebuild is
            // thousands of individual GetStringById remote calls — on
            // Linux, each one goes through a ptrace-hijack retry loop far
            // slower than Windows' CreateRemoteThread, so this was the
            // dominant cost of the 5-10s delay after launching the game.
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Some(cache) = notifier::load_matching_cache(&app_handle) {
                scanner.seed_matching_cache(cache);
            }

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let marker_handle = spawn_marker_thread(shared_state.clone());

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            let mut scanner = match DropScanner::new(loot_history.clone()) {
                Ok(s) => {
                    log_info("Scanner attached to Diablo II");
                    if let Err(e) = app_handle.emit("scanner-status", "running") {
                        log_error(&format!("Failed to emit event (running): {}", e));
                    }
                    s
                }
                Err(e) => {
                    log_error(&format!("Failed to attach to Diablo II: {}", e));
                    if let Err(e) = app_handle.emit("scanner-status", "error") {
                        log_error(&format!("Failed to emit event (error): {}", e));
                    }
                    // Ensure overlay is hidden if attachment failed
                    if let Some(overlay) = app_handle.get_webview_window("overlay") {
                        if let Err(e) = overlay.hide() {
                            log_error(&format!(
                                "Failed to hide overlay window after scanner attach error: {}",
                                e
                            ));
                        }
                    }
                    is_scanning.store(false, Ordering::SeqCst);
                    return;
                }
            };

            // Configure filter if available
            let mut last_config_gen = filter_config_generation.load(Ordering::SeqCst);
            if let Ok(guard) = filter_config.read() {
                if let Some(ref config) = *guard {
                    scanner.set_filter_config(Arc::new(RwLock::new(config.clone())));
                    scanner.on_filter_config_changed();
                }
            }
            scanner.set_verbose_filter_logging(verbose_filter_logging.load(Ordering::SeqCst));
            scanner.set_live_match_highlight(live_match_highlight.load(Ordering::SeqCst));

            // Seed the hook with the current flag so a key already held on
            // attach (e.g. user reopened the game) works on frame one.
            let mut last_reveal = reveal_hidden_active.load(Ordering::SeqCst);
            if let Err(e) = scanner.set_force_show_all(last_reveal) {
                log_error(&format!("Initial set_force_show_all failed: {}", e));
            }
            let mut last_auto_no_pickup = auto_no_pickup.load(Ordering::SeqCst);

            let mut was_ingame = false;
            let mut dict_published = false;
            // Skip the live rebuild (hundreds of GetStringById remote
            // calls) if a catalog is already loaded — either from disk at
            // app startup, or from an earlier attach this session.
            let mut weapon_bases_published = weapon_base_catalog
                .read()
                .map(|g| g.is_some())
                .unwrap_or(false);
            // Last successfully-read breakpoint snapshot per unit, reused
            // on a transient stat-read failure so the breakpoints tab
            // doesn't flash to 0 (see `breakpoints::read_unit_breakpoint_data`).
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_player_bp: Option<breakpoints::BreakpointData> = None;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_merc_bp: Option<breakpoints::BreakpointData> = None;
            // Same last-good-snapshot fallback for the Stats tab — a
            // transient `GetUnitStat` failure partway through the ~100-id
            // sweep used to flash individual rows (level, attributes, ...)
            // to 0 before the next successful poll corrected them.
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_player_stats: Option<stats_panel::CharacterStats> = None;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_merc_stats: Option<stats_panel::CharacterStats> = None;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_player_damage: Option<damage_stats::DamageStats> = None;
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut last_merc_damage: Option<damage_stats::DamageStats> = None;
            let mut pending_set_always_show = false;
            let mut pending_set_no_pickup: Option<bool> = None;
            let mut last_emitted_always_show: Option<bool> = None;
            // Area-change check throttle (~150 ms at 30 ms tick).
            let mut dps_area_tick_counter: u32 = 0;
            // Stats now prefer a direct bulk read, but a rejected bulk still
            // needs the legacy ~100-call GetUnitStat sweep per unit. Preserve
            // the existing ~300 ms gate rather than poll on every 30 ms tick.
            let mut stats_tick_counter: u32 = 0;
            const STATS_CHECK_EVERY: u32 = 10;
            // Breakpoints now read direct-first. The legacy path requested
            // 2 units x 6 injected stats per poll; persistent direct failure
            // can retain that pressure. Historically, polling every 30 ms
            // meant roughly 400 injected requests/sec while the tab was open.
            // Keep the existing ~300 ms gate for fallback costs and gear/buff
            // updates; reducing requests does not establish a crash/leak fix.
            let mut breakpoints_tick_counter: u32 = 0;
            const BREAKPOINTS_CHECK_EVERY: u32 = 10;

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let telemetry_snapshot = || stat_telemetry::try_snapshot(
                &shared_state.injector, |injector| injector.telemetry.snapshot(),
            );
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let mut telemetry = stat_telemetry::TelemetrySession::default();
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Some(event) = telemetry.poll(std::time::Instant::now(), telemetry_snapshot) {
                event.log(|| stat_telemetry::memory::sample(&shared_state.ctx.process));
            }

            // Main scanning loop
            while is_scanning.load(Ordering::SeqCst) {
                // Check if D2 is still running
                if !is_diablo2_running() {
                    log_info("Diablo II closed, stopping scanner");
                    break;
                }

                let ingame = scanner.is_ingame();

                game_status.store(
                    if ingame {
                        GAME_STATUS_INGAME
                    } else {
                        GAME_STATUS_MENU
                    },
                    Ordering::SeqCst,
                );

                // Detect entering a new game
                if ingame && !was_ingame {
                    log_info("Entered game");
                    scanner.clear_cache();
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    shared_state
                        .clear_markers
                        .store(true, Ordering::Relaxed);
                    if let Ok(mut hist) = loot_history.write() {
                        hist.clear();
                    }
                    if let Err(e) = app_handle.emit("loot-history-cleared", ()) {
                        log_error(&format!("Failed to emit loot-history-cleared: {}", e));
                    }
                    pending_set_always_show = true;
                    last_auto_no_pickup = auto_no_pickup.load(Ordering::SeqCst);
                    pending_set_no_pickup = last_auto_no_pickup.then_some(true);
                    last_emitted_always_show = None;
                    if let Err(e) = app_handle.emit("game-status", "ingame") {
                        log_error(&format!("Failed to emit event (ingame): {}", e));
                    }
                } else if !ingame && was_ingame {
                    pending_set_always_show = false;
                    pending_set_no_pickup = None;
                    last_emitted_always_show = None;
                    // Exiting to menu: every still-Pending entry is
                    // effectively lost from this session — broadcast each
                    // as a `loot-history-update` so the panel ticks them
                    // over to ⊘. The history is cleared on the next
                    // menu→ingame transition.
                    let pending_to_lost = loot_history
                        .write()
                        .map(|mut h| h.mark_all_pending_lost())
                        .unwrap_or_default();
                    for (unit_id, seed, pickup) in pending_to_lost {
                        #[derive(serde::Serialize)]
                        struct LootHistoryUpdatePayload {
                            unit_id: u32,
                            seed: u32,
                            pickup: PickupState,
                        }
                        let payload = LootHistoryUpdatePayload { unit_id, seed, pickup };
                        if let Err(e) = app_handle.emit("loot-history-update", &payload) {
                            log_error(&format!(
                                "Failed to emit loot-history-update (menu sweep): {}",
                                e
                            ));
                        }
                    }
                    if let Err(e) = app_handle.emit("game-status", "menu") {
                        log_error(&format!("Failed to emit event (menu): {}", e));
                    }
                }
                was_ingame = ingame;

                // Only re-sync config when generation changed (user saved or toggled mode).
                // This avoids reallocating Arcs every tick and also lets us trigger a
                // full re-evaluation of ground items + hide-mask reset on change.
                let current_gen = filter_config_generation.load(Ordering::SeqCst);
                if current_gen != last_config_gen {
                    if let Ok(guard) = filter_config.read() {
                        if let Some(ref config) = *guard {
                            scanner.set_filter_config(Arc::new(RwLock::new(config.clone())));
                            scanner.on_filter_config_changed();
                        }
                    }
                    last_config_gen = current_gen;
                }

                scanner.set_verbose_filter_logging(
                    verbose_filter_logging.load(Ordering::SeqCst),
                );
                scanner.set_live_match_highlight(live_match_highlight.load(Ordering::SeqCst));

                let current_reveal = reveal_hidden_active.load(Ordering::SeqCst);
                if current_reveal != last_reveal {
                    if let Err(e) = scanner.set_force_show_all(current_reveal) {
                        log_error(&format!("set_force_show_all failed: {}", e));
                    }
                    last_reveal = current_reveal;
                }

                let current_auto_no_pickup = auto_no_pickup.load(Ordering::SeqCst);
                if ingame && current_auto_no_pickup != last_auto_no_pickup {
                    pending_set_no_pickup = Some(current_auto_no_pickup);
                    last_auto_no_pickup = current_auto_no_pickup;
                }

                // Scan for items
                if ingame {
                    if pending_set_always_show
                        && auto_always_show_items.load(Ordering::SeqCst)
                    {
                        match scanner.set_always_show_items(true) {
                            Ok(true) => {
                                pending_set_always_show = false;
                            }
                            Ok(false) => {}
                            Err(e) => {
                                log_error(&format!(
                                    "set_always_show_items failed: {}",
                                    e
                                ));
                                pending_set_always_show = false;
                            }
                        }
                    }

                    if let Some(target_no_pickup) = pending_set_no_pickup.take() {
                        if let Err(e) = scanner.set_no_pickup(target_no_pickup) {
                            log_error(&format!("set_no_pickup failed: {}", e));
                        }
                    }

                    // `Ok(None)` = struct not lazy-allocated yet by MXL,
                    // semantically equivalent to flag=false (items hidden).
                    // Surface as `false` so the indicator appears.
                    let observed = match scanner.read_always_show_items() {
                        Ok(Some(state)) => Some(state),
                        Ok(None) => Some(false),
                        Err(e) => {
                            log_error(&format!(
                                "read_always_show_items failed: {}",
                                e
                            ));
                            None
                        }
                    };
                    if let Some(state) = observed {
                        if last_emitted_always_show != Some(state) {
                            if let Err(e) =
                                app_handle.emit("always-show-items-state", state)
                            {
                                log_error(&format!(
                                    "Failed to emit always-show-items-state: {}",
                                    e
                                ));
                            }
                            last_emitted_always_show = Some(state);
                        }
                    }

                    // Split pass: emit notifications first, then run the
                    // (potentially expensive) map-marker BFS. Otherwise
                    // `item-drop` events would wait on the marker pass and
                    // appear with noticeable lag on crowded maps.
                    let items = scanner.tick_items();
                    for item in items {
                        // Only emit loot-history-entry when the scanner
                        // actually inserted a new row (false when a
                        // dedup-merge happened — same physical item seen
                        // again after area reload).
                        if item.history_pushed {
                            #[derive(serde::Serialize, Clone)]
                            struct LootHistoryEntryPayload<'a> {
                                unit_id: u32,
                                seed: u32,
                                timestamp_ms: u64,
                                name: &'a str,
                                quality: &'a str,
                                color: Option<&'a str>,
                                pickup: PickupState,
                            }
                            // Read history once to get the timestamp+color
                            // the scanner just stamped the entry with.
                            let stamped = loot_history.read().ok().and_then(|h| {
                                h.snapshot()
                                    .iter()
                                    .find(|e| e.unit_id == item.unit_id)
                                    .map(|e| (e.timestamp_ms, e.color.clone()))
                            });
                            let (timestamp_ms, color_string) =
                                stamped.unwrap_or((0, None));
                            let payload = LootHistoryEntryPayload {
                                unit_id: item.unit_id,
                                seed: item.seed,
                                timestamp_ms,
                                name: &item.name,
                                quality: &item.quality,
                                color: color_string.as_deref(),
                                pickup: PickupState::Pending,
                            };
                            if let Err(e) = app_handle.emit("loot-history-entry", &payload) {
                                log_error(&format!(
                                    "Failed to emit loot-history-entry: {}",
                                    e
                                ));
                            }
                        }
                        log_info(&format!(
                            "item-drop: {} ({}) [{}] sound={:?}",
                            item.name,
                            item.quality,
                            item.stats,
                            item.filter.as_ref().and_then(|f| f.sound)
                        ));
                        if let Err(e) = app_handle.emit("item-drop", &item) {
                            log_error(&format!("Failed to emit item-drop event: {}", e));
                        }
                    }

                    // Drain pickup-state transitions and broadcast them.
                    for (unit_id, seed, pickup) in scanner.drain_pickup_updates() {
                        #[derive(serde::Serialize)]
                        struct LootHistoryUpdatePayload {
                            unit_id: u32,
                            seed: u32,
                            pickup: PickupState,
                        }
                        let payload = LootHistoryUpdatePayload { unit_id, seed, pickup };
                        if let Err(e) = app_handle.emit("loot-history-update", &payload) {
                            log_error(&format!(
                                "Failed to emit loot-history-update: {}",
                                e
                            ));
                        }
                    }

                    for ev in scanner.drain_goblin_events() {
                        if let Err(e) = app_handle.emit("goblin-detected", &ev) {
                            log_error(&format!("Failed to emit goblin-detected: {}", e));
                        }
                    }

                    let matched_lines = scanner.drain_matched_lines();
                    if !matched_lines.is_empty() {
                        if let Err(e) = app_handle.emit("filter-rule-matched", &matched_lines) {
                            log_error(&format!("Failed to emit filter-rule-matched: {}", e));
                        }
                    }

                    // Manual recovery from a stale on-disk/in-memory cache
                    // (e.g. after an MXL content patch) — see
                    // `refresh_game_data_caches` command. Drop every cache
                    // this loop knows about so the blocks below rebuild
                    // everything live, same as a fresh attach would.
                    if shared_state.refresh_requested.swap(false, Ordering::Relaxed) {
                        log_info("Cache refresh requested — rebuilding item/unique/set/weapon-base caches from live game memory");
                        scanner.clear_matching_cache();
                        dict_published = false;
                        #[cfg(any(target_os = "windows", target_os = "linux"))]
                        {
                            weapon_bases_published = false;
                            if let Ok(mut guard) = weapon_base_catalog.write() {
                                *guard = None;
                            }
                        }
                    }

                    if !dict_published {
                        if let Some(dict) = scanner.items_dictionary_snapshot() {
                            if let Ok(mut guard) = items_dictionary.write() {
                                *guard = Some(dict.clone());
                            }
                            if let Err(e) = notifier::save_items_cache(&app_handle, &dict) {
                                log_error(&format!("Failed to save items cache: {}", e));
                            }
                            #[cfg(any(target_os = "windows", target_os = "linux"))]
                            if let Some(cache) = scanner.matching_cache_snapshot() {
                                if let Err(e) = notifier::save_matching_cache(&app_handle, &cache) {
                                    log_error(&format!("Failed to save matching cache: {}", e));
                                }
                            }
                            if let Err(e) = app_handle.emit("items-dictionary-updated", &dict) {
                                log_error(&format!(
                                    "Failed to emit items-dictionary-updated: {}",
                                    e
                                ));
                            }
                            log_info(&format!(
                                "Published items dictionary ({} base, {} TU, {} SU, {} SSU, {} SSSU, {} set items)",
                                dict.base_types.len(),
                                dict.uniques_tu.len(),
                                dict.uniques_su.len(),
                                dict.uniques_ssu.len(),
                                dict.uniques_sssu.len(),
                                dict.set_items.len()
                            ));
                            dict_published = true;
                        }
                    }

                    // Built once per attach; needs the injector for name
                    // lookups via D2Lang.GetStringById.
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    if !weapon_bases_published {
                        let injector = shared_state.injector.lock().unwrap();
                        match breakpoints::build_weapon_base_catalog(&shared_state.ctx, &injector) {
                            Ok(catalog) => {
                                drop(injector);
                                if let Err(e) =
                                    breakpoints::save_weapon_base_cache(&app_handle, &catalog)
                                {
                                    log_error(&format!(
                                        "Failed to save weapon-bases cache: {}",
                                        e
                                    ));
                                }
                                if let Err(e) =
                                    app_handle.emit("weapon-base-catalog-updated", &catalog)
                                {
                                    log_error(&format!(
                                        "Failed to emit weapon-base-catalog-updated: {}",
                                        e
                                    ));
                                }
                                if let Ok(mut guard) = weapon_base_catalog.write() {
                                    *guard = Some(catalog);
                                }
                                weapon_bases_published = true;
                            }
                            Err(e) => {
                                drop(injector);
                                log_error(&format!(
                                    "Failed to build weapon-base catalog: {}",
                                    e
                                ));
                                // Stop retrying every tick — try again on next attach.
                                weapon_bases_published = true;
                            }
                        }
                    }

                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    if breakpoints_polling.load(Ordering::Relaxed) {
                        breakpoints_tick_counter = breakpoints_tick_counter.wrapping_add(1);
                        if breakpoints_tick_counter % BREAKPOINTS_CHECK_EVERY == 0 {
                            readouts::sample_breakpoints(
                                &shared_state,
                                &app_handle,
                                &mut last_player_bp,
                                &mut last_merc_bp,
                            );
                        }
                    }

                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    if stats_polling.load(Ordering::Relaxed) {
                        stats_tick_counter = stats_tick_counter.wrapping_add(1);
                        if stats_tick_counter % STATS_CHECK_EVERY == 0 {
                            readouts::sample_stats(
                                &shared_state,
                                &app_handle,
                                &mut last_player_damage,
                                &mut last_merc_damage,
                                &mut last_player_stats,
                                &mut last_merc_stats,
                            );
                        }
                    }

                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    readouts::sample_dps(
                        &shared_state,
                        &app_handle,
                        &dps_reset_pending,
                        &mut dps_area_tick_counter,
                    );
                }

                #[cfg(any(target_os = "windows", target_os = "linux"))]
                if let Some(event) = telemetry.poll(std::time::Instant::now(), telemetry_snapshot) {
                    event.log(|| stat_telemetry::memory::sample(&shared_state.ctx.process));
                }
                thread::sleep(Duration::from_millis(30));
            }

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Some(event) = telemetry.finish(std::time::Instant::now(), telemetry_snapshot) {
                event.log(|| stat_telemetry::memory::sample(&shared_state.ctx.process));
            }

            // Restore the DPS prologue while the D2 process handle is still
            // owned by SharedScannerState; otherwise Drop would run after the
            // handle closes and leave the E9 patch behind.
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Err(e) = shared_state.hovered_item_hook.uninstall() {
                log_error(&format!("Hovered-item hook uninstall failed: {}", e));
            }

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let _ = shared_state.dps_hook.uninstall();

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Ok(mut guard) = scanner_shared_state.write() {
                *guard = None;
            }

            // Signal the marker thread, then emit user-visible status before
            // joining — the join can block for one BFS tick.
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            shared_state.stop.store(true, Ordering::Relaxed);

            is_scanning.store(false, Ordering::SeqCst);
            game_status.store(GAME_STATUS_UNKNOWN, Ordering::SeqCst);
            if let Err(e) = app_handle.emit("scanner-status", "stopped") {
                log_error(&format!("Failed to emit event (stopped): {}", e));
            }
            if let Err(e) = app_handle.emit("game-status", "unknown") {
                log_error(&format!("Failed to emit event (unknown): {}", e));
            }
            if let Some(overlay) = app_handle.get_webview_window("overlay") {
                if let Err(e) = overlay.hide() {
                    log_error(&format!(
                        "Failed to hide overlay window when scanner stopped: {}",
                        e
                    ));
                }
            }

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            if let Some(h) = marker_handle {
                if h.join().is_err() {
                    log_error("marker-scanner thread panicked");
                }
            }
        })
        .expect("failed to spawn drop-scanner thread");
    *scanner_thread.lock().unwrap() = Some(handle);
}
