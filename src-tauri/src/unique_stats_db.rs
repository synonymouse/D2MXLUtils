//! Local database of unique/set item stat *templates* (with roll ranges,
//! e.g. "+(6 to 10) to all Attributes"), built offline by
//! `scripts/generate-unique-stats-db.mjs` (crawls the public MXL item API
//! the search overlay already uses — see `mxl_item_api.rs`) and loaded
//! here from disk. This module itself makes no network access at runtime —
//! `unique_stats_db_sync.rs` is what keeps `app_data_dir/unique-stats-db.json`
//! populated, downloading a maintainer-built copy from GitHub instead of
//! every client crawling the API themselves.
//!
//! Correlating a dropped item back to its DB entry is best-effort: our own
//! `name` field has our own "TU"/"SU"/"SSU"/"SSSU" suffix appended
//! (`notifier.rs`'s `unique_kind` labeling), while the API keys entries by
//! a parenthetical tier suffix instead (e.g. "Akara's Robe (Sacred)",
//! "Akara's Robe (1)"). `expected_db_name` guesses the latter from the
//! former plus the item's resolved `ItemTier`; if the guess doesn't hit,
//! stats are returned unannotated rather than guessing wrong.

use std::collections::HashMap;
use std::fs;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::logger::{error as log_error, info as log_info};
use crate::rules::ItemTier;

const DB_FILE: &str = "unique-stats-db.json";

#[derive(Debug, Deserialize)]
struct UniqueStatsEntry {
    name: String,
    stats: String,
}

#[derive(Debug, Deserialize)]
struct UniqueStatsDbFile {
    entries: Vec<UniqueStatsEntry>,
}

/// name -> template stats text (verbatim from the API, ranges included).
pub type UniqueStatsDb = HashMap<String, String>;

pub fn load_unique_stats_db(app: &AppHandle) -> Option<UniqueStatsDb> {
    let app_data = app.path().app_data_dir().ok()?;
    let path = app_data.join(DB_FILE);
    if !path.exists() {
        log_info(&format!("unique stats db: no file at {}", path.display()));
        return None;
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("unique stats db: read failed: {}", e));
            return None;
        }
    };
    match serde_json::from_str::<UniqueStatsDbFile>(&content) {
        Ok(file) => {
            let map: UniqueStatsDb = file
                .entries
                .into_iter()
                .map(|e| (e.name, e.stats))
                .collect();
            log_info(&format!("unique stats db: loaded {} entries", map.len()));
            Some(map)
        }
        Err(e) => {
            log_error(&format!("unique stats db: parse failed: {}", e));
            None
        }
    }
}

/// Strips a trailing " TU"/" SU"/" SSU"/" SSSU" kind label (see
/// `notifier.rs`'s `unique_kind` appending) to recover the bare display
/// name the DB keys off of.
fn strip_kind_label(name: &str) -> &str {
    for label in ["SSSU", "SSU", "SU", "TU"] {
        if let Some(stripped) = name.strip_suffix(label) {
            if let Some(bare) = stripped.strip_suffix(' ') {
                return bare;
            }
        }
    }
    name
}

/// Guess the API's parenthetical tier suffix for a given `ItemTier`. Not
/// verified against every case — a miss just means no range gets shown,
/// not a wrong one.
fn guessed_suffixes(tier: Option<ItemTier>) -> &'static [&'static str] {
    match tier {
        Some(ItemTier::Sacred) => &["(Sacred)"],
        Some(ItemTier::Angelic) => &["(Angelic)"],
        Some(ItemTier::Master) => &["(Master)", "(Mastercrafted)"],
        Some(ItemTier::Tier1) => &["(1)"],
        Some(ItemTier::Tier2) => &["(2)"],
        Some(ItemTier::Tier3) => &["(3)"],
        Some(ItemTier::Tier4) => &["(4)"],
        Some(ItemTier::Tier0) | None => &[],
    }
}

/// Given the item's resolved display `name` (our own, kind-labeled form)
/// and `tier`, find the matching DB entry, trying the bare name first
/// (covers items with only one variant) then each guessed tier suffix.
fn lookup<'a>(db: &'a UniqueStatsDb, name: &str, tier: Option<ItemTier>) -> Option<&'a str> {
    let bare = strip_kind_label(name);
    if let Some(stats) = db.get(bare) {
        return Some(stats.as_str());
    }
    for suffix in guessed_suffixes(tier) {
        if let Some(stats) = db.get(&format!("{} {}", bare, suffix)) {
            return Some(stats.as_str());
        }
    }
    None
}

