#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod d2types;
mod hotkeys;
mod injection;
mod items_cache;
mod logger;
mod loot_filter_hook;
mod notifier;
mod offsets;
mod process;
mod profiles;
mod rules;
mod settings;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::hotkeys::{EditModeState, HotkeyState};
use crate::logger::{error as log_error, info as log_info};

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
    SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNA, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// Shared state for controlling the scanner
struct AppState {
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
    /// Filter configuration shared with scanner thread
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    /// Whether filtering is enabled
    filter_enabled: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    // Joined on shutdown so DropScanner::drop → loot_hook.eject runs before exit.
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
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
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
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
            let mut scanner = match DropScanner::new() {
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

            let mut was_ingame = false;
            let mut dict_published = false;

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
                    if let Err(e) = app_handle.emit("game-status", "ingame") {
                        log_error(&format!("Failed to emit event (ingame): {}", e));
                    }
                } else if !ingame && was_ingame {
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

                // Scan for items
                if ingame {
                    let items = scanner.tick();
                    for item in items {
                        // Emit item-drop event to frontend
                        if let Err(e) = app_handle.emit("item-drop", &item) {
                            log_error(&format!("Failed to emit item-drop event: {}", e));
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
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
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
                    filter_config_generation.clone(),
                    scanner_thread.clone(),
                    game_status.clone(),
                    items_dictionary.clone(),
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
    // Re-sync immediately so the style change doesn't wait ~250 ms for the next tick.
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

/// Sync the transparent overlay window with the Diablo II game window.
///
/// - Positions and resizes the `overlay` window to match Diablo II bounds
/// - Shows overlay only when Diablo II is the foreground window
/// - Applies WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW styles
#[tauri::command]
fn sync_overlay_with_game(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        sync_overlay_with_game_impl(&app)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app; // suppress unused warning
        Err("Overlay sync is only supported on Windows".to_string())
    }
}

/// Track if overlay was visible in the previous sync call
static OVERLAY_WAS_VISIBLE: AtomicBool = AtomicBool::new(false);

// Read by sync_overlay_with_game_impl so its 250 ms style re-apply loop
// honors whatever set_overlay_interactive last set.
static OVERLAY_CLICK_THROUGH: AtomicBool = AtomicBool::new(true);

#[cfg(target_os = "windows")]
fn sync_overlay_with_game_impl(app: &AppHandle) -> Result<(), String> {
    // Find Diablo II top-level window by class name
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

    // Ensure Tauri overlay window exists (by label)
    let overlay_window = app
        .get_webview_window("overlay")
        .ok_or("Overlay window with label 'overlay' not found")?;

    // Find overlay OS window by its title
    let title_wide: Vec<u16> = OsStr::new("D2MXLUtils Overlay")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_overlay = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr())) }
        .map_err(|_| "Overlay OS window 'D2MXLUtils Overlay' not found".to_string())?;

    if hwnd_overlay.0.is_null() {
        return Err("Overlay HWND is null".to_string());
    }

    // If game is not the foreground window, hide overlay and exit
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0 != hwnd_game.0 {
            let _ = ShowWindow(hwnd_overlay, SW_HIDE);
            let _ = overlay_window.hide();
            OVERLAY_WAS_VISIBLE.store(false, Ordering::SeqCst);
            return Ok(());
        }
    }

