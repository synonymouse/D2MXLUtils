//! Profile management for loot filter configurations
//!
//! Profiles are stored as JSON files in `%APPDATA%/D2MXLUtils/profiles/`
//! Each profile contains a FilterConfig with rules in DSL format.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

use crate::logger::{error as log_error, info as log_info};
use crate::rules::FilterConfig;

/// Profile metadata (returned when listing profiles)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// Profile name (filename without .json extension)
    pub name: String,
    /// Number of rules in the profile
    pub rule_count: usize,
    /// Last modified timestamp (ISO 8601)
    pub modified: Option<String>,
}

/// Full profile data (returned when loading a profile)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Profile name
    pub name: String,
    /// Filter configuration with rules
    pub config: FilterConfig,
    /// DSL source text (for editor)
    pub dsl_source: String,
}

/// Get the profiles directory path
fn get_profiles_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;
    
    let profiles_dir = app_data.join("profiles");
    
    // Ensure directory exists
    if !profiles_dir.exists() {
        fs::create_dir_all(&profiles_dir)
            .map_err(|e| format!("Failed to create profiles directory: {}", e))?;
    }
    
    Ok(profiles_dir)
}

/// Sanitize profile name for use as filename
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

/// List all available profiles
#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Result<Vec<ProfileInfo>, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    
    let mut profiles = Vec::new();
    
    let entries = fs::read_dir(&profiles_dir)
        .map_err(|e| format!("Failed to read profiles directory: {}", e))?;
    
    for entry in entries.flatten() {
        let path = entry.path();
        
        // Only process .json files
        if path.extension().map_or(false, |ext| ext == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                // Try to read and parse the profile to get rule count
                let rule_count = match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<FilterConfig>(&content) {
                            Ok(config) => config.rules.len(),
                            Err(_) => 0,
                        }
                    }
                    Err(_) => 0,
                };
                
                // Get modified time
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        chrono::DateTime::<chrono::Utc>::from(t)
                            .format("%Y-%m-%dT%H:%M:%SZ")
                            .to_string()
                    });
                
                profiles.push(ProfileInfo {
                    name: stem.to_string(),
                    rule_count,
                    modified,
                });
            }
        }
    }
    
    // Sort by name
    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    
    log_info(&format!("Listed {} profiles", profiles.len()));
    Ok(profiles)
}

/// Load a profile by name
#[tauri::command]
pub fn load_profile(app: AppHandle, name: String) -> Result<Profile, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let path = profiles_dir.join(format!("{}.json", safe_name));
    
    if !path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }
    
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read profile: {}", e))?;
    
    let config: FilterConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse profile: {}", e))?;
    
    // Convert config to DSL for editor
    let dsl_source = crate::rules::to_dsl(&config);
    
    log_info(&format!("Loaded profile '{}' with {} rules", name, config.rules.len()));
    
    Ok(Profile {
        name: safe_name,
        config,
        dsl_source,
    })
}

/// Save a profile (create or update)
#[tauri::command]
pub fn save_profile(app: AppHandle, name: String, dsl_source: String) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    
    if safe_name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    
    let path = profiles_dir.join(format!("{}.json", safe_name));
    
    // Parse DSL to FilterConfig
    let mut config = crate::rules::parse_dsl(&dsl_source)
        .map_err(|errors| {
            format!(
                "Failed to parse DSL: {}",
                errors.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join(", ")
            )
        })?;
    
    // Set profile name
    config.name = safe_name.clone();
    config.dsl_source = Some(dsl_source);
    
    // Serialize and save
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize profile: {}", e))?;
    
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write profile: {}", e))?;
    
    log_info(&format!("Saved profile '{}' with {} rules", safe_name, config.rules.len()));
    
    Ok(ProfileInfo {
        name: safe_name,
        rule_count: config.rules.len(),
        modified: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    })
}