/// Appends the possible roll range from the template DB to each actual
/// stat line whose text matches a ranged template line — e.g.
/// "+6 to all Attributes" becomes "+6 to all Attributes (6-10)". Lines
/// with no DB match, or whose template entry has a flat (non-ranged)
/// value, are left unchanged. Returns `actual_stats` verbatim if `name`
/// has no DB entry at all.
pub fn annotate_with_roll_ranges(
    db: &UniqueStatsDb,
    name: &str,
    tier: Option<ItemTier>,
    actual_stats: &str,
) -> String {
    let Some(template) = lookup(db, name, tier) else {
        return actual_stats.to_string();
    };
    let template_lines: Vec<&str> = template.lines().collect();

    actual_stats
        .lines()
        .map(|line| {
            let normalized = normalize_stat_line(line);
            template_lines
                .iter()
                .find_map(|tline| {
                    let (t_normalized, range) = extract_range(tline);
                    let (min, max) = range?;
                    (t_normalized == normalized).then(|| format!("{} ({}-{})", line, min, max))
                })
                .unwrap_or_else(|| line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strips digits and any "(...)" content from a stat line, so "+6 to all
/// Attributes" and "+(6 to 10) to all Attributes" reduce to the same
/// shape for comparison.
fn normalize_stat_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_parens = false;
    for c in line.chars() {
        match c {
            '(' => in_parens = true,
            ')' => in_parens = false,
            _ if in_parens => {}
            c if c.is_ascii_digit() => {}
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extracts "(min to max)" from a template line, if present with `min !=
/// max` (a flat/non-ranged value has nothing meaningful to display).
/// Returns the line's normalized shape (for comparison) alongside it.
fn extract_range(line: &str) -> (String, Option<(u32, u32)>) {
    let normalized = normalize_stat_line(line);
    let range = (|| {
        let open = line.find('(')?;
        let close = line[open..].find(')')? + open;
        let inner = &line[open + 1..close];
        let mut parts = inner.split("to").map(str::trim);
        let min: u32 = parts.next()?.parse().ok()?;
        let max: u32 = parts.next()?.parse().ok()?;
        (min != max).then_some((min, max))
    })();
    (normalized, range)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(entries: &[(&str, &str)]) -> UniqueStatsDb {
        entries
            .iter()
            .map(|(n, s)| (n.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn appends_range_for_matching_line() {
        let db = db(&[(
            "Akara's Robe (1)",
            "+50 Defense\n+(6 to 10) to all Attributes\n+50 to Life\nElemental Resists +(11 to 15)%",
        )]);
        let actual = "+50 Defense\n+6 to all Attributes\n+50 to Life\nElemental Resists +13%";
        let result = annotate_with_roll_ranges(&db, "Akara's Robe (1)", None, actual);
        assert_eq!(
            result,
            "+50 Defense\n+6 to all Attributes (6-10)\n+50 to Life\nElemental Resists +13% (11-15)"
        );
    }

    #[test]
    fn strips_kind_label_and_uses_tier_suffix() {
        let db = db(&[("Akara's Robe (Sacred)", "+(31 to 50) to all Attributes")]);
        let actual = "+40 to all Attributes";
        let result =
            annotate_with_roll_ranges(&db, "Akara's Robe SSSU", Some(ItemTier::Sacred), actual);
        assert_eq!(result, "+40 to all Attributes (31-50)");
    }

    #[test]
    fn unknown_name_returns_input_unchanged() {
        let db = db(&[]);
        let actual = "+6 to all Attributes";
        assert_eq!(annotate_with_roll_ranges(&db, "Nope", None, actual), actual);
    }

    #[test]
    fn flat_template_value_is_not_annotated() {
        let db = db(&[("X", "+50 Defense")]);
        let actual = "+50 Defense";
        assert_eq!(annotate_with_roll_ranges(&db, "X", None, actual), actual);
    }

    #[test]
    fn line_with_no_matching_template_line_is_left_unchanged() {
        let db = db(&[("X", "+(6 to 10) to all Attributes")]);
        let actual = "Socketed (2)\n+6 to all Attributes";
        assert_eq!(
            annotate_with_roll_ranges(&db, "X", None, actual),
            "Socketed (2)\n+6 to all Attributes (6-10)"
        );
    }
}
