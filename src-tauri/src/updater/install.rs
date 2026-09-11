use std::io::Write;
use std::path::PathBuf;

use http::header::{HeaderValue, ACCEPT};
use self_update::Download;
use tauri::AppHandle;

use crate::logger::info as log_info;

use super::progress::ProgressWriter;

pub(super) fn download_and_replace(app: &AppHandle, url: &str) -> Result<(), String> {
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
/// (not `current_exe()` — see the parent module doc comment for why that resolves
/// inside the read-only squashfs mount instead). Absent when not actually
/// running from an AppImage (e.g. a raw dev build), which is a real error
/// here rather than something to silently fall back from.
#[cfg(target_os = "linux")]
pub(super) fn appimage_path() -> Result<PathBuf, String> {
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
