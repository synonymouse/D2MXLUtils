//! Auto-updater: checks GitHub releases, downloads the platform asset,
//! atomically replaces the running executable, and restarts.
//!
//! Strategy:
//! - `self_update::backends::github::ReleaseList` fetches release metadata.
//! - `self_update::Download` streams the binary through our `ProgressWriter`
//!   (which emits throttled `updater-progress` events to the frontend).
//! - Windows: `self_update::self_replace` does the atomic swap of the
//!   running `.exe` with the downloaded one.
//! - Linux: the release is an AppImage, run in place — `current_exe()`
//!   resolves to a path inside the AppImage's own read-only squashfs mount
//!   (e.g. `/tmp/.mount_XXXXX/usr/bin/d2mxlutils`), not the `.AppImage`
//!   file itself, so `self_replace` can't target it (nothing there is
//!   writable). Instead this uses the `APPIMAGE` env var the AppImage
//!   runtime sets to the real file's path, and does the swap with a plain
//!   `rename()` over it — safe even while it's the running process' own
//!   backing file, since Unix allows unlinking/replacing an open file (the
//!   old inode stays valid for this process until it exits; the new file
//!   at that path is what runs next time).
//! - Restart is an explicit `Command::new(<exe or AppImage path>).spawn()`
//!   + `exit(0)` triggered by the user clicking the update-ready button.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tauri::{AppHandle, Emitter};

use crate::logger::{error as log_error, info as log_info};

mod install;
mod progress;
mod release;

#[cfg(target_os = "linux")]
use install::appimage_path;
use install::download_and_replace;
use release::check_inner;

#[derive(serde::Serialize, Clone, Debug)]
pub struct UpdateCheckResult {
    pub status: &'static str, // "up_to_date" | "available"
    pub latest_version: Option<String>,
    pub current_version: String,
    pub asset_url: Option<String>,
}

/// Guard against concurrent download threads.
static DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn check_for_updates(manual: bool) -> Result<UpdateCheckResult, String> {
    let joined = tauri::async_runtime::spawn_blocking(check_inner)
        .await
        .map_err(|e| format!("spawn_blocking join: {}", e))?;

    match joined {
        Ok(r) => Ok(r),
        Err(e) => {
            if manual {
                log_error(&format!("updater: manual check failed: {}", e));
                Err(e)
            } else {
                log_error(&format!("updater: auto check failed: {}", e));
                // Sentinel: frontend treats this as silent-idle for the
                // automatic startup check (no UI surfacing).
                Err("silent".to_string())
            }
        }
    }
}

#[tauri::command]
pub fn start_update(app: AppHandle, asset_url: String) -> Result<(), String> {
    if DOWNLOAD_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("already downloading".to_string());
    }

    let result = thread::Builder::new()
        .name("updater-download".into())
        .spawn(move || {
            let outcome = download_and_replace(&app, &asset_url);
            DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
            match outcome {
                Ok(()) => {
                    log_info("updater: self-replace ok");
                    if let Err(e) = app.emit("updater-ready", ()) {
                        log_error(&format!("updater: emit ready failed: {}", e));
                    }
                }
                Err(e) => {
                    log_error(&format!("updater: download/replace failed: {}", e));
                    if let Err(e2) = app.emit("updater-error", &e) {
                        log_error(&format!("updater: emit error failed: {}", e2));
                    }
                }
            }
        });

    if let Err(e) = result {
        DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
        return Err(format!("spawn updater thread: {}", e));
    }
    Ok(())
}

#[tauri::command]
pub fn restart_app(app: AppHandle) -> Result<(), String> {
    // Linux: current_exe() would resolve into the OLD AppImage's now-stale
    // squashfs mount, not the freshly-renamed file at APPIMAGE's path.
    #[cfg(target_os = "windows")]
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    #[cfg(target_os = "linux")]
    let exe = appimage_path()?;

    log_info(&format!("updater: restarting via {:?}", exe));
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("spawn new process: {}", e))?;
    app.exit(0);
    Ok(())
}
