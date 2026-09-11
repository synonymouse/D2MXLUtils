//! Application commands and cross-feature cache recovery.

use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};
use crate::{AppState, GAME_STATUS_INGAME, GAME_STATUS_MENU};

#[tauri::command]
pub(crate) fn get_game_status(state: tauri::State<AppState>) -> &'static str {
    match state.game_status.load(Ordering::SeqCst) {
        GAME_STATUS_INGAME => "ingame",
        GAME_STATUS_MENU => "menu",
        _ => "unknown",
    }
}

#[tauri::command]
pub(crate) fn get_scanner_status(state: tauri::State<AppState>) -> bool {
    state.is_scanning.load(Ordering::SeqCst)
}

/// Opens the WebKit/WebView2 inspector for the calling window. Right-click's
/// native context menu is suppressed app-wide (see App.svelte) except inside
/// inputs/the rules editor, so this is the only way to reach devtools during
/// development. Debug-only: `devtools` is a real Cargo feature gate in
/// Tauri v2 (unlike v1, not automatic for debug builds), and `open_devtools`
/// only exists on the type when that feature is enabled.
#[tauri::command]
pub(crate) fn open_devtools(window: tauri::WebviewWindow) {
    #[cfg(debug_assertions)]
    window.open_devtools();
    #[cfg(not(debug_assertions))]
    let _ = window;
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
pub(crate) fn refresh_game_data_caches(
    app: AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
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

#[tauri::command]
pub(crate) fn open_app_folder(app: AppHandle) -> Result<(), String> {
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

#[tauri::command]
pub(crate) fn get_changelog() -> &'static str {
    include_str!("../../../CHANGELOG.md")
}

/// Open an http(s) URL in the user's default browser.
/// Scheme validation prevents `start` from being coaxed into launching a
/// local file or custom handler via attacker-controlled URLs.
#[tauri::command]
pub(crate) fn open_external_url(url: String) -> Result<(), String> {
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
