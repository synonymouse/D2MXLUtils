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

const REPO_OWNER: &str = "pertinate";
const REPO_NAME: &str = "D2MXLUtils";
#[cfg(target_os = "windows")]
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
                .find(|a| {
                    #[cfg(target_os = "windows")]
                    {
                        a.name == ASSET_NAME
                    }
                    #[cfg(target_os = "linux")]
                    {
                        // release.yml now uploads the plain lowercase
                        // "d2mxlutils.appimage" (matching the Windows job's
                        // "d2mxlutils.exe"), but older releases still carry
                        // Tauri's default versioned name (e.g.
                        // "D2MXLUtils_1.26.2_amd64.AppImage") — match by
                        // extension, case-insensitively, to cover both.
                        a.name.to_ascii_lowercase().ends_with(".appimage")
                    }
                    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                    {
                        false
                    }
                })
                .ok_or_else(|| {
                    format!("no matching release asset for this platform in v{}", ver)
                })?;

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

    let file = std::fs::File::create(&tmp_path).map_err(|e| format!("create temp file: {}", e))?;
    let mut writer = ProgressWriter::new(file, app.clone());

    Download::from_url(url)
        .set_header(ACCEPT, HeaderValue::from_static("application/octet-stream"))
        .show_progress(false)
        .download_to(&mut writer)
        .map_err(|e| format!("download: {}", e))?;

    writer.flush().ok();
    drop(writer); // ensure file handle is closed before the swap moves it

    log_info("updater: download complete, applying self-replace");

    #[cfg(target_os = "windows")]
    {
        self_update::self_replace::self_replace(&tmp_path)
            .map_err(|e| format!("self_replace: {}", e))?;

        // self_replace moves the file; remove any leftover just in case.
        let _ = std::fs::remove_file(&tmp_path);
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(&tmp_path)
            .map_err(|e| format!("stat downloaded AppImage: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_path, perms)
            .map_err(|e| format!("chmod +x downloaded AppImage: {}", e))?;

        let target = appimage_path()?;
        // Same-directory rename (download_path() already guarantees this)
        // so it's a same-filesystem atomic replace, valid even though
        // `target` is this process' own running AppImage.
        std::fs::rename(&tmp_path, &target)
            .map_err(|e| format!("rename over running AppImage: {}", e))?;
    }
    Ok(())
}

/// Path of the running `.AppImage`, from the env var its own runtime sets
/// (not `current_exe()` — see the module doc comment for why that resolves
/// inside the read-only squashfs mount instead). Absent when not actually
/// running from an AppImage (e.g. a raw dev build), which is a real error
/// here rather than something to silently fall back from.
#[cfg(target_os = "linux")]
fn appimage_path() -> Result<PathBuf, String> {
    std::env::var("APPIMAGE")
        .map(PathBuf::from)
        .map_err(|_| "APPIMAGE env var not set — not running from an AppImage".to_string())
}

/// Put the downloaded file next to the running executable/AppImage so the
/// subsequent swap is always a same-volume rename (works around a
/// potential cross-drive failure when TEMP is on a different volume, and
/// is required on Linux anyway for the rename-over-running-file trick).
fn download_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {}", e))?;
        let dir = exe
            .parent()
            .ok_or_else(|| "current exe has no parent directory".to_string())?
            .to_path_buf();
        Ok(dir.join("d2mxlutils-update.new.exe"))
    }
    #[cfg(target_os = "linux")]
    {
        let appimage = appimage_path()?;
        let dir = appimage
            .parent()
            .ok_or_else(|| "AppImage path has no parent directory".to_string())?
            .to_path_buf();
        Ok(dir.join("d2mxlutils-update.new.AppImage"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Err("unsupported platform".to_string())
    }
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
