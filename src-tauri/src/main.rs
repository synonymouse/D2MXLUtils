#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod d2types;
mod injection;
mod logger;
mod notifier;
mod offsets;
mod process;
mod rules;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::logger::{error as log_error, info as log_info};

use notifier::DropScanner;

// Windows-only imports for process / overlay / privileges
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HANDLE, HWND, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongW, GetWindowRect, MoveWindow, SetWindowLongW,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
#[cfg(target_os = "windows")]
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevationType,
    TokenLinkedToken, LUID_AND_ATTRIBUTES, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION_TYPE,
    TOKEN_LINKED_TOKEN, TOKEN_PRIVILEGES, TOKEN_QUERY, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;

/// Shared state for controlling the scanner
struct AppState {
    is_scanning: Arc<AtomicBool>,
}

#[tauri::command]
fn start_scanner(state: tauri::State<AppState>, app: AppHandle) -> String {
    // Check if already running
    if state.is_scanning.load(Ordering::SeqCst) {
        return "Scanner is already running".to_string();
    }
    
    // Set scanning flag
    state.is_scanning.store(true, Ordering::SeqCst);
    println!("Scanner starting...");
    log_info("Scanner starting...");
    
    // Emit status to frontend
    if let Err(e) = app.emit("scanner-status", "starting") {
        eprintln!("Failed to emit event: {}", e);
        log_error(&format!("Failed to emit event (starting): {}", e));
    }
    
    // Clone what we need for the background thread
    let is_scanning = state.is_scanning.clone();
    let app_handle = app.clone();
    
    // Spawn background scanning thread
    thread::spawn(move || {
        // Try to create scanner
        let mut scanner = match DropScanner::new() {
            Ok(s) => {
                println!("Scanner attached to Diablo II");
                log_info("Scanner attached to Diablo II");
                if let Err(e) = app_handle.emit("scanner-status", "running") {
                    eprintln!("Failed to emit event: {}", e);
                    log_error(&format!("Failed to emit event (running): {}", e));
                }
                s
            }
            Err(e) => {
                eprintln!("Failed to attach to Diablo II: {}", e);
                log_error(&format!("Failed to attach to Diablo II: {}", e));
                if let Err(e) = app_handle.emit("scanner-status", "error") {
                    eprintln!("Failed to emit event: {}", e);
                    log_error(&format!("Failed to emit event (error): {}", e));
                }
                is_scanning.store(false, Ordering::SeqCst);
                return;
            }
        };
        
        let mut was_ingame = false;
        
        // Main scanning loop
        while is_scanning.load(Ordering::SeqCst) {
            let ingame = scanner.is_ingame();
            
            // Detect entering a new game
            if ingame && !was_ingame {
                println!("Entered game, clearing item cache");
                log_info("Entered game, clearing item cache");
                scanner.clear_cache();
                if let Err(e) = app_handle.emit("game-status", "ingame") {
                    eprintln!("Failed to emit event: {}", e);
                    log_error(&format!("Failed to emit event (ingame): {}", e));
                }
            } else if !ingame && was_ingame {
                println!("Left game");
                log_info("Left game");
                if let Err(e) = app_handle.emit("game-status", "menu") {
                    eprintln!("Failed to emit event: {}", e);
                    log_error(&format!("Failed to emit event (menu): {}", e));
                }
            }
            was_ingame = ingame;
            
            // Scan for items
            if ingame {
                let items = scanner.tick();
                for item in items {
                    println!("Found item: {} ({})", item.name, item.quality);
                    log_info(&format!("Found item: {} ({})", item.name, item.quality));
                    
                    // Emit item-drop event to frontend
                    if let Err(e) = app_handle.emit("item-drop", &item) {
                        eprintln!("Failed to emit item-drop event: {}", e);
                        log_error(&format!("Failed to emit item-drop event: {}", e));
                    }
                }
            }
            
            // Sleep between scans (200ms as in original D2Stats)
            thread::sleep(Duration::from_millis(200));
        }
        
        println!("Scanner thread stopped");
        log_info("Scanner thread stopped");
        if let Err(e) = app_handle.emit("scanner-status", "stopped") {
            eprintln!("Failed to emit event: {}", e);
            log_error(&format!("Failed to emit event (stopped): {}", e));
        }
    });
    
    "Scanner started".to_string()
}