    // Read game window rect
    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd_game, &mut rect).map_err(|e| format!("GetWindowRect failed: {}", e))?;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    // Check if overlay was hidden before (transition from hidden -> visible)
    let was_visible = OVERLAY_WAS_VISIBLE.swap(true, Ordering::SeqCst);

    // WS_EX_TRANSPARENT is toggled by OVERLAY_CLICK_THROUGH so the edit-mode
    // hotkey can make the overlay receive input.
    unsafe {
        let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
        let base_style = ex_style | WS_EX_LAYERED.0 as i32 | WS_EX_TOOLWINDOW.0 as i32;
        let new_ex_style = if OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst) {
            base_style | WS_EX_TRANSPARENT.0 as i32
        } else {
            base_style & !(WS_EX_TRANSPARENT.0 as i32)
        };

        SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex_style);

        // Disable the Windows 11 DWM border. Even borderless/undecorated windows
        // get a 1px accent frame on Win11, which shows up as a faint rectangle
        // on top of the game on a transparent layered overlay. Setting the border
        // color to DWMWA_COLOR_NONE (0xFFFFFFFE) tells DWM to skip that frame.
        // Silently ignored on Windows 10 (attribute unsupported).
        const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
        let _ = DwmSetWindowAttribute(
            hwnd_overlay,
            DWMWA_BORDER_COLOR,
            &DWMWA_COLOR_NONE as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        // Workaround for WebView2 transparency bug on Windows:
        // WebView2 doesn't apply transparency until the window is resized.
        // When transitioning from hidden to visible, resize by 1 pixel then back.
        if !was_visible {
            // First resize to different size
            let _ = MoveWindow(
                hwnd_overlay,
                rect.left,
                rect.top,
                width + 1,
                height + 1,
                BOOL(1),
            );
            let _ = ShowWindow(hwnd_overlay, SW_SHOWNA);
            let _ = overlay_window.show();
            // Then resize to correct size
            let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
        } else {
            // Normal case: just move/resize to match game window
            let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
            let _ = ShowWindow(hwnd_overlay, SW_SHOWNA);
            let _ = overlay_window.show();
        }

        // Reassert top-most z-order without stealing focus
        let _ = SetWindowPos(
            hwnd_overlay,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
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

fn main() {
    // Enable SeDebugPrivilege so OpenProcess has the same behavior as legacy tools.
    enable_debug_privilege();

    // Configure WebView2 data folder for elevated processes BEFORE Tauri init
    setup_webview2_for_elevation();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let cached_items = items_cache::load_items_cache(app.handle());

            // Shared scanner state
            let state = AppState {
                is_scanning: Arc::new(AtomicBool::new(false)),
                should_auto_scan: Arc::new(AtomicBool::new(true)),
                filter_config: Arc::new(RwLock::new(None)),
                filter_enabled: Arc::new(AtomicBool::new(true)),
                filter_config_generation: Arc::new(AtomicU64::new(0)),
                scanner_thread: Arc::new(Mutex::new(None)),
                game_status: Arc::new(AtomicU8::new(GAME_STATUS_UNKNOWN)),
                items_dictionary: Arc::new(RwLock::new(cached_items)),
            };
            let is_scanning = state.is_scanning.clone();
            let should_auto_scan = state.should_auto_scan.clone();
            let filter_config = state.filter_config.clone();
            let filter_enabled = state.filter_enabled.clone();
            let filter_config_generation = state.filter_config_generation.clone();
            let scanner_thread = state.scanner_thread.clone();
            let game_status = state.game_status.clone();
            let items_dictionary = state.items_dictionary.clone();
            app.manage(state);

            // Initialize hotkey state
            let hotkey_state = HotkeyState::new();
            let edit_mode_state = EditModeState::new();

            // Load settings and start hotkey listener
            let app_handle_for_hotkeys = app.handle().clone();
            let app_handle_for_edit_mode = app.handle().clone();
            match settings::load_settings(app.handle().clone()) {
                Ok(loaded_settings) => {
                    hotkey_state
                        .start(app_handle_for_hotkeys, loaded_settings.toggle_window_hotkey);
                    edit_mode_state.start(
                        app_handle_for_edit_mode,
                        loaded_settings.edit_overlay_hotkey,
                    );
                }
                Err(e) => {
                    log_error(&format!("Failed to load settings for hotkeys: {}", e));
                    // Start with default hotkeys
                    hotkey_state.start(app_handle_for_hotkeys, hotkeys::HotkeyConfig::default());
                    edit_mode_state.start(
                        app_handle_for_edit_mode,
                        settings::AppSettings::default().edit_overlay_hotkey,
                    );
                }
            }

            app.manage(hotkey_state);
            app.manage(edit_mode_state);

            // Spawn auto-scanner monitor
            let app_handle = app.handle().clone();
            spawn_auto_scanner(
                is_scanning.clone(),
                should_auto_scan.clone(),
                filter_config.clone(),
                filter_enabled.clone(),
                filter_config_generation.clone(),
                scanner_thread.clone(),
                game_status.clone(),
                items_dictionary.clone(),
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
            set_filter_config,
            set_filter_enabled,
            sync_overlay_with_game,
            set_overlay_interactive,
            parse_filter_dsl,
            validate_filter_dsl,
            get_item_filter_action,
            settings::load_settings,
            settings::save_settings,
            settings::get_window_state,
            settings::save_window_state,
            hotkeys::update_hotkey,
            hotkeys::update_edit_mode_hotkey,
            profiles::list_profiles,
            profiles::load_profile,
            profiles::save_profile,
            profiles::delete_profile,
            profiles::rename_profile,
            profiles::duplicate_profile,
            profiles::create_profile
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
