#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod d2types;
mod injection;
mod notifier;
mod offsets;
mod process;
mod rules;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use notifier::DropScanner;

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
    
    // Emit status to frontend
    if let Err(e) = app.emit("scanner-status", "starting") {
        eprintln!("Failed to emit event: {}", e);
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
                if let Err(e) = app_handle.emit("scanner-status", "running") {
                    eprintln!("Failed to emit event: {}", e);
                }
                s
            }
            Err(e) => {
                eprintln!("Failed to attach to Diablo II: {}", e);
                if let Err(e) = app_handle.emit("scanner-status", "error") {
                    eprintln!("Failed to emit event: {}", e);
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
                scanner.clear_cache();
                if let Err(e) = app_handle.emit("game-status", "ingame") {
                    eprintln!("Failed to emit event: {}", e);
                }
            } else if !ingame && was_ingame {
                println!("Left game");
                if let Err(e) = app_handle.emit("game-status", "menu") {
                    eprintln!("Failed to emit event: {}", e);
                }
            }
            was_ingame = ingame;
            
            // Scan for items
            if ingame {
                let items = scanner.tick();
                for item in items {
                    println!("Found item: {} ({})", item.name, item.quality);
                    
                    // Emit item-drop event to frontend
                    if let Err(e) = app_handle.emit("item-drop", &item) {
                        eprintln!("Failed to emit item-drop event: {}", e);
                    }
                }
            }
            
            // Sleep between scans (200ms as in original D2Stats)
            thread::sleep(Duration::from_millis(200));
        }
        
        println!("Scanner thread stopped");
        if let Err(e) = app_handle.emit("scanner-status", "stopped") {
            eprintln!("Failed to emit event: {}", e);
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
    
    if let Err(e) = app.emit("scanner-status", "stopping") {
        eprintln!("Failed to emit event: {}", e);
    }
    
    "Scanner stopped".to_string()
}

#[tauri::command]
fn get_scanner_status(state: tauri::State<AppState>) -> bool {
    state.is_scanning.load(Ordering::SeqCst)
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
            get_scanner_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
