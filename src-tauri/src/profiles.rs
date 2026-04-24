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

const NEW_PROFILE_TEMPLATE: &str = r#"# ==================== TRASH HIDES ====================

"Gold" hide

# Low-tier base items (pre-sacred)
1 2 3 4 low normal magic rare hide

# hide sacred non eth
sacred normal superior magic rare hide

# show sacred eth
sacred normal superior eth notify

# ==================== ANNOUNCEMENTS ====================

# Jewelry
"Ring$|Amulet$|Jewel|Quiver" rare notify

# Uniques and sets
unique notify
set notify map

# Sacred uniques
sacred unique notify map

# angelics
angelic notify

# Mastercrafted items
master show notify map purple sound1

# Runes
#"^(El|Eld|Tir|Nef|Eth|Ith|Tal|Ral|Ort|Thul|Amn|Sol|Shael|Dol|Hel|Io|Lum|Ko|Fal|Lem|Pul|Um|Mal|Ist|Gul|Vex|Ohm|Lo|Sur|Ber|Jah|Cham|Zod) Rune$"
"^(Ber|Jah|Cham|Zod) Rune$" notify

[notify map sound3] {
  "Great Rune"
  "Enchanted Rune"
  "Elemental Rune"
}

# Consumables
[notify] {
  "Mystic Orb"
  "Arcane (Shard|Crystal|Cluster)"
  "Heavenly|Crate"
  "Shrine \(10"
  "Vessel"
}

"Container" purple notify map
"Runestone|Essence$" red notify map

# Essences, relics, arcane materials, enchant scrolls
[notify map sound2] {
  "Essence"
  "Corrupted (Shard|Crystal|Cluster)"
  "Enchant Scroll"
}

# Reagents
[notify] {
  "Enchanting"
  "Mystic Dye"
  "Treasure"
  "Item Design"
}

# Oils and special consumables
[notify map sound2] {
  "Oil of Augmentation"
  "Oil of Conjuration"
  "Oil of Greater Luck"
  "Oil of Intensity"
  "Belladonna Extract"
  "Heavenly Soul"
}

# Quest items
[notify map] {
  "Ring of the Five"
  "Sigil$"
  "Tome of Possession"
  "Tenet"
  "Book of Cain"
  "Positronic Brain"
}

"Riftstone" red notify map
"Relic" red notify map sound3

# Trophies, effigies, emblems
[notify map] {
  "Trophy"
  "Occult Effigy"
  "Emblem"
}

# Cycles
[notify] {
  "Cycle"
  "Medium Cycle" sound1
  "Large Cycle" sound2
  "Golden Cycle" red sound3 map
}

# Signets (Attributes / Learning / Skill)
"^Signet of" show notify map orange sound2

# Charms
[stat green notify map] {
  "Zakarum's Ear|Visions of Akarat|Bone Chimes|Spirit Trance Herb|Soul of Kabraxis|Fool's Gold"
  "Sunstone of the Twin Seas|The Butcher's Tooth|Optical Detector|Laser Focus Crystal|Scroll of Kings|Moon of the Spider|Horazon's Focus|Six Angel Bag"
  "Sacred Worldstone Key|The Black Road|Azmodan's Heart|Hammer of the Taan Judges|Sunstone of the Gods|Spirit of Creation|Idol of Vanity|Silver Seal of Ureh"
  "Crystalline Flame Medallion|Legacy of Blood|Weather Control|Demonsbane|Umbaru Treasure|Xazax's Illusion|The Ancient Repositories|The Sleep|Dragon Claw|Neutrality Pact"
  "Eternal Bone Pile|Corrupted Wormhole|Cold Fusion Schematics|Lylia's Curse|Astrogha's Venom Stinger|The Glorious Book of Median|Books of Kalan|Vial of Elder Blood"
}

# Stat group examples
# [rare angelic stat notify] {
#   {focus} {enemy fire}
#   {focus} {speeds}
#   {speeds} {enemy fire}
#   "Light Plated" {speeds} {focus}
#   {Frozen Soul}
# }
#
# "Amulet" rare {[3-9] to All Skills} {focus} {enemy fire} stat notify
# "Arrow Quiver" {druid} rare stat notify
"#;

/// Default profile name seeded on first run.
pub const DEFAULT_PROFILE_NAME: &str = "Default-starter";

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

pub fn seed_default_profile(app: &AppHandle) -> Result<String, String> {
    let profiles_dir = get_profiles_dir(app)?;
    let path = profile_path(&profiles_dir, DEFAULT_PROFILE_NAME);

    if path.exists() {
        return Ok(DEFAULT_PROFILE_NAME.to_string());
    }

    fs::write(&path, NEW_PROFILE_TEMPLATE)
        .map_err(|e| format!("Failed to write default profile: {}", e))?;

    log_info(&format!(
        "Seeded default profile '{}'",
        DEFAULT_PROFILE_NAME
    ));

    Ok(DEFAULT_PROFILE_NAME.to_string())
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
