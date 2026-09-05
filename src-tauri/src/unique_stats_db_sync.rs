//! Keeps `app_data_dir/unique-stats-db.json` (see `unique_stats_db.rs`) in
//! sync with a maintainer-built copy published on GitHub, instead of every
//! client crawling the third-party MXL item API themselves via
//! `scripts/generate-unique-stats-db.mjs` (a ~15 minute paced crawl against
//! a community-run site — fine for one maintainer to run occasionally, not
//! something every install should repeat).
//!
//! The build side is `pnpm unique-stats-db:publish`
//! (`scripts/publish-unique-stats-db.mjs`), run locally by the maintainer
//! whenever they want to refresh it — not tied to app version releases,
//! since the underlying item data only changes when MXL itself patches.
//! Deliberately NOT a CI workflow: the target API blocks GitHub Actions'
//! runner IPs outright (confirmed live — 403 from Actions, 200 for the
//! identical request from a normal dev IP). It publishes to a GitHub
//! release with a fixed tag (`unique-stats-db`, never a version string),
//! so `updater.rs`'s own release scan skips right over it (it fails that
//! code's `semver::Version::parse` filter, same as any non-version tag).
//!
//! Applying a freshly downloaded file doesn't happen live: the scanner
//! loads `unique_stats_db.rs`'s DB once into a plain (non-`RwLock`) field
//! at startup (see `SharedScannerState`), so a download completed mid-
//! session only takes effect on the next launch — acceptable given how
//! infrequently this data actually changes.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};

const REPO_OWNER: &str = "pertinate";
const REPO_NAME: &str = "D2MXLUtils";
const RELEASE_TAG: &str = "unique-stats-db";
const ASSET_NAME: &str = "unique-stats-db.json";
const DB_FILE: &str = "unique-stats-db.json";
const META_FILE: &str = "unique-stats-db.meta.json";

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

/// Sidecar next to the DB file itself — records which published version is
/// currently on disk, so `check` doesn't need to hash/compare the (fairly
/// large) DB file's own contents.
#[derive(Debug, Serialize, Deserialize)]
struct LocalMeta {
    asset_updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UniqueStatsDbCheckResult {
    pub status: &'static str, // "not_downloaded" | "up_to_date" | "available"
    pub asset_updated_at: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .build()
        .into()
}

fn fetch_release_asset() -> Result<GithubAsset, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/tags/{}",
        REPO_OWNER, REPO_NAME, RELEASE_TAG
    );
    let response = agent()
        .get(&url)
        // GitHub's REST API 403s an unauthenticated request with no
        // User-Agent at all; identify as the app, same spirit as the
        // ACCEPT header updater.rs sets on its own GitHub calls.
        .header("User-Agent", "D2MXLUtils")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("fetch release metadata: {}", e))?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("read release metadata body: {}", e))?;
    let release: GithubRelease =
        serde_json::from_str(&body).map_err(|e| format!("parse release metadata: {}", e))?;
    release
        .assets
        .into_iter()
        .find(|a| a.name == ASSET_NAME)
        .ok_or_else(|| {
            format!(
                "asset '{}' missing from '{}' release",
                ASSET_NAME, RELEASE_TAG
            )
        })
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {}", e))?;
    Ok(dir.join(DB_FILE))
}

fn meta_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {}", e))?;
    Ok(dir.join(META_FILE))
}

fn read_local_meta(app: &AppHandle) -> Option<LocalMeta> {
    let path = meta_path(app).ok()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

#[tauri::command]
pub async fn check_unique_stats_db_update(
    app: AppHandle,
) -> Result<UniqueStatsDbCheckResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        log_info("unique stats db: checking for update");
        let asset = fetch_release_asset()?;

        let db_exists = db_path(&app).map(|p| p.exists()).unwrap_or(false);
        if !db_exists {
            return Ok(UniqueStatsDbCheckResult {
                status: "not_downloaded",
                asset_updated_at: Some(asset.updated_at),
            });
        }

        let up_to_date = read_local_meta(&app)
            .map(|m| m.asset_updated_at == asset.updated_at)
            .unwrap_or(false);

        Ok(UniqueStatsDbCheckResult {
            status: if up_to_date {
                "up_to_date"
            } else {
                "available"
            },
            asset_updated_at: Some(asset.updated_at),
        })
    })
    .await
    .map_err(|e| format!("spawn_blocking join: {}", e))?
}

#[tauri::command]
pub async fn download_unique_stats_db(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        log_info("unique stats db: downloading update");
        let asset = fetch_release_asset()?;

        let response = agent()
            .get(&asset.browser_download_url)
            .header("User-Agent", "D2MXLUtils")
            .call()
            .map_err(|e| format!("download asset: {}", e))?;
        let body = response
            .into_body()
            .read_to_string()
            .map_err(|e| format!("read asset body: {}", e))?;

        // Sanity-check it actually parses as the expected shape before
        // overwriting the working local copy with it.
        if serde_json::from_str::<serde_json::Value>(&body).is_err() {
            return Err("downloaded file is not valid JSON".to_string());
        }

        let db_path = db_path(&app)?;
        if let Some(dir) = db_path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("create app data dir: {}", e))?;
        }
        fs::write(&db_path, &body).map_err(|e| format!("write {}: {}", db_path.display(), e))?;

        let meta = LocalMeta {
            asset_updated_at: asset.updated_at,
        };
        let meta_json =
            serde_json::to_string(&meta).map_err(|e| format!("serialize meta: {}", e))?;
        fs::write(meta_path(&app)?, meta_json).map_err(|e| format!("write meta: {}", e))?;

        log_info(&format!(
            "unique stats db: downloaded to {}",
            db_path.display()
        ));
        Ok(())
    })
    .await
    .map_err(|e| format!("spawn_blocking join: {}", e))?
    .inspect_err(|e| log_error(&format!("unique stats db: download failed: {}", e)))
}
