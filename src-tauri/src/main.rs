#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod process;

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

struct AppState {
    is_scanning: Mutex<bool>,
}

#[tauri::command]
fn start_scanner(state: tauri::State<AppState>, app: AppHandle) -> String {
    let mut is_scanning = state.is_scanning.lock().unwrap();
    if *is_scanning {
        return "Scanner is already running".to_string();
    }
    *is_scanning = true;
    println!("Scanner started");
    
    // Emit event to frontend to update status
    if let Err(e) = app.emit("scanner-status", "running") {
        eprintln!("Failed to emit event: {}", e);
    }

    // Try to attach to D2 process
    match process::D2Context::new() {
        Ok(ctx) => println!("Successfully attached to Diablo II. D2Client base: 0x{:x}", ctx.d2_client),
        Err(e) => println!("Could not attach to Diablo II: {}", e),
    }
    
    // TODO: In the future, when an item is found, we will emit:
    // app.emit("item-drop", item_payload).unwrap();
    
    "Scanner started".to_string()
}

#[tauri::command]
fn stop_scanner(state: tauri::State<AppState>, app: AppHandle) -> String {
    let mut is_scanning = state.is_scanning.lock().unwrap();
    if !*is_scanning {
        return "Scanner is not running".to_string();
    }
    *is_scanning = false;
    println!("Scanner stopped");
    
    if let Err(e) = app.emit("scanner-status", "stopped") {
        eprintln!("Failed to emit event: {}", e);
    }
    
    "Scanner stopped".to_string()
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState { 
                is_scanning: Mutex::new(false) 
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_scanner, stop_scanner])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
