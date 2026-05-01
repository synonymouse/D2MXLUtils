#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod d2types;
mod hotkeys;
mod injection;
mod items_cache;
mod logger;
mod loot_filter_hook;
mod loot_history;
mod map_marker;
mod notifier;
mod offsets;
mod process;
mod profiles;
mod rules;
mod settings;
mod updater;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::hotkeys::{EditModeState, HotkeyState, LootHistoryHotkeyState, RevealHiddenState};
use crate::logger::{error as log_error, info as log_info};
use crate::loot_history::{LootEntry, LootHistory, PickupState};

use notifier::{DropScanner, ItemsDictionary};

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
    FindWindowW, GetForegroundWindow, GetWindowLongW, GetWindowRect, MoveWindow, SetWindowLongW,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, WS_BORDER,
    WS_CAPTION, WS_DLGFRAME, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
};

/// Shared state for controlling the scanner
struct AppState {
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
    /// Filter configuration shared with scanner thread
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    /// Whether filtering is enabled
    filter_enabled: Arc<AtomicBool>,
    /// When true, scanner logs per-item filter decisions (noisy; opt-in for debugging).
    verbose_filter_logging: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    /// Driven by the reveal-hidden hotkey watcher; mirrored into the hook.
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    // Joined on shutdown so DropScanner::drop → loot_hook.eject runs before exit.
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    /// Session loot history shared with scanner thread.
    loot_history: Arc<RwLock<LootHistory>>,
}

const GAME_STATUS_UNKNOWN: u8 = 0;
const GAME_STATUS_INGAME: u8 = 1;
const GAME_STATUS_MENU: u8 = 2;

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

#[cfg(not(target_os = "windows"))]
fn is_diablo2_running() -> bool {
    false
}