#[tauri::command]
fn stop_scanner(state: tauri::State<AppState>, app: AppHandle) -> String {
    if !state.is_scanning.load(Ordering::SeqCst) {
        return "Scanner is not running".to_string();
    }
    
    // Signal the scanner to stop
    state.is_scanning.store(false, Ordering::SeqCst);
    println!("Scanner stop requested");
    log_info("Scanner stop requested");
    
    if let Err(e) = app.emit("scanner-status", "stopping") {
        eprintln!("Failed to emit event: {}", e);
        log_error(&format!("Failed to emit event (stopping): {}", e));
    }
    
    "Scanner stopped".to_string()
}

#[tauri::command]
fn get_scanner_status(state: tauri::State<AppState>) -> bool {
    state.is_scanning.load(Ordering::SeqCst)
}

/// Sync the transparent overlay window with the Diablo II game window.
///
/// - Positions and resizes the `overlay` window to match Diablo II bounds
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
    if app.get_webview_window("overlay").is_none() {
        return Err("Overlay window with label 'overlay' not found".to_string());
    }

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

    // Apply extended styles: layered + transparent (click-through) + toolwindow (hide from Alt+Tab)
    unsafe {
        let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
        let new_ex_style = ex_style
            | WS_EX_LAYERED.0 as i32
            | WS_EX_TRANSPARENT.0 as i32
            | WS_EX_TOOLWINDOW.0 as i32;

        SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex_style);

        // Move and resize overlay to match game window and ensure it is top-most over the game
        if let Err(e) = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1)) {
            return Err(format!("MoveWindow failed: {}", e));
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
        let _ = ShowWindow(hwnd_overlay, SW_SHOWNA);
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
            log_info(&format!(
                "Failed to open process token for SeDebugPrivilege: {}",
                e
            ));
            return;
        }

        // Resolve the LUID for SeDebugPrivilege.
        let mut luid = LUID::default();
        if let Err(e) = LookupPrivilegeValueW(None, SE_DEBUG_NAME, &mut luid) {
            log_info(&format!(
                "LookupPrivilegeValueW(SeDebugPrivilege) failed: {}",
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

        match result {
            Ok(_) => {
                log_info("SeDebugPrivilege enabled (or already enabled)");
            }
            Err(e) => {
                log_info(&format!(
                    "AdjustTokenPrivileges for SeDebugPrivilege failed: {}",
                    e
                ));
            }
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
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle).is_err() {
            log_info("Failed to open process token, skipping elevation check");
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

        if result.is_err() {
            log_info("Failed to get token elevation type");
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            return;
        }

        // TokenElevationTypeFull (2) means the process is elevated via UAC
        // We need to get the linked token (non-elevated user token) to find correct AppData
        if elevation_type.0 != 2 {
            // Not elevated via UAC, no need to adjust WebView2 path
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            log_info("Process is not UAC-elevated, using default WebView2 data folder");
            return;
        }

        log_info("Process is UAC-elevated, configuring WebView2 data folder for linked user");

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

        if result.is_err() {
            log_info("Failed to get linked token, using default WebView2 data folder");
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

                    log_info(&format!(
                        "Setting WEBVIEW2_USER_DATA_FOLDER to: {}",
                        webview2_path
                    ));

                    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview2_path);
                }
            }
            Err(e) => {
                log_info(&format!(
                    "Failed to get LocalAppData path for linked user: {:?}",
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
        .setup(|app| {
            app.manage(AppState {
                is_scanning: Arc::new(AtomicBool::new(false)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_scanner,
            stop_scanner,
            get_scanner_status,
            sync_overlay_with_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
