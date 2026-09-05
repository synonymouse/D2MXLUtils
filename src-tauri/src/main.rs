#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod breakpoints;
mod d2types;
mod damage_stats;
mod dps_hook;
mod dps_meter;
mod hook_bit_tracker;
mod hotkeys;
mod hovered_item;
mod injection;
mod items_cache;
mod keystroke_sim;
mod logger;
mod loot_filter_hook;
mod loot_history;
mod map_marker;
mod marker_scanner;
mod migrations;
mod mxl_item_api;
mod notifier;
mod offsets;
mod process;
mod profiles;
mod rules;
mod scanner_state;
mod settings;
mod sounds;
mod speedcalc_data;
mod stats_panel;
mod unique_stats_db;
mod unique_stats_db_sync;
mod updater;
mod weapon_families;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::hotkeys::{
    DpsMeterResetHotkeyState, EditModeState, HotkeyState, ItemSearchHotkeyState,
    LootHistoryHotkeyState, RevealHiddenState,
};
use crate::logger::{error as log_error, info as log_info};
use crate::loot_history::{LootEntry, LootHistory, PickupState};

use notifier::{DropScanner, ItemsDictionary};

const MARKER_SCAN_INTERVAL_MS: u64 = 100;

// Windows-only imports for process / overlay / privileges
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HANDLE, HWND, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
#[cfg(target_os = "windows")]
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevationType,
    TokenLinkedToken, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION_TYPE, TOKEN_LINKED_TOKEN, TOKEN_PRIVILEGES,
    TOKEN_QUERY,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongW, GetWindowRect, IsIconic, MoveWindow,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOW, SW_SHOWNA, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
    WS_THICKFRAME,
};

/// Shared state for controlling the scanner
struct AppState {
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
    /// Filter configuration shared with scanner thread
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    /// When true, scanner logs per-item filter decisions (noisy; opt-in for debugging).
    verbose_filter_logging: Arc<AtomicBool>,
    /// When true, scanner reports which rule line decided each drop so the
    /// Loot Filter tab can flash it live ("show matches" mode).
    live_match_highlight: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    auto_no_pickup: Arc<AtomicBool>,
    /// Driven by the reveal-hidden hotkey watcher; mirrored into the hook.
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    // Joined on shutdown so DropScanner::drop → loot_hook.eject runs before exit.
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    /// Session loot history shared with scanner thread.
    loot_history: Arc<RwLock<LootHistory>>,
    breakpoints_polling: Arc<AtomicBool>,
    stats_polling: Arc<AtomicBool>,
    speedcalc_table: Arc<RwLock<Option<speedcalc_data::SpeedcalcTable>>>,
    weapon_base_catalog: Arc<RwLock<Option<weapon_families::WeaponBaseCatalog>>>,
    dps_reset_pending: Arc<AtomicBool>,
    /// Lets `refresh_game_data_caches` signal a currently-attached scanner
    /// to rebuild its class/unique/set caches without an app restart.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    scanner_shared_state: Arc<RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>>,
}

const GAME_STATUS_UNKNOWN: u8 = 0;
const GAME_STATUS_INGAME: u8 = 1;
pub(crate) const GAME_STATUS_MENU: u8 = 2;

/// Check if Diablo II window exists
#[cfg(target_os = "windows")]
fn is_diablo2_running() -> bool {
    let class_wide: Vec<u16> = OsStr::new("Diablo II")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd = unsafe { FindWindowW(PCWSTR(class_wide.as_ptr()), PCWSTR::null()) };
    hwnd.is_ok() && !hwnd.unwrap().0.is_null()
}

