//! Drop-sound file management.
//!
//! Custom audio files live in `app_data_dir/sounds/slot-{N}.{ext}`
//! (one file per slot). Used by the Sounds tab UI; the slot metadata
//! (label, volume, source kind) is persisted via `AppSettings.sounds`.

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};

const SOUNDS_DIR: &str = "sounds";
const MAX_BYTES: usize = 5 * 1024 * 1024; // 5 MB
const ALLOWED_EXTS: &[&str] = &["mp3", "wav", "ogg", "m4a", "flac"];

fn sounds_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app_data_dir: {}", e))?
        .join(SOUNDS_DIR);
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("failed to create {:?}: {}", dir, e))?;
    }
    Ok(dir)
}

fn extension_from(file_name: &str) -> Option<String> {
    PathBuf::from(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn remove_existing_for_slot(dir: &std::path::Path, slot: u8) -> std::io::Result<()> {
    for ext in ALLOWED_EXTS {
        let path = dir.join(format!("slot-{}.{}", slot, ext));
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Validates extension/size, writes bytes to `app_data_dir/sounds/slot-{N}.{ext}`,
/// removes any prior file for the slot. Returns the saved file name.
#[tauri::command]
pub fn import_sound_file(
    app: AppHandle,
    slot: u8,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    if slot < 1 {
        return Err("slot must be >= 1".to_string());
    }
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "file too large ({} bytes, max {} bytes)",
            bytes.len(),
            MAX_BYTES
        ));
    }
    let ext = extension_from(&file_name)
        .ok_or_else(|| "file has no extension".to_string())?;
    if !ALLOWED_EXTS.contains(&ext.as_str()) {
        return Err(format!(
            "unsupported format '{}' (allowed: {})",
            ext,
            ALLOWED_EXTS.join(", ")
        ));
    }

    let dir = sounds_dir(&app)?;
    let new_name = format!("slot-{}.{}", slot, ext);
    let new_path = dir.join(&new_name);

    // Write the new file first; only on success do we delete any prior file
    // for this slot (so a write failure doesn't leave the slot empty).
    fs::write(&new_path, &bytes).map_err(|e| format!("failed to write {:?}: {}", new_path, e))?;
    if let Err(e) = remove_existing_for_slot(&dir, slot) {
        log_error(&format!(
            "import_sound_file: failed to clean prior file for slot {}: {}",
            slot, e
        ));
    }
    // After cleanup, ensure the new file is still present (the cleanup pass
    // would have deleted it if its extension equals the new one).
    if !new_path.exists() {
        fs::write(&new_path, &bytes)
            .map_err(|e| format!("failed to re-write {:?}: {}", new_path, e))?;
    }
    log_info(&format!(
        "Imported sound for slot {} ({} bytes, ext={})",
        slot,
        bytes.len(),
        ext
    ));
    Ok(new_name)
}

/// Removes any file for the slot. Idempotent.
#[tauri::command]
pub fn delete_sound_file(app: AppHandle, slot: u8) -> Result<(), String> {
    let dir = sounds_dir(&app)?;
    remove_existing_for_slot(&dir, slot)
        .map_err(|e| format!("failed to delete sound file(s) for slot {}: {}", slot, e))?;
    log_info(&format!("Deleted sound files for slot {}", slot));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_from_lowercases_and_strips_dot() {
        assert_eq!(extension_from("foo.MP3").as_deref(), Some("mp3"));
        assert_eq!(extension_from("with.spaces.ogg").as_deref(), Some("ogg"));
        assert_eq!(extension_from("no_ext"), None);
    }
}