/// Delete a profile
#[tauri::command]
pub fn delete_profile(app: AppHandle, name: String) -> Result<(), String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let path = profiles_dir.join(format!("{}.json", safe_name));
    
    if !path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }
    
    fs::remove_file(&path)
        .map_err(|e| format!("Failed to delete profile: {}", e))?;
    
    log_info(&format!("Deleted profile '{}'", safe_name));
    Ok(())
}

/// Rename a profile
#[tauri::command]
pub fn rename_profile(app: AppHandle, old_name: String, new_name: String) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_old_name = sanitize_name(&old_name);
    let safe_new_name = sanitize_name(&new_name);
    
    if safe_new_name.is_empty() {
        return Err("New profile name cannot be empty".to_string());
    }
    
    let old_path = profiles_dir.join(format!("{}.json", safe_old_name));
    let new_path = profiles_dir.join(format!("{}.json", safe_new_name));
    
    if !old_path.exists() {
        return Err(format!("Profile '{}' not found", old_name));
    }
    
    if new_path.exists() {
        return Err(format!("Profile '{}' already exists", new_name));
    }
    
    // Read, update name, write to new location, delete old
    let content = fs::read_to_string(&old_path)
        .map_err(|e| format!("Failed to read profile: {}", e))?;
    
    let mut config: FilterConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse profile: {}", e))?;
    
    config.name = safe_new_name.clone();
    
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize profile: {}", e))?;
    
    fs::write(&new_path, json)
        .map_err(|e| format!("Failed to write profile: {}", e))?;
    
    fs::remove_file(&old_path)
        .map_err(|e| format!("Failed to delete old profile: {}", e))?;
    
    log_info(&format!("Renamed profile '{}' to '{}'", safe_old_name, safe_new_name));
    
    Ok(ProfileInfo {
        name: safe_new_name,
        rule_count: config.rules.len(),
        modified: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    })
}

/// Duplicate a profile
#[tauri::command]
pub fn duplicate_profile(app: AppHandle, name: String, new_name: String) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    let safe_new_name = sanitize_name(&new_name);
    
    if safe_new_name.is_empty() {
        return Err("New profile name cannot be empty".to_string());
    }
    
    let source_path = profiles_dir.join(format!("{}.json", safe_name));
    let dest_path = profiles_dir.join(format!("{}.json", safe_new_name));
    
    if !source_path.exists() {
        return Err(format!("Profile '{}' not found", name));
    }
    
    if dest_path.exists() {
        return Err(format!("Profile '{}' already exists", new_name));
    }
    
    // Read, update name, write to new location
    let content = fs::read_to_string(&source_path)
        .map_err(|e| format!("Failed to read profile: {}", e))?;
    
    let mut config: FilterConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse profile: {}", e))?;
    
    config.name = safe_new_name.clone();
    
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize profile: {}", e))?;
    
    fs::write(&dest_path, json)
        .map_err(|e| format!("Failed to write profile: {}", e))?;
    
    log_info(&format!("Duplicated profile '{}' to '{}'", safe_name, safe_new_name));
    
    Ok(ProfileInfo {
        name: safe_new_name,
        rule_count: config.rules.len(),
        modified: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    })
}

/// Create a new empty profile
#[tauri::command]
pub fn create_profile(app: AppHandle, name: String) -> Result<ProfileInfo, String> {
    let profiles_dir = get_profiles_dir(&app)?;
    let safe_name = sanitize_name(&name);
    
    if safe_name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    
    let path = profiles_dir.join(format!("{}.json", safe_name));
    
    if path.exists() {
        return Err(format!("Profile '{}' already exists", name));
    }
    
    // Create default config with example rules
    let config = FilterConfig {
        name: safe_name.clone(),
        default_show_items: true,
        default_notify: false,
        rules: Vec::new(),
        dsl_source: Some(format!(
            "# {} Loot Filter\n# Add your rules below\n\n",
            safe_name
        )),
    };
    
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize profile: {}", e))?;
    
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write profile: {}", e))?;
    
    log_info(&format!("Created new profile '{}'", safe_name));
    
    Ok(ProfileInfo {
        name: safe_name,
        rule_count: 0,
        modified: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    })
}