/// X11 window lookup (works regardless of which Wine/Proton prefix the game
/// is running in — see `process.rs`'s Linux `D2Context`).
#[cfg(target_os = "linux")]
fn is_diablo2_running() -> bool {
    crate::process::open_process_by_window_class(crate::process::LINUX_WINDOW_TITLE).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn is_diablo2_running() -> bool {
    false
}

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
            let mut marker_scanner = crate::marker_scanner::MarkerScanner::new(state.clone());
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
fn start_scanner_internal(
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
    weapon_base_catalog: Arc<RwLock<Option<weapon_families::WeaponBaseCatalog>>>,
    dps_reset_pending: Arc<AtomicBool>,
    #[cfg(any(target_os = "windows", target_os = "linux"))] scanner_shared_state: Arc<
        RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
    >,
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
                    Ok(i) => i,
                    Err(e) => {
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
            // Full character-stats sheet is ~90 GetUnitStat calls per unit —
            // throttle to ~300 ms at 30 ms tick instead of every tick.
            let mut stats_tick_counter: u32 = 0;
            const STATS_CHECK_EVERY: u32 = 10;

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
                            if let Err(e) = items_cache::save_items_cache(&app_handle, &dict) {
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
                        match weapon_families::build_catalog(&shared_state.ctx, &injector) {
                            Ok(catalog) => {
                                drop(injector);
                                if let Err(e) =
                                    weapon_families::save_to_cache(&app_handle, &catalog)
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
                        let injector = shared_state.injector.lock().unwrap();
                        let player_result = breakpoints::read_unit_breakpoint_data(
                            &shared_state.ctx,
                            &injector,
                            offsets::d2client::PLAYER_UNIT,
                        );
                        let merc_result = breakpoints::read_unit_breakpoint_data(
                            &shared_state.ctx,
                            &injector,
                            offsets::d2client::MERCENARY_UNIT,
                        );
                        drop(injector);

                        let player_data = match player_result {
                            Ok(data) => {
                                last_player_bp = data.clone();
                                data
                            }
                            Err(()) => last_player_bp.clone(),
                        };
                        let merc_data = match merc_result {
                            Ok(data) => {
                                last_merc_bp = data.clone();
                                data
                            }
                            Err(()) => last_merc_bp.clone(),
                        };

                        #[derive(serde::Serialize)]
                        struct BreakpointsPayload {
                            player: Option<breakpoints::BreakpointData>,
                            merc: Option<breakpoints::BreakpointData>,
                        }
                        let payload = BreakpointsPayload {
                            player: player_data,
                            merc: merc_data,
                        };
                        if let Err(e) = app_handle.emit("breakpoints-update", &payload) {
                            log_error(&format!("Failed to emit breakpoints-update: {}", e));
                        }
                    }

                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    if stats_polling.load(Ordering::Relaxed) {
                        stats_tick_counter = stats_tick_counter.wrapping_add(1);
                        if stats_tick_counter % STATS_CHECK_EVERY == 0 {
                            let injector = shared_state.injector.lock().unwrap();

                            let player_damage = match damage_stats::read_unit_damage_stats(
                                &shared_state.ctx,
                                &injector,
                                offsets::d2client::PLAYER_UNIT,
                            ) {
                                Ok(data) => {
                                    last_player_damage = data.clone();
                                    data
                                }
                                Err(()) => last_player_damage.clone(),
                            };
                            let merc_damage = match damage_stats::read_unit_damage_stats(
                                &shared_state.ctx,
                                &injector,
                                offsets::d2client::MERCENARY_UNIT,
                            ) {
                                Ok(data) => {
                                    last_merc_damage = data.clone();
                                    data
                                }
                                Err(()) => last_merc_damage.clone(),
                            };

                            let player_stats = stats_panel::read_unit_character_stats(
                                &shared_state.ctx,
                                &injector,
                                offsets::d2client::PLAYER_UNIT,
                                last_player_stats.as_ref(),
                            );
                            last_player_stats = player_stats.clone();
                            let merc_stats = stats_panel::read_unit_character_stats(
                                &shared_state.ctx,
                                &injector,
                                offsets::d2client::MERCENARY_UNIT,
                                last_merc_stats.as_ref(),
                            );
                            last_merc_stats = merc_stats.clone();
                            drop(injector);

                            #[derive(serde::Serialize)]
                            struct UnitStatsPayload {
                                class: u32,
                                stats: std::collections::BTreeMap<u32, i32>,
                                #[serde(rename = "baseStats")]
                                base_stats: std::collections::BTreeMap<u32, i32>,
                                damage: Option<damage_stats::DamageStats>,
                            }
                            #[derive(serde::Serialize)]
                            struct StatsPayload {
                                player: Option<UnitStatsPayload>,
                                merc: Option<UnitStatsPayload>,
                            }
                            let payload = StatsPayload {
                                player: player_stats.map(|s| UnitStatsPayload {
                                    class: s.class,
                                    stats: s.stats,
                                    base_stats: s.base_stats,
                                    damage: player_damage,
                                }),
                                merc: merc_stats.map(|s| UnitStatsPayload {
                                    class: s.class,
                                    stats: s.stats,
                                    base_stats: s.base_stats,
                                    damage: merc_damage,
                                }),
                            };
                            if let Err(e) = app_handle.emit("stats-update", &payload) {
                                log_error(&format!("Failed to emit stats-update: {}", e));
                            }
                        }
                    }

                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    {
                        const AREA_CHECK_EVERY: u32 = 5;
                        let events = shared_state.dps_hook.drain();
                        let manual_reset = dps_reset_pending.swap(false, Ordering::SeqCst);

                        dps_area_tick_counter = dps_area_tick_counter.wrapping_add(1);
                        let area_token = if dps_area_tick_counter % AREA_CHECK_EVERY == 0 {
                            shared_state.read_current_area_token()
                        } else {
                            None
                        };
                        let area_change = match area_token {
                            Some(token) => {
                                // Sentinel -1 means first observation: record
                                // without resetting.
                                let prev = shared_state
                                    .last_area_token
                                    .swap(token as i64, Ordering::Relaxed);
                                prev >= 0 && prev != token as i64
                            }
                            None => false,
                        };

                        if let Ok(mut meter) = shared_state.dps_meter.write() {
                            if manual_reset || area_change {
                                if area_change {
                                    log_info(&format!(
                                        "DPS meter: area change → token 0x{:08X} (auto-reset)",
                                        area_token.unwrap_or(0)
                                    ));
                                }
                                meter.reset();
                            }
                            for ev in &events {
                                meter.ingest(
                                    ev.ts_ms,
                                    ev.delta_raw,
                                    ev.max_hp,
                                    ev.monster_level,
                                );
                            }
                            let snap = meter.snapshot(crate::dps_meter::now_ms());
                            drop(meter);
                            if let Err(e) = app_handle.emit("dps-update", &snap) {
                                log_error(&format!("Failed to emit dps-update: {}", e));
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_millis(30));
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

/// Spawn background thread that monitors for Diablo II and auto-starts scanner
fn spawn_auto_scanner(
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
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
    weapon_base_catalog: Arc<RwLock<Option<weapon_families::WeaponBaseCatalog>>>,
    dps_reset_pending: Arc<AtomicBool>,
    #[cfg(any(target_os = "windows", target_os = "linux"))] scanner_shared_state: Arc<
        RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
    >,
    app_handle: AppHandle,
) {
    thread::spawn(move || {
        while should_auto_scan.load(Ordering::SeqCst) {
            // If not currently scanning, check if D2 is running
            if !is_scanning.load(Ordering::SeqCst) && is_diablo2_running() {
                start_scanner_internal(
                    is_scanning.clone(),
                    filter_config.clone(),
                    verbose_filter_logging.clone(),
                    live_match_highlight.clone(),
                    auto_always_show_items.clone(),
                    auto_no_pickup.clone(),
                    reveal_hidden_active.clone(),
                    filter_config_generation.clone(),
                    scanner_thread.clone(),
                    game_status.clone(),
                    items_dictionary.clone(),
                    loot_history.clone(),
                    breakpoints_polling.clone(),
                    stats_polling.clone(),
                    weapon_base_catalog.clone(),
                    dps_reset_pending.clone(),
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    scanner_shared_state.clone(),
                    app_handle.clone(),
                );
            }

            // Check for the game launching frequently — this poll was the
            // single biggest fixed delay before the scanner even started
            // attaching (up to 2s of the reported 5-10s "time to ready").
            // `is_diablo2_running()` is cheap (a single X11 property read
            // over the shared connection on Linux, `FindWindowW` on
            // Windows), so polling this often is not a real cost.
            thread::sleep(Duration::from_millis(300));
        }
    });
}

#[tauri::command]
fn get_game_status(state: tauri::State<AppState>) -> &'static str {
    match state.game_status.load(Ordering::SeqCst) {
        GAME_STATUS_INGAME => "ingame",
        GAME_STATUS_MENU => "menu",
        _ => "unknown",
    }
}

#[tauri::command]
fn get_scanner_status(state: tauri::State<AppState>) -> bool {
    state.is_scanning.load(Ordering::SeqCst)
}

#[tauri::command]
fn get_items_dictionary(state: tauri::State<AppState>) -> ItemsDictionary {
    state
        .items_dictionary
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn get_loot_history(state: tauri::State<AppState>) -> Vec<LootEntry> {
    state
        .loot_history
        .read()
        .map(|h| h.snapshot())
        .unwrap_or_default()
}

#[tauri::command]
fn clear_loot_history(state: tauri::State<AppState>, app_handle: AppHandle) -> Result<(), String> {
    if let Ok(mut h) = state.loot_history.write() {
        h.clear();
    }
    app_handle
        .emit("loot-history-cleared", ())
        .map_err(|e| format!("Failed to emit loot-history-cleared: {}", e))
}

// ===== Filter Configuration Commands =====

/// Set the filter configuration for the scanner
#[tauri::command]
fn set_filter_config(
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

/// Opens the WebKit/WebView2 inspector for the calling window. Right-click's
/// native context menu is suppressed app-wide (see App.svelte) except inside
/// inputs/the rules editor, so this is the only way to reach devtools during
/// development. Debug-only: `devtools` is a real Cargo feature gate in
/// Tauri v2 (unlike v1, not automatic for debug builds), and `open_devtools`
/// only exists on the type when that feature is enabled.
#[tauri::command]
fn open_devtools(window: tauri::WebviewWindow) {
    #[cfg(debug_assertions)]
    window.open_devtools();
    #[cfg(not(debug_assertions))]
    let _ = window;
}

/// Enable or disable the per-item `[Filter] ...` log line.
#[tauri::command]
fn set_verbose_filter_logging(enabled: bool, state: tauri::State<AppState>) {
    state
        .verbose_filter_logging
        .store(enabled, Ordering::SeqCst);
}

/// Enable or disable the Loot Filter tab's live "show matches" highlight mode.
#[tauri::command]
fn set_live_match_highlight(enabled: bool, state: tauri::State<AppState>) {
    state.live_match_highlight.store(enabled, Ordering::SeqCst);
}

/// Enable or disable auto-toggling of MXL's "always show items" on game entry.
#[tauri::command]
fn set_auto_always_show_items(enabled: bool, state: tauri::State<AppState>) {
    state
        .auto_always_show_items
        .store(enabled, Ordering::SeqCst);
}

/// Enable or disable auto-enabling Diablo II's no-pickup flag on game entry.
#[tauri::command]
fn set_auto_no_pickup(enabled: bool, state: tauri::State<AppState>) {
    state.auto_no_pickup.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
fn set_breakpoints_polling(enabled: bool, state: tauri::State<AppState>) {
    state.breakpoints_polling.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
fn set_stats_polling(enabled: bool, state: tauri::State<AppState>) {
    state.stats_polling.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
fn reset_dps_session(state: tauri::State<AppState>) {
    state.dps_reset_pending.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn get_speedcalc_data(state: tauri::State<AppState>) -> Option<speedcalc_data::SpeedcalcTable> {
    state
        .speedcalc_table
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

#[tauri::command]
fn refresh_speedcalc_data(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let table = speedcalc_data::fetch_and_cache(&app_data_dir)?;
    if let Ok(mut guard) = state.speedcalc_table.write() {
        *guard = Some(table);
    }
    Ok(())
}

#[tauri::command]
fn get_weapon_base_catalog(
    state: tauri::State<AppState>,
) -> Option<weapon_families::WeaponBaseCatalog> {
    state
        .weapon_base_catalog
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Manual recovery from a stale `items.txt`/`UniqueItems.txt`/`SetItems.txt`
/// snapshot (e.g. after an MXL content patch changed item data without a
/// D2MXLUtils version bump — the on-disk caches are schema-versioned
/// against *our* version, not the game's, so they don't self-invalidate).
/// Clears the on-disk caches and, if a scanner is currently attached,
/// signals it to rebuild live — no app restart required either way: an
/// attached scanner rebuilds on its next tick, and a detached one rebuilds
/// on its next attach.
#[tauri::command]
fn refresh_game_data_caches(app: AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
    if let Ok(dir) = app.path().app_data_dir() {
        for file in [
            "matching-cache.json",
            "items-cache.json",
            "weapon-bases.json",
        ] {
            let path = dir.join(file);
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    log_error(&format!(
                        "refresh_game_data_caches: failed to remove {}: {}",
                        file, e
                    ));
                }
            }
        }
    }

    if let Ok(mut guard) = state.items_dictionary.write() {
        *guard = None;
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        if let Ok(mut guard) = state.weapon_base_catalog.write() {
            *guard = None;
        }
        if let Ok(guard) = state.scanner_shared_state.read() {
            if let Some(shared) = guard.as_ref() {
                shared.refresh_requested.store(true, Ordering::Relaxed);
                log_info(
                    "refresh_game_data_caches: signaled live scanner to rebuild from current game memory",
                );
                return Ok(());
            }
        }
    }

    log_info(
        "refresh_game_data_caches: no live attach — cleared on-disk/in-memory caches, next attach will rebuild",
    );
    Ok(())
}

// ===== DSL Parser Commands =====

/// Parse DSL text into FilterConfig JSON
#[tauri::command]
fn parse_filter_dsl(text: String) -> Result<rules::FilterConfig, Vec<rules::ParseError>> {
    rules::parse_dsl(&text)
}

/// Validate DSL text and return errors/warnings
#[tauri::command]
fn validate_filter_dsl(text: String) -> Vec<rules::ValidationError> {
    rules::validate_dsl(&text)
}

/// Plain-English explanation for a single rule line, used by the
/// editor's hover tooltip. Returns `None` for blank lines, comments,
/// group close `}`, and unparseable input.
#[tauri::command]
fn explain_filter_line(line: String) -> Option<String> {
    rules::explain_line(&line)
}

/// Resolve the filter decision for a hypothetical item. Used by the UI
/// to preview what the current filter would do without actually dropping
/// anything in-game. See `docs/filter-preview-todo.md` for the planned UI
/// scenarios built around this command.
#[tauri::command]
fn get_item_filter_action(
    mut config: rules::FilterConfig,
    item: notifier::ItemDropEvent,
) -> rules::FilterDecision {
    use crate::rules::MatchContext;
    config.prepare_for_matching();
    let ctx = MatchContext::new(&item);
    config.decide(&ctx)
}

#[tauri::command]
fn set_overlay_interactive(
    app: AppHandle,
    active: bool,
    keyboard_active: bool,
) -> Result<(), String> {
    OVERLAY_CLICK_THROUGH.store(!active, Ordering::SeqCst);
    OVERLAY_KEYBOARD_INTERACTIVE.store(keyboard_active, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let _ = sync_overlay_with_game_impl(&app);
        if overlay_should_force_foreground(active, keyboard_active) {
            force_overlay_foreground(&app);
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Overlay is shown non-focusable/click-through by default (see
        // `sync_overlay_with_game_impl_linux`) to avoid a focus-flicker
        // loop against the game and to not block clicks into it. Any
        // interactive panel (item search, edit-mode drag, loot history)
        // needs mouse clicks to actually land on it, and unlike Windows'
        // WS_EX_NOACTIVATE (which guarantees click delivery to a
        // non-activatable window), there's no such guarantee under an
        // arbitrary X11 WM — so `active` (not just `keyboard_active`)
        // toggles focusable here, not only the item-search typing case.
        // `overlay_should_be_visible` (used by the sync call below) keeps
        // the overlay from being hidden out from under the user once it
        // holds focus itself. Call the sync directly (not just wait for
        // the next 250ms poll) so the click-through/focus change applies
        // immediately, same as the Windows branch above.
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.set_focusable(active);
        }
        let _ = sync_overlay_with_game_impl_linux(&app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = &app;
    }
    // Windows only force-focuses for the keyboard case (WS_EX_NOACTIVATE
    // already guarantees mouse-only interaction like edit-mode dragging
    // works without taking foreground — forcing it there would just
    // steal keyboard focus from the game for no reason). Linux has no
    // such guarantee: some WMs swallow a window's *first* click after a
    // focus change (using it only to activate/raise the window, not
    // deliver it to the app), which showed up as "the first drag click
    // after the game regains focus does nothing, second click works" —
    // so proactively focus the overlay the moment any panel goes
    // interactive, before the user's own click would have had to do it.
    #[cfg(target_os = "linux")]
    let should_focus = active;
    #[cfg(not(target_os = "linux"))]
    let should_focus = active && keyboard_active;
    if should_focus {
        if let Some(overlay) = app.get_webview_window("overlay") {
            if let Err(e) = overlay.set_focus() {
                log_error(&format!("Failed to focus overlay window: {}", e));
            }
        }
    }
    // Closing a panel (active -> false) leaves keyboard focus wherever the
    // overlay panel put it. On Windows, WS_EX_NOACTIVATE means mouse-only
    // panels never took focus in the first place and the keyboard case is
    // handled by `force_overlay_foreground`'s counterpart elsewhere; on
    // Linux there's no such guarantee, and the overlay hiding itself
    // (`sync_overlay_with_game_impl_linux`) only *implicitly* returns focus
    // to D2 via the WM's own unmap-focus behavior — not guaranteed under
    // every focus policy, and was the actual root cause of focus visibly
    // swapping between the overlay and the game after closing a panel. So
    // explicitly hand focus back to D2 here instead of hoping the WM does it.
    #[cfg(target_os = "linux")]
    if !should_focus && is_diablo2_running() {
        if let Err(e) = crate::process::linux_activate_window_by_title_confirmed(
            crate::process::LINUX_WINDOW_TITLE,
        ) {
            log_error(&format!("Failed to refocus D2 window: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
fn set_overlay_edit_mode(app: AppHandle, active: bool) -> Result<(), String> {
    OVERLAY_EDIT_ACTIVE.store(active, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let _ = sync_overlay_with_game_impl(&app);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = sync_overlay_with_game_impl_linux(&app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
    }
    Ok(())
}

#[tauri::command]
fn sync_overlay_with_game(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        sync_overlay_with_game_impl(&app)
    }

    #[cfg(target_os = "linux")]
    {
        sync_overlay_with_game_impl_linux(&app)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
        Err("Overlay sync is not supported on this OS".to_string())
    }
}

/// Linux overlay, matching the Windows design: the overlay window is
/// resized/repositioned to exactly cover the game window every tick (not
/// just once on first show — the game can move/resize), and made
/// click-through via the X11 SHAPE extension's input region unless a
/// panel (item search, edit mode, loot history) is actually open. This
/// is the Linux analog of Windows' live `WS_EX_LAYERED`/`WS_EX_TRANSPARENT`
/// toggling (see `docs/overlay-reposition-hittest-bug.md` for the
/// Windows-side version of this problem). Percentage-based widget
/// positions (`ItemSearchOverlay.svelte` etc.) assume the overlay spans
/// the full game window, same as on Windows — a smaller fixed-size corner
/// toast (the original v1 approach here) clips them.
#[cfg(target_os = "linux")]
fn sync_overlay_with_game_impl_linux(app: &AppHandle) -> Result<(), String> {
    const MARGIN: i32 = 16;
    const FALLBACK_WIDTH: u32 = 1024;
    const FALLBACK_HEIGHT: u32 = 768;

    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "overlay window not found".to_string())?;

    // Only draw while D2 is both running *and* the focused window — no
    // point covering the screen with a game overlay while the user has
    // switched to something else. Exception: while the overlay itself is
    // in an interactive panel (item search, edit-mode drag, loot history),
    // it may hold real X11 focus (typing) or just get raised/activated by
    // the WM as a side effect of a plain click on it (edit-mode dragging
    // never explicitly requests focus, but some WMs activate a window on
    // click regardless) — either way that makes this tick look identical
    // to "the user alt-tabbed away", hiding the overlay out from under a
    // mid-drag/mid-search user. `panel_active` (not just the narrower
    // `keyboard_interactive`) is what actually protects against that;
    // `overlay_should_be_visible` (shared with the Windows path) applies
    // it via `foreground_matches_overlay`.
    let running = is_diablo2_running();
    let d2_focused = running
        && crate::process::linux_is_window_focused_by_title(crate::process::LINUX_WINDOW_TITLE)
            .unwrap_or(false);
    let own_focused = crate::process::linux_is_own_window_focused().unwrap_or(false);
    let panel_active = !OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
    let focused = overlay_should_be_visible(d2_focused, own_focused, !running, panel_active);
    let was_visible = OVERLAY_WAS_VISIBLE.swap(focused, Ordering::SeqCst);

    if !focused {
        if was_visible {
            overlay
                .hide()
                .map_err(|e| format!("Failed to hide overlay: {}", e))?;
            if let Ok(mut last) = OVERLAY_LAST_RECT_LINUX.lock() {
                *last = None;
            }
            // Force a fresh click-through/input-shape application on the
            // next show — the "last applied" cache surviving a hide/show
            // cycle caused a real bug: if edit mode was already captured
            // before a transient focus-loss hide, the desired state looks
            // unchanged after reshowing, so the code skipped reapplying
            // the X11 SHAPE input region even though it may not have
            // survived the hide/show cycle intact, leaving clicks falling
            // through to the game until the user released and re-pressed
            // the edit-mode chord (which forces reapplication via a
            // logical state change).
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(-1, Ordering::SeqCst);
        }
        return Ok(());
    }

    // Anchor/size to the actual game window, not just its monitor — the
    // game is often windowed and doesn't fill the monitor, so a
    // monitor-corner anchor can land well away from the game on an
    // unusual multi-monitor layout. Requires XWayland (forced in
    // `main()`): native Wayland gives clients no control over top-level
    // window position/size at all.
    let game_rect =
        crate::process::linux_find_window_rect_by_title(crate::process::LINUX_WINDOW_TITLE).ok();
    let (x, y, width, height) = game_rect.unwrap_or_else(|| {
        let fallback_pos = overlay
            .primary_monitor()
            .ok()
            .flatten()
            .or_else(|| overlay.current_monitor().ok().flatten())
            .or_else(|| {
                overlay.available_monitors().ok().and_then(|mut m| {
                    if m.is_empty() {
                        None
                    } else {
                        Some(m.remove(0))
                    }
                })
            })
            .map(|m| *m.position())
            .unwrap_or(tauri::PhysicalPosition { x: 0, y: 0 });
        (
            fallback_pos.x + MARGIN,
            fallback_pos.y + MARGIN,
            FALLBACK_WIDTH,
            FALLBACK_HEIGHT,
        )
    });

    if !was_visible {
        // Without this, showing the overlay hands it keyboard focus (most
        // WMs auto-focus newly-mapped windows), which our own focus check
        // above then reads as "D2 lost focus" on the very next 250ms tick
        // — hide, which returns focus to D2 — which we read as "D2 focused
        // again" — show — repeat, flickering forever. `focusable(false)` is
        // a runtime-only call (this Linux code path is the only caller),
        // so it doesn't touch the shared `tauri.conf.json` window config
        // Windows' own dynamic WS_EX_NOACTIVATE toggling still relies on.
        let _ = overlay.set_focusable(panel_active);
    }

    let needs_move = OVERLAY_LAST_RECT_LINUX
        .lock()
        .ok()
        .map(|guard| *guard != Some((x, y, width, height)))
        .unwrap_or(true);
    if needs_move {
        let _ = overlay.set_size(tauri::Size::Physical(tauri::PhysicalSize { width, height }));
        let _ = overlay.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        if let Ok(mut last) = OVERLAY_LAST_RECT_LINUX.lock() {
            *last = Some((x, y, width, height));
        }
    }

    if !was_visible {
        overlay
            .show()
            .map_err(|e| format!("Failed to show overlay: {}", e))?;
        if panel_active {
            // Reshowing mid-interactive-session (e.g. the game regained
            // focus while edit mode was still logically active) needs the
            // same proactive focus `set_overlay_interactive` does on the
            // initial transition into an interactive panel — otherwise
            // the WM treats the user's next click as "just activate the
            // window" and swallows it instead of delivering it as a drag
            // start.
            if let Err(e) = overlay.set_focus() {
                log_error(&format!("Failed to focus overlay window on reshow: {}", e));
            }
        } else if running {
            // `set_focusable(false)` above is meant to stop the WM from
            // handing the newly-mapped overlay focus in the first place,
            // but that's just an advisory ICCCM hint — confirmed live
            // (KWin) to not be honored reliably at map time, which
            // reproduces exactly the flicker loop described above: show
            // steals focus, next tick reads "D2 lost focus", hide,
            // "D2 focused again", show, repeat — happening right at
            // launch, before any panel is ever touched. Deterministically
            // reassert D2 as focused immediately after showing, the same
            // way `set_overlay_interactive` already does when a panel
            // closes, instead of trusting the hint alone.
            if let Err(e) = crate::process::linux_activate_window_by_title_confirmed(
                crate::process::LINUX_WINDOW_TITLE,
            ) {
                log_error(&format!(
                    "Failed to refocus D2 window after showing overlay: {}",
                    e
                ));
            }
        }
    }

    // Click-through unless a panel is actually open (edit mode / item
    // search / loot history) — `panel_active` (computed above from
    // `OVERLAY_CLICK_THROUGH`, updated by `set_overlay_interactive`) is
    // the same signal Windows' WS_EX_TRANSPARENT toggle uses.
    let desired_click_through = !panel_active;
    let desired_i8: i8 = if desired_click_through { 1 } else { 0 };
    if OVERLAY_LAST_CLICK_THROUGH_APPLIED.swap(desired_i8, Ordering::SeqCst) != desired_i8 {
        set_overlay_click_through_linux(&overlay, desired_click_through, width, height);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
static OVERLAY_LAST_RECT_LINUX: Mutex<Option<(i32, i32, u32, u32)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn overlay_xid(window: &tauri::WebviewWindow) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        _ => None,
    }
}

/// Toggle whether the overlay window intercepts mouse input, via the X11
/// SHAPE extension's input region — the Linux analog of Windows'
/// WS_EX_TRANSPARENT toggling. An empty input region makes the whole
/// window click-through (events fall through to whatever's behind it,
/// i.e. the game); a single rect covering the window makes it capture
/// input normally, needed while an interactive panel is open.
#[cfg(target_os = "linux")]
fn set_overlay_click_through_linux(
    window: &tauri::WebviewWindow,
    click_through: bool,
    width: u32,
    height: u32,
) {
    use x11rb::protocol::shape::{self, SK, SO};
    use x11rb::protocol::xproto::{ClipOrdering, Rectangle};

    let Some(xid) = overlay_xid(window) else {
        return;
    };
    let Ok((conn, _)) = crate::process::linux_x11_conn() else {
        return;
    };

    let rects: Vec<Rectangle> = if click_through {
        Vec::new()
    } else {
        vec![Rectangle {
            x: 0,
            y: 0,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
        }]
    };

    let cookie = match shape::rectangles(
        conn,
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        xid,
        0,
        0,
        &rects,
    ) {
        Ok(cookie) => cookie,
        Err(e) => {
            log_error(&format!(
                "overlay click-through: shape::rectangles request failed: {}",
                e
            ));
            return;
        }
    };
    if let Err(e) = cookie.check() {
        log_error(&format!(
            "overlay click-through: shape::rectangles failed: {}",
            e
        ));
    }
}

static OVERLAY_WAS_VISIBLE: AtomicBool = AtomicBool::new(false);
static OVERLAY_CLICK_THROUGH: AtomicBool = AtomicBool::new(true);
static OVERLAY_KEYBOARD_INTERACTIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY_STYLES_APPLIED: AtomicBool = AtomicBool::new(false);
static OVERLAY_EDIT_ACTIVE: AtomicBool = AtomicBool::new(false);

// -1 sentinel = never applied; forces first sync to push the style.
static OVERLAY_LAST_CLICK_THROUGH_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);
static OVERLAY_LAST_EDIT_MODE_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);
static OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayWindowKind {
    Visual,
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OverlayWindowSpec {
    label: &'static str,
    title: &'static str,
    layered: bool,
    click_through: bool,
}

fn overlay_window_spec(kind: OverlayWindowKind) -> OverlayWindowSpec {
    match kind {
        OverlayWindowKind::Visual => OverlayWindowSpec {
            label: "overlay",
            title: "D2MXLUtils Overlay",
            layered: true,
            click_through: true,
        },
        OverlayWindowKind::Edit => OverlayWindowSpec {
            label: "overlay",
            title: "D2MXLUtils Overlay",
            layered: false,
            click_through: false,
        },
    }
}

fn overlay_should_be_visible(
    foreground_matches_game: bool,
    foreground_matches_overlay: bool,
    game_minimized: bool,
    keyboard_interactive: bool,
) -> bool {
    !game_minimized
        && (foreground_matches_game || (keyboard_interactive && foreground_matches_overlay))
}

fn overlay_should_use_noactivate(keyboard_interactive: bool) -> bool {
    !keyboard_interactive
}

fn overlay_should_force_foreground(active: bool, keyboard_active: bool) -> bool {
    active && keyboard_active
}

fn overlay_window_kind_for_state(edit_active: bool, click_through: bool) -> OverlayWindowKind {
    if edit_active || !click_through {
        OverlayWindowKind::Edit
    } else {
        OverlayWindowKind::Visual
    }
}

fn overlay_style_needs_update(
    just_applied: bool,
    last_click_through: i8,
    desired_click_through: i8,
    last_edit_mode: i8,
    desired_edit_mode: i8,
    last_keyboard_interactive: i8,
    desired_keyboard_interactive: i8,
) -> bool {
    just_applied
        || last_click_through != desired_click_through
        || last_edit_mode != desired_edit_mode
        || last_keyboard_interactive != desired_keyboard_interactive
}

fn overlay_should_strip_chrome(_style_changed: bool, chrome_present: bool) -> bool {
    chrome_present
}

#[cfg(target_os = "windows")]
fn overlay_chrome_mask() -> i32 {
    (WS_CAPTION.0
        | WS_BORDER.0
        | WS_DLGFRAME.0
        | WS_THICKFRAME.0
        | WS_SYSMENU.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0) as i32
}

#[cfg(target_os = "windows")]
static OVERLAY_LAST_RECT: Mutex<Option<RECT>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn force_overlay_foreground(app: &AppHandle) {
    let spec = overlay_window_spec(OverlayWindowKind::Visual);
    let Some(overlay) = app.get_webview_window(spec.label) else {
        return;
    };

    let hwnd_overlay = match overlay.hwnd() {
        Ok(hwnd) => hwnd,
        Err(e) => {
            log_error(&format!(
                "Failed to get overlay HWND for foreground activation: {}",
                e
            ));
            return;
        }
    };
    let hwnd_overlay = HWND(hwnd_overlay.0 as _);

    if hwnd_overlay.0.is_null() {
        return;
    }

    unsafe {
        if !SetForegroundWindow(hwnd_overlay).as_bool() {
            log_error("Failed to activate overlay foreground for item search");
        }
    }
}

#[cfg(target_os = "windows")]
fn reset_overlay_runtime_state() {
    OVERLAY_WAS_VISIBLE.store(false, Ordering::SeqCst);
    OVERLAY_STYLES_APPLIED.store(false, Ordering::SeqCst);
    OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(-1, Ordering::SeqCst);
    OVERLAY_LAST_EDIT_MODE_APPLIED.store(-1, Ordering::SeqCst);
    OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED.store(-1, Ordering::SeqCst);
    if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
        *last = None;
    }
}

#[cfg(test)]
mod overlay_window_tests {
    use super::*;

    #[test]
    fn visual_and_edit_modes_reuse_one_overlay_window_with_different_styles() {
        let visual = overlay_window_spec(OverlayWindowKind::Visual);
        let edit = overlay_window_spec(OverlayWindowKind::Edit);

        assert_eq!(visual.label, "overlay");
        assert_eq!(visual.title, "D2MXLUtils Overlay");
        assert!(visual.layered);
        assert!(visual.click_through);

        assert_eq!(edit.label, "overlay");
        assert_eq!(edit.title, "D2MXLUtils Overlay");
        assert!(!edit.layered);
        assert!(!edit.click_through);
    }

    #[test]
    fn overlay_is_hidden_when_game_is_minimized_even_if_foreground_still_matches() {
        assert!(overlay_should_be_visible(true, false, false, false));
        assert!(!overlay_should_be_visible(false, false, false, false));
        assert!(!overlay_should_be_visible(true, false, true, false));
    }

    #[test]
    fn overlay_stays_visible_when_keyboard_panel_has_focus() {
        assert!(overlay_should_be_visible(false, true, false, true));
        assert!(!overlay_should_be_visible(false, true, false, false));
        assert!(!overlay_should_be_visible(false, true, true, true));
    }

    #[test]
    fn keyboard_interactive_overlay_can_activate() {
        assert!(overlay_should_use_noactivate(false));
        assert!(!overlay_should_use_noactivate(true));
    }

    #[test]
    fn keyboard_interactive_overlay_forces_foreground_on_open() {
        assert!(overlay_should_force_foreground(true, true));
        assert!(!overlay_should_force_foreground(true, false));
        assert!(!overlay_should_force_foreground(false, true));
    }

    #[test]
    fn keyboard_interactive_change_reapplies_overlay_style() {
        assert!(overlay_style_needs_update(false, 0, 0, 0, 0, 0, 1));
        assert!(!overlay_style_needs_update(false, 0, 0, 0, 0, 1, 1));
    }

    #[test]
    fn interactive_overlay_uses_non_layered_mode() {
        assert_eq!(
            overlay_window_kind_for_state(false, false),
            OverlayWindowKind::Edit
        );
    }

    #[test]
    fn overlay_chrome_is_stripped_even_without_style_transition() {
        assert!(overlay_should_strip_chrome(true, true));
        assert!(overlay_should_strip_chrome(false, true));
        assert!(!overlay_should_strip_chrome(false, false));
    }
}

#[cfg(target_os = "windows")]
fn sync_overlay_with_game_impl(app: &AppHandle) -> Result<(), String> {
    let visual_spec = overlay_window_spec(OverlayWindowKind::Visual);
    let class_wide: Vec<u16> = OsStr::new("Diablo II")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_game =
        unsafe { FindWindowW(PCWSTR(class_wide.as_ptr()), PCWSTR::null()) }.map_err(|_| {
            "Diablo II window not found (class 'Diablo II'). Is the game running?".to_string()
        })?;

    if hwnd_game.0.is_null() {
        return Err("Diablo II window handle is null".to_string());
    }

    let overlay_window = app.get_webview_window(visual_spec.label).ok_or(format!(
        "Overlay window with label '{}' not found",
        visual_spec.label
    ))?;

    let title_wide: Vec<u16> = OsStr::new(visual_spec.title)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_overlay = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr())) }
        .map_err(|_| format!("Overlay OS window '{}' not found", visual_spec.title))?;

    if hwnd_overlay.0.is_null() {
        return Err("Overlay HWND is null".to_string());
    }

    unsafe {
        let fg = GetForegroundWindow();
        let game_minimized = IsIconic(hwnd_game).as_bool();
        let keyboard_interactive = OVERLAY_KEYBOARD_INTERACTIVE.load(Ordering::SeqCst);
        if !overlay_should_be_visible(
            fg.0 == hwnd_game.0,
            fg.0 == hwnd_overlay.0,
            game_minimized,
            keyboard_interactive,
        ) {
            let _ = ShowWindow(hwnd_overlay, SW_HIDE);
            let _ = overlay_window.hide();
            reset_overlay_runtime_state();
            return Ok(());
        }
    }

    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd_game, &mut rect).map_err(|e| format!("GetWindowRect failed: {}", e))?;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    let was_visible = OVERLAY_WAS_VISIBLE.swap(true, Ordering::SeqCst);

    unsafe {
        // WS_EX_NOACTIVATE prevents the overlay from ever stealing foreground
        // from the game — without it, alt-tabbing back triggers a focus war
        // that flickers the screen edges and steals mouse input.
        let just_applied = !OVERLAY_STYLES_APPLIED.swap(true, Ordering::SeqCst);
        let edit_active = OVERLAY_EDIT_ACTIVE.load(Ordering::SeqCst);
        let keyboard_interactive = OVERLAY_KEYBOARD_INTERACTIVE.load(Ordering::SeqCst);
        let click_through = OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
        let mode_spec =
            overlay_window_spec(overlay_window_kind_for_state(edit_active, click_through));
        let desired_ct = mode_spec.click_through && click_through;
        let desired_ct_i8 = if desired_ct { 1 } else { 0 };
        let edit_active_i8 = if edit_active { 1 } else { 0 };
        let keyboard_interactive_i8 = if keyboard_interactive { 1 } else { 0 };
        let needs_style = overlay_style_needs_update(
            just_applied,
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.load(Ordering::SeqCst),
            desired_ct_i8,
            OVERLAY_LAST_EDIT_MODE_APPLIED.load(Ordering::SeqCst),
            edit_active_i8,
            OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED.load(Ordering::SeqCst),
            keyboard_interactive_i8,
        );
        if needs_style {
            let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
            let mut new_ex = ex_style | WS_EX_TOOLWINDOW.0 as i32;
            if overlay_should_use_noactivate(keyboard_interactive) {
                new_ex |= WS_EX_NOACTIVATE.0 as i32;
            } else {
                new_ex &= !(WS_EX_NOACTIVATE.0 as i32);
            }
            if mode_spec.layered {
                new_ex |= WS_EX_LAYERED.0 as i32;
            } else {
                new_ex &= !(WS_EX_LAYERED.0 as i32);
            }
            if desired_ct {
                new_ex |= WS_EX_TRANSPARENT.0 as i32;
            } else {
                new_ex &= !(WS_EX_TRANSPARENT.0 as i32);
            }
            SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex);
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(desired_ct_i8, Ordering::SeqCst);
            OVERLAY_LAST_EDIT_MODE_APPLIED.store(edit_active_i8, Ordering::SeqCst);
            OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED
                .store(keyboard_interactive_i8, Ordering::SeqCst);

            // Suppress the 1px Win11 DWM accent frame; ignored on Win10.
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd_overlay,
                DWMWA_BORDER_COLOR,
                &DWMWA_COLOR_NONE as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // Tauri/Windows can reintroduce caption bits when transparency styles
        // change. Re-strip them every sync so a title bar cannot survive until
        // the next edit-mode transition.
        let style = GetWindowLongW(hwnd_overlay, GWL_STYLE);
        let chrome_mask = overlay_chrome_mask();
        if overlay_should_strip_chrome(needs_style, (style & chrome_mask) != 0) {
            let new_style = (style & !chrome_mask) | WS_POPUP.0 as i32;
            SetWindowLongW(hwnd_overlay, GWL_STYLE, new_style);
            let _ = SetWindowPos(
                hwnd_overlay,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }

        // WebView2 only commits transparency on a resize, so on the first show
        // we resize by 1px and back. SW_SHOWNA (not Tauri's show(), which uses
        // SW_SHOW) — SW_SHOW would activate and steal focus from the game.
        if !was_visible {
            let _ = MoveWindow(
                hwnd_overlay,
                rect.left,
                rect.top,
                width + 1,
                height + 1,
                BOOL(1),
            );
            let show_cmd = if keyboard_interactive {
                SW_SHOW
            } else {
                SW_SHOWNA
            };
            let _ = ShowWindow(hwnd_overlay, show_cmd);
            let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
            if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
                *last = Some(rect);
            }
        } else {
            let needs_move = OVERLAY_LAST_RECT
                .lock()
                .ok()
                .map(|guard| match *guard {
                    Some(prev) => {
                        prev.left != rect.left
                            || prev.top != rect.top
                            || prev.right != rect.right
                            || prev.bottom != rect.bottom
                    }
                    None => true,
                })
                .unwrap_or(true);
            if needs_move {
                let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
                if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
                    *last = Some(rect);
                }
            }
        }

        // No SWP_SHOWWINDOW: that flag forces a frame repaint each tick, which
        // re-flashed the Win11 DWM border and was a major source of the
        // edge-flicker.
        let _ = SetWindowPos(
            hwnd_overlay,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }

    Ok(())
}

/// Enable SeDebugPrivilege on the current process token, if possible.
///
/// This matches what many memory tools (including the original AutoIt-based D2Stats)
/// do before calling OpenProcess on game processes. Without this privilege, some
/// Windows configurations may return ACCESS_DENIED even for the same user.
#[cfg(target_os = "windows")]
fn enable_debug_privilege() {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, LUID};

    unsafe {
        let mut token_handle = HANDLE::default();
        // We need both QUERY and ADJUST_PRIVILEGES to toggle SeDebugPrivilege.
        let desired_access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
        if let Err(e) = OpenProcessToken(GetCurrentProcess(), desired_access, &mut token_handle) {
            log_error(&format!("SeDebugPrivilege: OpenProcessToken failed: {}", e));
            return;
        }

        // Resolve the LUID for SeDebugPrivilege.
        let mut luid = LUID::default();
        if let Err(e) = LookupPrivilegeValueW(None, SE_DEBUG_NAME, &mut luid) {
            log_error(&format!(
                "SeDebugPrivilege: LookupPrivilegeValueW failed: {}",
                e
            ));
            let _ = CloseHandle(token_handle);
            return;
        }

        let mut tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        // Enable SeDebugPrivilege on this token.
        let result = AdjustTokenPrivileges(
            token_handle,
            BOOL(0),
            Some(&tp as *const TOKEN_PRIVILEGES),
            size_of::<TOKEN_PRIVILEGES>() as u32,
            None,
            None,
        );

        let _ = CloseHandle(token_handle);

        if let Err(e) = result {
            log_error(&format!(
                "SeDebugPrivilege: AdjustTokenPrivileges failed: {}",
                e
            ));
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn enable_debug_privilege() {
    // No-op on non-Windows platforms.
}

/// Configure WebView2 user data folder for elevated processes.
///
/// When running with administrator privileges (elevated), WebView2 may fail
/// to access the user's LocalAppData because the elevated process runs under
/// a different user context. This function detects elevation and sets
/// WEBVIEW2_USER_DATA_FOLDER to the non-elevated user's LocalAppData path.
#[cfg(target_os = "windows")]
fn setup_webview2_for_elevation() {
    use std::mem::size_of;

    unsafe {
        // Get the current process token
        let mut token_handle = HANDLE::default();
        if let Err(e) = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) {
            log_error(&format!(
                "WebView2 setup: OpenProcessToken failed, skipping elevation check: {}",
                e
            ));
            return;
        }

        // Check elevation type
        let mut elevation_type = TOKEN_ELEVATION_TYPE::default();
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token_handle,
            TokenElevationType,
            Some(&mut elevation_type as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut return_length,
        );

        if let Err(e) = result {
            log_error(&format!(
                "WebView2 setup: GetTokenInformation(TokenElevationType) failed: {}",
                e
            ));
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            return;
        }

        // TokenElevationTypeFull (2) means the process is elevated via UAC
        // We need to get the linked token (non-elevated user token) to find correct AppData
        if elevation_type.0 != 2 {
            // Not elevated via UAC, no need to adjust WebView2 path
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            return;
        }

        // Get the linked token (the non-elevated user token)
        let mut linked_token = TOKEN_LINKED_TOKEN::default();
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token_handle,
            TokenLinkedToken,
            Some(&mut linked_token as *mut _ as *mut _),
            size_of::<TOKEN_LINKED_TOKEN>() as u32,
            &mut return_length,
        );

        let _ = windows::Win32::Foundation::CloseHandle(token_handle);

        if let Err(e) = result {
            log_error(&format!(
                "WebView2 setup: GetTokenInformation(TokenLinkedToken) failed: {}",
                e
            ));
            return;
        }

        // Get LocalAppData path using the linked (non-elevated) token
        let path_ptr = SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DEFAULT,
            linked_token.LinkedToken,
        );

        let _ = windows::Win32::Foundation::CloseHandle(linked_token.LinkedToken);

        match path_ptr {
            Ok(ptr) => {
                // Convert PWSTR to Rust String
                let path_str = ptr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(ptr.as_ptr() as *const _));

                if !path_str.is_empty() {
                    // Construct WebView2 data folder path
                    let webview2_path = format!("{}\\D2MXLUtils\\WebView2", path_str);
                    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview2_path);
                }
            }
            Err(e) => {
                log_error(&format!(
                    "WebView2 setup: SHGetKnownFolderPath(LocalAppData) failed: {:?}",
                    e
                ));
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn setup_webview2_for_elevation() {
    // No-op on non-Windows platforms
}

/// Pre-populate the scanner's filter config from the last-used profile on
/// startup
fn load_initial_filter_config(app: &AppHandle) -> Option<rules::FilterConfig> {
    let settings = settings::load_settings(app.clone()).ok()?;
    let name = settings.active_profile.filter(|s| !s.is_empty())?;
    let text = match profiles::load_profile(app.clone(), name.clone()) {
        Ok(t) => t,
        Err(e) => {
            log_error(&format!(
                "Startup: failed to read active profile '{}': {}",
                name, e
            ));
            return None;
        }
    };
    match rules::parse_dsl(&text) {
        Ok(cfg) => {
            log_info(&format!(
                "Startup: loaded filter config from active profile '{}' ({} rules)",
                name,
                cfg.rules.len()
            ));
            Some(cfg)
        }
        Err(errors) => {
            log_error(&format!(
                "Startup: failed to parse active profile '{}': {}",
                name,
                errors
                    .iter()
                    .map(|e| e.message.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            None
        }
    }
}

#[tauri::command]
fn open_app_folder(app: AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {}", e))?;
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let opener = "open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("Failed to open file manager: {}", e))?;
    Ok(())
}

/// Open an http(s) URL in the user's default browser.
/// Scheme validation prevents `start` from being coaxed into launching a
/// local file or custom handler via attacker-controlled URLs.
#[tauri::command]
fn get_changelog() -> &'static str {
    include_str!("../../CHANGELOG.md")
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http(s) URLs are allowed".into());
    }
    #[cfg(target_os = "windows")]
    {
        // `cmd /c start "" <url>` — the empty "" arg is the window title slot
        // that `start` consumes before the target, so the URL is parsed as
        // the target.
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| format!("Failed to open url: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open url: {}", e))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open url: {}", e))?;
    }
    Ok(())
}

fn main() {
    // Force XWayland instead of native Wayland for GTK/webkit2gtk. Native
    // Wayland gives client apps no control over top-level window position
    // at all (no X11-RANDR-style settable (x, y) — only the compositor
    // decides), which breaks placing the overlay in a screen corner and,
    // on unusual multi-monitor layouts, can land the default position on a
    // monitor the user isn't even looking at. XWayland restores normal
    // X11 positioning semantics; must be set before GTK initializes.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }

    // NVIDIA's proprietary driver has long had incomplete/buggy DMA-BUF
    // export support, which is what WebKitGTK's hardware compositing path
    // relies on. On affected setups (confirmed: NVIDIA + Wayland session,
    // even with GDK_BACKEND forced to x11 above) this doesn't crash or log
    // anything from WebKit — the window just paints its background and
    // never draws page content, i.e. a silent white screen. This is a
    // lightweight overlay UI, not a GPU-heavy page, so there's no real
    // cost to disabling the DMA-BUF renderer unconditionally.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    // Enable SeDebugPrivilege so OpenProcess has the same behavior as legacy tools.
    enable_debug_privilege();

    // Configure WebView2 data folder for elevated processes BEFORE Tauri init
    setup_webview2_for_elevation();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let cached_items = items_cache::load_items_cache(app.handle());
            let cached_weapon_bases = weapon_families::load_from_cache(app.handle());

            // First-run: if the settings file has never been written, drop a
            // ready-to-use Default profile and mark it active
            if let Ok(dir) = app.handle().path().app_data_dir() {
                let settings_path = dir.join("settings.json");
                if !settings_path.exists() {
                    match profiles::seed_default_profile(app.handle()) {
                        Ok(name) => {
                            let mut s =
                                settings::load_settings(app.handle().clone()).unwrap_or_default();
                            s.active_profile = Some(name);
                            if let Err(e) = settings::save_settings(app.handle().clone(), s) {
                                log_error(&format!(
                                    "First-run seed: failed to persist active profile: {}",
                                    e
                                ));
                            }
                        }
                        Err(e) => log_error(&format!(
                            "First-run seed: failed to create Default profile: {}",
                            e
                        )),
                    }
                }
            }

            let initial_filter_config = load_initial_filter_config(app.handle());

            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let scanner_shared_state = Arc::new(RwLock::new(None));

            // Shared scanner state
            let state = AppState {
                is_scanning: Arc::new(AtomicBool::new(false)),
                should_auto_scan: Arc::new(AtomicBool::new(true)),
                filter_config: Arc::new(RwLock::new(initial_filter_config)),
                verbose_filter_logging: Arc::new(AtomicBool::new(false)),
                live_match_highlight: Arc::new(AtomicBool::new(false)),
                auto_always_show_items: Arc::new(AtomicBool::new(true)),
                auto_no_pickup: Arc::new(AtomicBool::new(true)),
                reveal_hidden_active: Arc::new(AtomicBool::new(false)),
                filter_config_generation: Arc::new(AtomicU64::new(0)),
                scanner_thread: Arc::new(Mutex::new(None)),
                game_status: Arc::new(AtomicU8::new(GAME_STATUS_UNKNOWN)),
                items_dictionary: Arc::new(RwLock::new(cached_items)),
                loot_history: Arc::new(RwLock::new(LootHistory::new())),
                breakpoints_polling: Arc::new(AtomicBool::new(false)),
                stats_polling: Arc::new(AtomicBool::new(false)),
                speedcalc_table: Arc::new(RwLock::new(None)),
                weapon_base_catalog: Arc::new(RwLock::new(cached_weapon_bases)),
                dps_reset_pending: Arc::new(AtomicBool::new(false)),
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                scanner_shared_state: scanner_shared_state.clone(),
            };
            let is_scanning = state.is_scanning.clone();
            let should_auto_scan = state.should_auto_scan.clone();
            let filter_config = state.filter_config.clone();
            let verbose_filter_logging = state.verbose_filter_logging.clone();
            let live_match_highlight = state.live_match_highlight.clone();
            let auto_always_show_items = state.auto_always_show_items.clone();
            let auto_no_pickup = state.auto_no_pickup.clone();
            let reveal_hidden_active = state.reveal_hidden_active.clone();
            let filter_config_generation = state.filter_config_generation.clone();
            let scanner_thread = state.scanner_thread.clone();
            let game_status = state.game_status.clone();
            let items_dictionary = state.items_dictionary.clone();
            let loot_history = state.loot_history.clone();
            let breakpoints_polling = state.breakpoints_polling.clone();
            let stats_polling = state.stats_polling.clone();
            let speedcalc_table_for_cache = state.speedcalc_table.clone();
            let weapon_base_catalog = state.weapon_base_catalog.clone();
            let dps_reset_pending = state.dps_reset_pending.clone();
            app.manage(state);
            app.manage(mxl_item_api::MxlItemApiState::default());

            if let Some(dir) = app.handle().path().app_data_dir().ok() {
                if let Some(table) = speedcalc_data::load_from_cache(&dir) {
                    if let Ok(mut guard) = speedcalc_table_for_cache.write() {
                        *guard = Some(table);
                    }
                }
            }

            // Initialize hotkey state
            let hotkey_state = HotkeyState::new();
            let edit_mode_state = EditModeState::new();
            let reveal_hidden_state = RevealHiddenState::new(reveal_hidden_active.clone());
            let loot_history_hotkey_state = LootHistoryHotkeyState::new();
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let item_search_hotkey_state = ItemSearchHotkeyState::new(scanner_shared_state.clone());
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            let item_search_hotkey_state = ItemSearchHotkeyState::new();
            let dps_meter_reset_state = DpsMeterResetHotkeyState::new();
            let game_create_autofill_state =
                hotkeys::GameCreateAutofillHotkeyState::new(game_status.clone());

            // Load settings and start hotkey listener
            let app_handle_for_hotkeys = app.handle().clone();
            let app_handle_for_edit_mode = app.handle().clone();
            let app_handle_for_reveal = app.handle().clone();
            let app_handle_for_loot_history = app.handle().clone();
            let app_handle_for_item_search = app.handle().clone();
            let app_handle_for_dps_reset = app.handle().clone();
            match settings::load_settings(app.handle().clone()) {
                Ok(loaded_settings) => {
                    hotkey_state
                        .start(app_handle_for_hotkeys, loaded_settings.toggle_window_hotkey);
                    edit_mode_state.start(
                        app_handle_for_edit_mode,
                        loaded_settings.edit_overlay_hotkey,
                    );
                    reveal_hidden_state
                        .start(app_handle_for_reveal, loaded_settings.reveal_hidden_hotkey);
                    loot_history_hotkey_state.start(
                        app_handle_for_loot_history,
                        loaded_settings.loot_history_hotkey,
                    );
                    item_search_hotkey_state.start(
                        app_handle_for_item_search,
                        loaded_settings.item_search_hotkey,
                    );
                    if let Some(hk) = loaded_settings.dps_meter.hotkey_reset.clone() {
                        dps_meter_reset_state.start(app_handle_for_dps_reset, hk);
                    }
                    game_create_autofill_state.start(hotkeys::GameCreateAutofillConfig {
                        hotkey: loaded_settings.game_create_autofill_hotkey.clone(),
                        name_prefix: loaded_settings.game_create_name_prefix.clone(),
                        password: loaded_settings.game_create_password.clone(),
                        password_prefix: loaded_settings.game_create_password_prefix.clone(),
                        password_use_prefix: loaded_settings.game_create_password_use_prefix,
                        description: loaded_settings.game_create_description.clone(),
                    });
                    verbose_filter_logging
                        .store(loaded_settings.verbose_filter_logging, Ordering::SeqCst);
                    auto_always_show_items
                        .store(loaded_settings.auto_always_show_items, Ordering::SeqCst);
                    auto_no_pickup.store(loaded_settings.auto_no_pickup, Ordering::SeqCst);
                }
                Err(e) => {
                    log_error(&format!("Failed to load settings for hotkeys: {}", e));
                    // Start with default hotkeys
                    hotkey_state.start(app_handle_for_hotkeys, hotkeys::HotkeyConfig::default());
                    let defaults = settings::AppSettings::default();
                    edit_mode_state.start(app_handle_for_edit_mode, defaults.edit_overlay_hotkey);
                    reveal_hidden_state.start(app_handle_for_reveal, defaults.reveal_hidden_hotkey);
                    loot_history_hotkey_state
                        .start(app_handle_for_loot_history, defaults.loot_history_hotkey);
                    item_search_hotkey_state
                        .start(app_handle_for_item_search, defaults.item_search_hotkey);
                    let _ = app_handle_for_dps_reset;
                    game_create_autofill_state.start(hotkeys::GameCreateAutofillConfig {
                        hotkey: defaults.game_create_autofill_hotkey,
                        name_prefix: defaults.game_create_name_prefix,
                        password: defaults.game_create_password,
                        password_prefix: defaults.game_create_password_prefix,
                        password_use_prefix: defaults.game_create_password_use_prefix,
                        description: defaults.game_create_description,
                    });
                }
            }

            app.manage(hotkey_state);
            app.manage(edit_mode_state);
            app.manage(reveal_hidden_state);
            app.manage(loot_history_hotkey_state);
            app.manage(item_search_hotkey_state);
            app.manage(dps_meter_reset_state);
            app.manage(game_create_autofill_state);

            // Spawn auto-scanner monitor
            let app_handle = app.handle().clone();
            spawn_auto_scanner(
                is_scanning.clone(),
                should_auto_scan.clone(),
                filter_config.clone(),
                verbose_filter_logging.clone(),
                live_match_highlight.clone(),
                auto_always_show_items.clone(),
                auto_no_pickup.clone(),
                reveal_hidden_active.clone(),
                filter_config_generation.clone(),
                scanner_thread.clone(),
                game_status.clone(),
                items_dictionary.clone(),
                loot_history.clone(),
                breakpoints_polling.clone(),
                stats_polling.clone(),
                weapon_base_catalog.clone(),
                dps_reset_pending.clone(),
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                scanner_shared_state.clone(),
                app_handle,
            );

            // When the main window is closed, stop everything, close overlay windows
            // and terminate the application.
            if let Some(main_window) = app.get_webview_window("main") {
                let is_scanning_clone = is_scanning.clone();
                let should_auto_scan_clone = should_auto_scan.clone();
                let scanner_thread_clone = scanner_thread.clone();
                let app_handle_clone = app.handle().clone();
                main_window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { .. } = event {
                        should_auto_scan_clone.store(false, Ordering::SeqCst);
                        is_scanning_clone.store(false, Ordering::SeqCst);

                        if let Some(overlay) = app_handle_clone.get_webview_window("overlay") {
                            if let Err(e) = overlay.close() {
                                log_error(&format!(
                                    "Failed to close overlay window on main close: {}",
                                    e
                                ));
                            }
                        }
                        let handle_opt = scanner_thread_clone.lock().unwrap().take();
                        let ah = app_handle_clone.clone();
                        thread::spawn(move || {
                            let watchdog_fired = Arc::new(AtomicBool::new(false));
                            let wf_w = watchdog_fired.clone();
                            let ah_w = ah.clone();
                            thread::spawn(move || {
                                thread::sleep(Duration::from_millis(2500));
                                wf_w.store(true, Ordering::SeqCst);
                                log_error("scanner join watchdog fired after 2.5s; exiting");
                                ah_w.exit(0);
                            });
                            if let Some(h) = handle_opt {
                                let _ = h.join();
                            }
                            if !watchdog_fired.load(Ordering::SeqCst) {
                                ah.exit(0);
                            }
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_devtools,
            get_scanner_status,
            get_game_status,
            get_items_dictionary,
            get_loot_history,
            clear_loot_history,
            set_filter_config,
            set_verbose_filter_logging,
            set_live_match_highlight,
            set_auto_always_show_items,
            set_auto_no_pickup,
            set_breakpoints_polling,
            set_stats_polling,
            get_speedcalc_data,
            refresh_speedcalc_data,
            unique_stats_db_sync::check_unique_stats_db_update,
            unique_stats_db_sync::download_unique_stats_db,
            get_weapon_base_catalog,
            refresh_game_data_caches,
            sync_overlay_with_game,
            set_overlay_interactive,
            set_overlay_edit_mode,
            parse_filter_dsl,
            validate_filter_dsl,
            explain_filter_line,
            get_item_filter_action,
            settings::load_settings,
            settings::save_settings,
            settings::get_window_state,
            settings::save_window_state,
            sounds::import_sound_file,
            sounds::delete_sound_file,
            sounds::play_audio_bytes_native,
            sounds::should_use_native_audio,
            hotkeys::update_hotkey,
            hotkeys::update_edit_mode_hotkey,
            hotkeys::update_reveal_hidden_hotkey,
            hotkeys::update_loot_history_hotkey,
            hotkeys::update_item_search_hotkey,
            hotkeys::update_dps_meter_reset_hotkey,
            hotkeys::update_game_create_autofill_hotkey,
            reset_dps_session,
            mxl_item_api::search_mxl_items,
            profiles::list_profiles,
            profiles::load_profile,
            profiles::save_profile,
            profiles::delete_profile,
            profiles::rename_profile,
            profiles::duplicate_profile,
            profiles::create_profile,
            updater::check_for_updates,
            updater::start_update,
            updater::restart_app,
            open_app_folder,
            open_external_url,
            get_changelog
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
