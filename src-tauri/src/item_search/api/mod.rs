//! Backend for the in-game Median XL item database search overlay.
//! This module calls the public item API, normalizes responses for the Svelte
//! UI, caches successful lookups, and rate-limits uncached requests.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::logger::info as log_info;

mod index;
mod response;
mod state;
mod transport;

pub use state::MxlItemApiState;

const TYPEAHEAD_MIN_QUERY_LEN: usize = 2;
const TOO_MANY_MATCHES_MESSAGE: &str = "Too many matches. Keep typing to narrow results.";
const RATE_LIMIT_MESSAGE: &str = "Search is cooling down. Try again in a few seconds.";
const SEARCH_FAILED_MESSAGE: &str = "Search failed. Check connection and try again.";
const ITEM_NOT_FOUND_MESSAGE: &str = "Item not found";
const TYPEAHEAD_MIN_QUERY_MESSAGE: &str = "Type at least 2 characters to search.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MxlItemDetail {
    pub name: String,
    pub name_display: String,
    pub quality: String,
    pub class: String,
    pub type_name: String,
    pub runeword: String,
    pub runeword_level: String,
    pub stats: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MxlItemEntry {
    pub name: String,
    pub quality: String,
    pub class: String,
    pub type_name: String,
    pub detail: Option<MxlItemDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MxlItemSearchResult {
    Results {
        query: String,
        entries: Vec<MxlItemEntry>,
        message: Option<String>,
    },
    NotFound {
        query: String,
        message: String,
    },
    RateLimited {
        message: String,
        retry_after_ms: u64,
    },
    Error {
        query: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MxlItemSearchMode {
    Detail,
    Index,
}

impl MxlItemSearchMode {
    fn from_optional(value: Option<String>) -> Self {
        match value.as_deref().map(str::trim) {
            Some(mode) if mode.eq_ignore_ascii_case("index") => Self::Index,
            _ => Self::Detail,
        }
    }
}

fn result_kind(result: &MxlItemSearchResult) -> &'static str {
    match result {
        MxlItemSearchResult::Results { entries, .. } => {
            if entries.is_empty() {
                "results(empty)"
            } else {
                "results"
            }
        }
        MxlItemSearchResult::NotFound { .. } => "notFound",
        MxlItemSearchResult::RateLimited { .. } => "rateLimited",
        MxlItemSearchResult::Error { .. } => "error",
    }
}

#[tauri::command]
pub fn search_mxl_items(
    query: String,
    mode: Option<String>,
    state: tauri::State<MxlItemApiState>,
) -> MxlItemSearchResult {
    let now = Instant::now();
    let search_mode = MxlItemSearchMode::from_optional(mode);
    if state.verbose() {
        log_info(&format!(
            "[ItemSearch] search_mxl_items query='{}' mode={:?}",
            query, search_mode
        ));
    }
    let result = match search_mode {
        MxlItemSearchMode::Detail => state.cached_or_fetch(&query, now),
        MxlItemSearchMode::Index => state.search_index(&query, now),
    };
    if state.verbose() {
        log_info(&format!(
            "[ItemSearch] search_mxl_items result for '{}': {}",
            query,
            result_kind(&result)
        ));
    }
    result
}

#[cfg(test)]
mod tests;