/// Start the scanner (internal function used by auto-start and manual start)
fn start_scanner_internal(
    is_scanning: Arc<AtomicBool>,
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    filter_enabled: Arc<AtomicBool>,
    verbose_filter_logging: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    loot_history: Arc<RwLock<LootHistory>>,
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
            // Try to create scanner
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
            scanner.set_filter_enabled(filter_enabled.load(Ordering::SeqCst));
            scanner.set_verbose_filter_logging(verbose_filter_logging.load(Ordering::SeqCst));

            // Seed the hook with the current flag so a key already held on
            // attach (e.g. user reopened the game) works on frame one.
            let mut last_reveal = reveal_hidden_active.load(Ordering::SeqCst);
            if let Err(e) = scanner.set_force_show_all(last_reveal) {
                log_error(&format!("Initial set_force_show_all failed: {}", e));
            }

            let mut was_ingame = false;
            let mut dict_published = false;
            let mut pending_set_always_show = false;
            let mut last_emitted_always_show: Option<bool> = None;

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
                    if let Ok(mut hist) = loot_history.write() {
                        hist.clear();
                    }
                    if let Err(e) = app_handle.emit("loot-history-cleared", ()) {
                        log_error(&format!("Failed to emit loot-history-cleared: {}", e));
                    }
                    pending_set_always_show = true;
                    last_emitted_always_show = None;
                    if let Err(e) = app_handle.emit("game-status", "ingame") {
                        log_error(&format!("Failed to emit event (ingame): {}", e));
                    }
                } else if !ingame && was_ingame {
                    pending_set_always_show = false;
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

                // Sync filter_enabled state from AppState
                let current_filter_enabled = filter_enabled.load(Ordering::SeqCst);
                scanner.set_filter_enabled(current_filter_enabled);
                scanner.set_verbose_filter_logging(
                    verbose_filter_logging.load(Ordering::SeqCst),
                );

                let current_reveal = reveal_hidden_active.load(Ordering::SeqCst);
                if current_reveal != last_reveal {
                    if let Err(e) = scanner.set_force_show_all(current_reveal) {
                        log_error(&format!("set_force_show_all failed: {}", e));
                    }
                    last_reveal = current_reveal;
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

                    match scanner.read_always_show_items() {
                        Ok(Some(state)) => {
                            if last_emitted_always_show != Some(state) {
                                if let Err(e) = app_handle
                                    .emit("always-show-items-state", state)
                                {
                                    log_error(&format!(
                                        "Failed to emit always-show-items-state: {}",
                                        e
                                    ));
                                }
                                last_emitted_always_show = Some(state);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log_error(&format!(
                                "read_always_show_items failed: {}",
                                e
                            ));
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

                    scanner.tick_map_markers();

                    if !dict_published {
                        if let Some(dict) = scanner.items_dictionary_snapshot() {
                            if let Ok(mut guard) = items_dictionary.write() {
                                *guard = Some(dict.clone());
                            }
                            if let Err(e) = items_cache::save_items_cache(&app_handle, &dict) {
                                log_error(&format!("Failed to save items cache: {}", e));
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
                }

                thread::sleep(Duration::from_millis(30));
            }

            is_scanning.store(false, Ordering::SeqCst);
            game_status.store(GAME_STATUS_UNKNOWN, Ordering::SeqCst);
            if let Err(e) = app_handle.emit("scanner-status", "stopped") {
                log_error(&format!("Failed to emit event (stopped): {}", e));
            }
            if let Err(e) = app_handle.emit("game-status", "unknown") {
                log_error(&format!("Failed to emit event (unknown): {}", e));
            }
            // Ensure overlay is hidden when scanner stops
            if let Some(overlay) = app_handle.get_webview_window("overlay") {
                if let Err(e) = overlay.hide() {
                    log_error(&format!(
                        "Failed to hide overlay window when scanner stopped: {}",
                        e
                    ));
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
    filter_enabled: Arc<AtomicBool>,
    verbose_filter_logging: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    loot_history: Arc<RwLock<LootHistory>>,
    app_handle: AppHandle,
) {
    thread::spawn(move || {
        while should_auto_scan.load(Ordering::SeqCst) {
            // If not currently scanning, check if D2 is running
            if !is_scanning.load(Ordering::SeqCst) && is_diablo2_running() {
                start_scanner_internal(
                    is_scanning.clone(),
                    filter_config.clone(),
                    filter_enabled.clone(),
                    verbose_filter_logging.clone(),
                    auto_always_show_items.clone(),
                    reveal_hidden_active.clone(),
                    filter_config_generation.clone(),
                    scanner_thread.clone(),
                    game_status.clone(),
                    items_dictionary.clone(),
                    loot_history.clone(),
                    app_handle.clone(),
                );
            }

            // Check every 2 seconds
            thread::sleep(Duration::from_secs(2));
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
fn clear_loot_history(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
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

/// Enable or disable item filtering
#[tauri::command]
fn set_filter_enabled(enabled: bool, state: tauri::State<AppState>) {
    state.filter_enabled.store(enabled, Ordering::SeqCst);
}

/// Enable or disable the per-item `[Filter] ...` log line.
#[tauri::command]
fn set_verbose_filter_logging(enabled: bool, state: tauri::State<AppState>) {
    state
        .verbose_filter_logging
        .store(enabled, Ordering::SeqCst);
}

/// Enable or disable auto-toggling of MXL's "always show items" on game entry.
#[tauri::command]
fn set_auto_always_show_items(enabled: bool, state: tauri::State<AppState>) {
    state
        .auto_always_show_items
        .store(enabled, Ordering::SeqCst);
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
    config: rules::FilterConfig,
    item: notifier::ItemDropEvent,
) -> rules::FilterDecision {
    use crate::rules::MatchContext;
    let ctx = MatchContext::new(&item);
    config.decide(&ctx)
}

#[tauri::command]
fn set_overlay_interactive(app: AppHandle, active: bool) -> Result<(), String> {
    OVERLAY_CLICK_THROUGH.store(!active, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let _ = sync_overlay_with_game_impl(&app);
    }
    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("Overlay sync is only supported on Windows".to_string())
    }
}

static OVERLAY_WAS_VISIBLE: AtomicBool = AtomicBool::new(false);
static OVERLAY_CLICK_THROUGH: AtomicBool = AtomicBool::new(true);
static OVERLAY_STYLES_APPLIED: AtomicBool = AtomicBool::new(false);

// -1 sentinel = never applied; forces first sync to push the style.
static OVERLAY_LAST_CLICK_THROUGH_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);

#[cfg(target_os = "windows")]
static OVERLAY_LAST_RECT: Mutex<Option<RECT>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn sync_overlay_with_game_impl(app: &AppHandle) -> Result<(), String> {
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

    let overlay_window = app
        .get_webview_window("overlay")
        .ok_or("Overlay window with label 'overlay' not found")?;

    let title_wide: Vec<u16> = OsStr::new("D2MXLUtils Overlay")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_overlay = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr())) }
        .map_err(|_| "Overlay OS window 'D2MXLUtils Overlay' not found".to_string())?;

    if hwnd_overlay.0.is_null() {
        return Err("Overlay HWND is null".to_string());
    }

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0 != hwnd_game.0 {
            let _ = ShowWindow(hwnd_overlay, SW_HIDE);
            let _ = overlay_window.hide();
            OVERLAY_WAS_VISIBLE.store(false, Ordering::SeqCst);
            OVERLAY_STYLES_APPLIED.store(false, Ordering::SeqCst);
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(-1, Ordering::SeqCst);
            if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
                *last = None;
            }
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
        if just_applied {
            let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
            let desired_ct = OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
            let mut new_ex = ex_style
                | WS_EX_LAYERED.0 as i32
                | WS_EX_TOOLWINDOW.0 as i32
                | WS_EX_NOACTIVATE.0 as i32;
            if desired_ct {
                new_ex |= WS_EX_TRANSPARENT.0 as i32;
            } else {
                new_ex &= !(WS_EX_TRANSPARENT.0 as i32);
            }
            SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex);
            OVERLAY_LAST_CLICK_THROUGH_APPLIED
                .store(if desired_ct { 1 } else { 0 }, Ordering::SeqCst);

            // On some systems Tauri's `decorations: false` leaks chrome bits
            // (Aero Lite, Windhawk/ExplorerPatcher, classic theme), so strip
            // them by hand and force WS_POPUP.
            let style = GetWindowLongW(hwnd_overlay, GWL_STYLE);
            let chrome_mask = (WS_CAPTION.0
                | WS_BORDER.0
                | WS_DLGFRAME.0
                | WS_THICKFRAME.0
                | WS_SYSMENU.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0) as i32;
            let new_style = (style & !chrome_mask) | WS_POPUP.0 as i32;
            if new_style != style {
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

            // Suppress the 1px Win11 DWM accent frame; ignored on Win10.
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd_overlay,
                DWMWA_BORDER_COLOR,
                &DWMWA_COLOR_NONE as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
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
            let _ = ShowWindow(hwnd_overlay, SW_SHOWNA);
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

    let desired_ct = OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
    let desired_i8: i8 = if desired_ct { 1 } else { 0 };
    if OVERLAY_LAST_CLICK_THROUGH_APPLIED.load(Ordering::SeqCst) != desired_i8 {
        unsafe {
            let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
            let new_ex = if desired_ct {
                ex_style | WS_EX_TRANSPARENT.0 as i32
            } else {
                ex_style & !(WS_EX_TRANSPARENT.0 as i32)
            };
            SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex);
        }
        OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(desired_i8, Ordering::SeqCst);
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
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("Failed to open explorer: {}", e))?;
    Ok(())
}

/// Open an http(s) URL in the user's default browser.
/// Scheme validation prevents `start` from being coaxed into launching a
/// local file or custom handler via attacker-controlled URLs.
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http(s) URLs are allowed".into());
    }
    // `cmd /c start "" <url>` — the empty "" arg is the window title slot that
    // `start` consumes before the target, so the URL is parsed as the target.
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &url])
        .spawn()
        .map_err(|e| format!("Failed to open url: {}", e))?;
    Ok(())
}

fn main() {
    // Enable SeDebugPrivilege so OpenProcess has the same behavior as legacy tools.
    enable_debug_privilege();

    // Configure WebView2 data folder for elevated processes BEFORE Tauri init
    setup_webview2_for_elevation();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let cached_items = items_cache::load_items_cache(app.handle());

            // First-run: if the settings file has never been written, drop a
            // ready-to-use Default profile and mark it active
            if let Ok(dir) = app.handle().path().app_data_dir() {
                let settings_path = dir.join("settings.json");
                if !settings_path.exists() {
                    match profiles::seed_default_profile(app.handle()) {
                        Ok(name) => {
                            let mut s = settings::load_settings(app.handle().clone())
                                .unwrap_or_default();
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

            // Shared scanner state
            let state = AppState {
                is_scanning: Arc::new(AtomicBool::new(false)),
                should_auto_scan: Arc::new(AtomicBool::new(true)),
                filter_config: Arc::new(RwLock::new(initial_filter_config)),
                filter_enabled: Arc::new(AtomicBool::new(true)),
                verbose_filter_logging: Arc::new(AtomicBool::new(false)),
                auto_always_show_items: Arc::new(AtomicBool::new(true)),
                reveal_hidden_active: Arc::new(AtomicBool::new(false)),
                filter_config_generation: Arc::new(AtomicU64::new(0)),
                scanner_thread: Arc::new(Mutex::new(None)),
                game_status: Arc::new(AtomicU8::new(GAME_STATUS_UNKNOWN)),
                items_dictionary: Arc::new(RwLock::new(cached_items)),
                loot_history: Arc::new(RwLock::new(LootHistory::new())),
            };
            let is_scanning = state.is_scanning.clone();
            let should_auto_scan = state.should_auto_scan.clone();
            let filter_config = state.filter_config.clone();
            let filter_enabled = state.filter_enabled.clone();
            let verbose_filter_logging = state.verbose_filter_logging.clone();
            let auto_always_show_items = state.auto_always_show_items.clone();
            let reveal_hidden_active = state.reveal_hidden_active.clone();
            let filter_config_generation = state.filter_config_generation.clone();
            let scanner_thread = state.scanner_thread.clone();
            let game_status = state.game_status.clone();
            let items_dictionary = state.items_dictionary.clone();
            let loot_history = state.loot_history.clone();
            app.manage(state);

            // Initialize hotkey state
            let hotkey_state = HotkeyState::new();
            let edit_mode_state = EditModeState::new();
            let reveal_hidden_state = RevealHiddenState::new(reveal_hidden_active.clone());
            let loot_history_hotkey_state = LootHistoryHotkeyState::new();

            // Load settings and start hotkey listener
            let app_handle_for_hotkeys = app.handle().clone();
            let app_handle_for_edit_mode = app.handle().clone();
            let app_handle_for_reveal = app.handle().clone();
            let app_handle_for_loot_history = app.handle().clone();
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
                    verbose_filter_logging
                        .store(loaded_settings.verbose_filter_logging, Ordering::SeqCst);
                    auto_always_show_items
                        .store(loaded_settings.auto_always_show_items, Ordering::SeqCst);
                }
                Err(e) => {
                    log_error(&format!("Failed to load settings for hotkeys: {}", e));
                    // Start with default hotkeys
                    hotkey_state.start(app_handle_for_hotkeys, hotkeys::HotkeyConfig::default());
                    let defaults = settings::AppSettings::default();
                    edit_mode_state.start(app_handle_for_edit_mode, defaults.edit_overlay_hotkey);
                    reveal_hidden_state
                        .start(app_handle_for_reveal, defaults.reveal_hidden_hotkey);
                    loot_history_hotkey_state
                        .start(app_handle_for_loot_history, defaults.loot_history_hotkey);
                }
            }

            app.manage(hotkey_state);
            app.manage(edit_mode_state);
            app.manage(reveal_hidden_state);
            app.manage(loot_history_hotkey_state);

            // Spawn auto-scanner monitor
            let app_handle = app.handle().clone();
            spawn_auto_scanner(
                is_scanning.clone(),
                should_auto_scan.clone(),
                filter_config.clone(),
                filter_enabled.clone(),
                verbose_filter_logging.clone(),
                auto_always_show_items.clone(),
                reveal_hidden_active.clone(),
                filter_config_generation.clone(),
                scanner_thread.clone(),
                game_status.clone(),
                items_dictionary.clone(),
                loot_history.clone(),
                app_handle,
            );

            // When the main window is closed, stop everything, close the overlay window
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
                                thread::sleep(Duration::from_millis(1000));
                                wf_w.store(true, Ordering::SeqCst);
                                log_error("scanner join watchdog fired after 1s; exiting");
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
            get_scanner_status,
            get_game_status,
            get_items_dictionary,
            get_loot_history,
            clear_loot_history,
            set_filter_config,
            set_filter_enabled,
            set_verbose_filter_logging,
            set_auto_always_show_items,
            sync_overlay_with_game,
            set_overlay_interactive,
            parse_filter_dsl,
            validate_filter_dsl,
            explain_filter_line,
            get_item_filter_action,
            settings::load_settings,
            settings::save_settings,
            settings::get_window_state,
            settings::save_window_state,
            hotkeys::update_hotkey,
            hotkeys::update_edit_mode_hotkey,
            hotkeys::update_reveal_hidden_hotkey,
            hotkeys::update_loot_history_hotkey,
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
            open_external_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
