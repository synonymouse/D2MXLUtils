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

// Windows-only imports for overlay window sync
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HWND, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongW, GetWindowRect, MoveWindow, SetWindowLongW,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
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

fn main() {
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
