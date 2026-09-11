use self_update::backends::github::ReleaseList;

use crate::logger::info as log_info;

use super::UpdateCheckResult;

const REPO_OWNER: &str = "synonymouse";
const REPO_NAME: &str = "D2MXLUtils";
#[cfg(target_os = "windows")]
const ASSET_NAME: &str = "d2mxlutils.exe";

pub(super) fn check_inner() -> Result<UpdateCheckResult, String> {
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
                        // release.yml uploads "d2mxlutils.AppImage", while
                        // older releases use Tauri's versioned name. Match
                        // the extension case-insensitively to cover both.
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
