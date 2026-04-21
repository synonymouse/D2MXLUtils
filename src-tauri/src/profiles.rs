//! Profile management for loot filter configurations
//!
//! Profiles are stored as plain-text `.rules` DSL files in
//! `%APPDATA%/D2MXLUtils/profiles/`. The filename stem is the profile name,
//! and the file contents are the full DSL (including any `hide default` /
//! `show default` directive). There is no intermediate JSON form.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

use crate::logger::info as log_info;

const PROFILE_EXT: &str = "rules";

const NEW_PROFILE_TEMPLATE: &str = "\
# D2MXLUtils Loot Filter
# Uncomment the next line to hide unmatched items by default:
# hide default

";

/// Profile metadata (returned when listing profiles)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// Profile name (filename without extension)
    pub name: String,
    /// Number of rules in the profile
    pub rule_count: usize,
    /// Last modified timestamp (ISO 8601)
    pub modified: Option<String>,
}

/// Get the profiles directory path
fn get_profiles_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;

    let profiles_dir = app_data.join("profiles");

    if !profiles_dir.exists() {
        fs::create_dir_all(&profiles_dir)
            .map_err(|e| format!("Failed to create profiles directory: {}", e))?;
    }

    Ok(profiles_dir)
}

/// Sanitize profile name for use as filename.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn profile_path(dir: &std::path::Path, safe_name: &str) -> PathBuf {
    dir.join(format!("{}.{}", safe_name, PROFILE_EXT))
}

fn modified_iso(entry: &fs::DirEntry) -> Option<String> {
    entry
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            chrono::DateTime::<chrono::Utc>::from(t)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        })
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// List all available profiles.
#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Result<Vec<ProfileInfo>, String> {
    let profiles_dir = get_profiles_dir(&app)?;

    let mut profiles = Vec::new();

    let entries = fs::read_dir(&profiles_dir)
        .map_err(|e| format!("Failed to read profiles directory: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map_or(false, |ext| ext == PROFILE_EXT) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        let rule_count = fs::read_to_string(&path)
            .ok()
            .and_then(|text| crate::rules::parse_dsl(&text).ok())
            .map_or(0, |cfg| cfg.rules.len());

        profiles.push(ProfileInfo {
            name: stem.to_string(),
            rule_count,
            modified: modified_iso(&entry),
        });
    }

    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    log_info(&format!("Listed {} profiles", profiles.len()));
    Ok(profiles)
}

/// Load a profile's raw DSL text by name.
#[tauri::command]
pub fn load_profile(app: AppHandle, name: String) -> Result<String, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let path = profile_path(&profiles_dir, &safe_name);

    if !path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read profile: {}", e))?;

    log_info(&format!("Loaded profile '{}'", safe_name));
    Ok(content)
}

/// Save a profile (create or overwrite).
#[tauri::command]
pub fn save_profile(
    app: AppHandle,
    name: String,
    rules_text: String,
) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);

    if safe_name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }

    // Parse once for validation; we only persist the raw text.
    let config = crate::rules::parse_dsl(&rules_text).map_err(|errors| {
        format!(
            "Failed to parse DSL: {}",
            errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let path = profile_path(&profiles_dir, &safe_name);
    fs::write(&path, &rules_text).map_err(|e| format!("Failed to write profile: {}", e))?;

    log_info(&format!(
        "Saved profile '{}' with {} rules",
        safe_name,
        config.rules.len()
    ));

    Ok(ProfileInfo {
        name: safe_name,
        rule_count: config.rules.len(),
        modified: Some(now_iso()),
    })
}

/// Delete a profile.
#[tauri::command]
pub fn delete_profile(app: AppHandle, name: String) -> Result<(), String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let path = profile_path(&profiles_dir, &safe_name);

    if !path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }

    fs::remove_file(&path).map_err(|e| format!("Failed to delete profile: {}", e))?;

    log_info(&format!("Deleted profile '{}'", safe_name));
    Ok(())
}

/// Rename a profile.
#[tauri::command]
pub fn rename_profile(
    app: AppHandle,
    old_name: String,
    new_name: String,
) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_old_name = sanitize_name(&old_name);
    let safe_new_name = sanitize_name(&new_name);

    if safe_new_name.is_empty() {
        return Err("New profile name cannot be empty".to_string());
    }

    let old_path = profile_path(&profiles_dir, &safe_old_name);
    let new_path = profile_path(&profiles_dir, &safe_new_name);

    if !old_path.exists() {
        return Err(format!("Profile '{}' not found", old_name));
    }
    if new_path.exists() {
        return Err(format!("Profile '{}' already exists", new_name));
    }

    fs::rename(&old_path, &new_path).map_err(|e| format!("Failed to rename profile: {}", e))?;

    let rule_count = fs::read_to_string(&new_path)
        .ok()
        .and_then(|text| crate::rules::parse_dsl(&text).ok())
        .map_or(0, |cfg| cfg.rules.len());

    log_info(&format!(
        "Renamed profile '{}' to '{}'",
        safe_old_name, safe_new_name
    ));

    Ok(ProfileInfo {
        name: safe_new_name,
        rule_count,
        modified: Some(now_iso()),
    })
}

/// Duplicate a profile.
#[tauri::command]
pub fn duplicate_profile(
    app: AppHandle,
    name: String,
    new_name: String,
) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let safe_new_name = sanitize_name(&new_name);

    if safe_new_name.is_empty() {
        return Err("New profile name cannot be empty".to_string());
    }

    let source_path = profile_path(&profiles_dir, &safe_name);
    let dest_path = profile_path(&profiles_dir, &safe_new_name);

    if !source_path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }
    if dest_path.exists() {
        return Err(format!("Profile '{}' already exists", new_name));
    }

    fs::copy(&source_path, &dest_path)
        .map_err(|e| format!("Failed to duplicate profile: {}", e))?;

    let rule_count = fs::read_to_string(&dest_path)
        .ok()
        .and_then(|text| crate::rules::parse_dsl(&text).ok())
        .map_or(0, |cfg| cfg.rules.len());

    log_info(&format!(
        "Duplicated profile '{}' to '{}'",
        safe_name, safe_new_name
    ));

    Ok(ProfileInfo {
        name: safe_new_name,
        rule_count,
        modified: Some(now_iso()),
    })
}

/// Create a new empty profile with a starter template.
#[tauri::command]
pub fn create_profile(app: AppHandle, name: String) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);

    if safe_name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }

    let path = profile_path(&profiles_dir, &safe_name);
    if path.exists() {
        return Err(format!("Profile '{}' already exists", name));
    }

    fs::write(&path, NEW_PROFILE_TEMPLATE)
        .map_err(|e| format!("Failed to write profile: {}", e))?;

    log_info(&format!("Created new profile '{}'", safe_name));

    Ok(ProfileInfo {
        name: safe_name,
        rule_count: 0,
        modified: Some(now_iso()),
    })
}
