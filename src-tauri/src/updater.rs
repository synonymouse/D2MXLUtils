//! Auto-updater: checks GitHub releases, downloads the `.exe`, atomically
//! replaces the running executable, and restarts.
//!
//! Strategy:
//! - `self_update::backends::github::ReleaseList` fetches release metadata.
//! - `self_update::Download` streams the binary through our `ProgressWriter`
//!   (which emits throttled `updater-progress` events to the frontend).
//! - `self_update::self_replace` does the Windows-safe atomic swap of the
//!   running `.exe` with the downloaded one.
//! - Restart is an explicit `Command::new(current_exe()).spawn()` + `exit(0)`
//!   triggered by the user clicking the «Перезапустить» button.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use self_update::backends::github::ReleaseList;
use self_update::Download;

use http::header::{HeaderValue, ACCEPT};

use crate::logger::{error as log_error, info as log_info};

const REPO_OWNER: &str = "synonymouse";
const REPO_NAME: &str = "D2MXLUtils";
const ASSET_NAME: &str = "d2mxlutils.exe";

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
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    log_info(&format!("updater: restarting via {:?}", exe));
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("spawn new process: {}", e))?;
    app.exit(0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn check_inner() -> Result<UpdateCheckResult, String> {
    log_info("updater: checking for updates");

    let releases = ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("build release list: {}", e))?
        .fetch()
        .map_err(|e| format!("fetch releases: {}", e))?;

    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| format!("invalid CARGO_PKG_VERSION: {}", e))?;

    // Pick the newest stable release (no prerelease suffix, e.g. "1.7.0-beta.1").
    let latest = releases
        .iter()
        .filter_map(|r| semver::Version::parse(&r.version).ok().map(|v| (v, r)))
        .filter(|(v, _)| v.pre.is_empty())
        .max_by(|(a, _), (b, _)| a.cmp(b));

    match latest {
        Some((ver, rel)) if ver > current => {
            let asset = rel
                .assets
                .iter()
                .find(|a| a.name == ASSET_NAME)
                .ok_or_else(|| format!("asset '{}' missing in release v{}", ASSET_NAME, ver))?;

            log_info(&format!(
                "updater: available v{} (current v{})",
                ver, current
            ));

            Ok(UpdateCheckResult {
                status: "available",
                latest_version: Some(ver.to_string()),
                current_version: current.to_string(),
                asset_url: Some(asset.download_url.clone()),
            })
        }
        _ => {
            log_info(&format!("updater: up-to-date (current v{})", current));
            Ok(UpdateCheckResult {
                status: "up_to_date",
                latest_version: None,
                current_version: current.to_string(),
                asset_url: None,
            })
        }
    }
}

fn download_and_replace(app: &AppHandle, url: &str) -> Result<(), String> {
    let tmp_path = download_path()?;
    log_info(&format!("updater: downloading to {:?}", tmp_path));

    // Remove any stale file from a previous aborted attempt.
    let _ = std::fs::remove_file(&tmp_path);

    let file =
        std::fs::File::create(&tmp_path).map_err(|e| format!("create temp file: {}", e))?;
    let mut writer = ProgressWriter::new(file, app.clone());

    Download::from_url(url)
        .set_header(ACCEPT, HeaderValue::from_static("application/octet-stream"))
        .show_progress(false)
        .download_to(&mut writer)
        .map_err(|e| format!("download: {}", e))?;

    writer.flush().ok();
    drop(writer); // ensure file handle is closed before self_replace moves it

    log_info("updater: download complete, applying self-replace");

    self_update::self_replace::self_replace(&tmp_path)
        .map_err(|e| format!("self_replace: {}", e))?;

    // self_replace moves the file; remove any leftover just in case.
    let _ = std::fs::remove_file(&tmp_path);
    Ok(())
}

/// Put the downloaded file next to the running `.exe` so the subsequent
/// `self_replace` is always a same-volume rename (works around a potential
/// cross-drive failure when TEMP is on a different volume).
fn download_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "current exe has no parent directory".to_string())?
        .to_path_buf();
    Ok(dir.join("d2mxlutils-update.new.exe"))
}

// ---------------------------------------------------------------------------
// ProgressWriter — io::Write wrapper that counts bytes and emits throttled
// `updater-progress` events (at most ~10 Hz) to the frontend.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    downloaded: u64,
}

struct ProgressWriter<W: Write> {
    inner: W,
    app: AppHandle,
    downloaded: u64,
    last_emit: Instant,
}

impl<W: Write> ProgressWriter<W> {
    fn new(inner: W, app: AppHandle) -> Self {
        Self {
            inner,
            app,
            downloaded: 0,
            // Force the first write to emit immediately.
            last_emit: Instant::now() - Duration::from_secs(1),
        }
    }

    fn emit(&mut self, force: bool) {
        if !force && self.last_emit.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_emit = Instant::now();
        let _ = self.app.emit(
            "updater-progress",
            ProgressPayload {
                downloaded: self.downloaded,
            },
        );
    }
}

impl<W: Write> Write for ProgressWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.downloaded = self.downloaded.saturating_add(n as u64);
        self.emit(false);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()?;
        self.emit(true);
        Ok(())
    }
}
