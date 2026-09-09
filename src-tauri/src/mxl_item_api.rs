//! Backend for the in-game Median XL item database search overlay.
//! This module calls the public item API, normalizes responses for the Svelte
//! UI, caches successful lookups, and rate-limits uncached requests.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::logger::info as log_info;

const API_URL: &str = "https://tsw.vn.cz/stats/api_item.php";
const RATE_LIMIT_COUNT: usize = 10;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);
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

#[derive(Debug, Deserialize)]
struct RawApiResponse {
    ok: Option<bool>,
    query: Option<String>,
    count: Option<usize>,
    truncated: Option<bool>,
    items: Option<Vec<RawItem>>,
    matches: Option<Vec<RawMatch>>,
    message: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    name: String,
    #[serde(default)]
    name_display: String,
    quality: String,
    class: String,
    #[serde(rename = "type", default)]
    type_name: String,
    #[serde(default)]
    runeword: String,
    #[serde(default)]
    runeword_level: String,
    #[serde(default)]
    stats: String,
}

#[derive(Debug, Deserialize)]
struct RawMatch {
    name: String,
    quality: String,
    class: String,
    #[serde(rename = "type", default)]
    type_name: String,
}

#[derive(Debug, Deserialize)]
struct RawIndexResponse {
    count: usize,
    items: Vec<RawIndexItem>,
}

#[derive(Debug, Deserialize)]
struct RawIndexItem {
    name: String,
    quality: String,
    class: String,
    #[serde(rename = "type", default)]
    type_name: String,
}

pub struct MxlItemApiState {
    cache: Mutex<HashMap<String, MxlItemSearchResult>>,
    index_cache: Mutex<Option<Vec<MxlItemEntry>>>,
    index_load_lock: Mutex<()>,
    limiter: Mutex<RequestWindow>,
    agent: ureq::Agent,
    verbose_logging: Arc<AtomicBool>,
}

impl MxlItemApiState {
    pub fn new(verbose_logging: Arc<AtomicBool>) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            index_cache: Mutex::new(None),
            index_load_lock: Mutex::new(()),
            limiter: Mutex::new(RequestWindow::default()),
            agent: default_agent(),
            verbose_logging,
        }
    }

    fn verbose(&self) -> bool {
        self.verbose_logging.load(Ordering::SeqCst)
    }
}

impl Default for MxlItemApiState {
    fn default() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)))
    }
}

#[derive(Default)]
struct RequestWindow {
    requests: VecDeque<Instant>,
}

impl RequestWindow {
    fn check(&mut self, now: Instant) -> Result<(), u64> {
        while let Some(front) = self.requests.front().copied() {
            if now.duration_since(front) >= RATE_LIMIT_WINDOW {
                self.requests.pop_front();
            } else {
                break;
            }
        }

        if self.requests.len() < RATE_LIMIT_COUNT {
            self.requests.push_back(now);
            return Ok(());
        }

        let oldest = self.requests.front().copied().unwrap_or(now);
        let retry_after = RATE_LIMIT_WINDOW.saturating_sub(now.duration_since(oldest));
        Err(retry_after.as_millis() as u64)
    }
}

fn normalize_query(query: &str) -> String {
    query.trim().to_lowercase()
}

fn normalized_contains(value: &str, query: &str) -> bool {
    value.to_lowercase().contains(query)
}

fn index_match_rank(entry: &MxlItemEntry, query: &str) -> Option<u8> {
    let name = entry.name.to_lowercase();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.contains(query) {
        return Some(2);
    }
    if normalized_contains(&entry.class, query)
        || normalized_contains(&entry.type_name, query)
        || normalized_contains(&entry.quality, query)
    {
        return Some(3);
    }
    None
}

fn search_index_entries(query: &str, entries: &[MxlItemEntry]) -> MxlItemSearchResult {
    let trimmed = query.trim();
    let normalized = normalize_query(trimmed);

    if normalized.len() < TYPEAHEAD_MIN_QUERY_LEN {
        return MxlItemSearchResult::Results {
            query: trimmed.to_string(),
            entries: Vec::new(),
            message: Some(TYPEAHEAD_MIN_QUERY_MESSAGE.to_string()),
        };
    }

    let mut matches = entries
        .iter()
        .filter_map(|entry| index_match_rank(entry, &normalized).map(|rank| (rank, entry)))
        .collect::<Vec<_>>();

    matches.sort_by_cached_key(|(rank, entry)| (*rank, entry.name.to_lowercase()));

    let entries = matches
        .into_iter()
        .map(|(_, entry)| entry.clone())
        .collect::<Vec<_>>();

    MxlItemSearchResult::Results {
        query: trimmed.to_string(),
        entries,
        message: None,
    }
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn default_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_recv_body(Some(Duration::from_secs(5)))
        .build()
        .into()
}

fn result_is_cacheable(result: &MxlItemSearchResult) -> bool {
    !matches!(
        result,
        MxlItemSearchResult::RateLimited { .. } | MxlItemSearchResult::Error { .. }
    )
}

fn fetch_from_api(agent: &ureq::Agent, trimmed: &str) -> MxlItemSearchResult {
    let url = format!("{}?q={}", API_URL, percent_encode_query(trimmed));
    let body = match agent.get(&url).call() {
        Ok(response) => match response.into_body().read_to_string() {
            Ok(body) => body,
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: trimmed.to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        },
        Err(err) => {
            let text = err.to_string();
            if text.contains("404") {
                return MxlItemSearchResult::NotFound {
                    query: trimmed.to_string(),
                    message: ITEM_NOT_FOUND_MESSAGE.to_string(),
                };
            }
            return MxlItemSearchResult::Error {
                query: trimmed.to_string(),
                message: SEARCH_FAILED_MESSAGE.to_string(),
            };
        }
    };

    parse_api_response(trimmed, &body)
}

fn fetch_index_from_api(agent: &ureq::Agent) -> Result<Vec<MxlItemEntry>, String> {
    let url = format!("{}?mode=index", API_URL);
    let body = match agent.get(&url).call() {
        Ok(response) => response
            .into_body()
            .read_to_string()
            .map_err(|_| SEARCH_FAILED_MESSAGE.to_string())?,
        Err(_) => return Err(SEARCH_FAILED_MESSAGE.to_string()),
    };

    parse_index_response(&body)
}

impl MxlItemApiState {
    fn cached_or_fetch(&self, query: &str, now: Instant) -> MxlItemSearchResult {
        let verbose = self.verbose();
        let trimmed = query.trim();
        if trimmed.is_empty() {
            if verbose {
                log_info("[ItemSearch] detail query is empty, skipping");
            }
            return MxlItemSearchResult::Error {
                query: String::new(),
                message: String::new(),
            };
        }

        let key = normalize_query(trimmed);
        match self.cache.lock() {
            Ok(cache) => {
                if let Some(cached) = cache.get(&key) {
                    if verbose {
                        log_info(&format!("[ItemSearch] detail cache hit for '{}'", trimmed));
                    }
                    return cached.clone();
                }
            }
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: trimmed.to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        }

        match self.limiter.lock() {
            Ok(mut limiter) => {
                if let Err(retry_after_ms) = limiter.check(now) {
                    if verbose {
                        log_info(&format!(
                            "[ItemSearch] rate limited detail query '{}', retry_after_ms={}",
                            trimmed, retry_after_ms
                        ));
                    }
                    return MxlItemSearchResult::RateLimited {
                        message: RATE_LIMIT_MESSAGE.to_string(),
                        retry_after_ms,
                    };
                }
            }
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: trimmed.to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        }

        if verbose {
            log_info(&format!(
                "[ItemSearch] fetching detail from API for '{}'",
                trimmed
            ));
        }
        let result = fetch_from_api(&self.agent, trimmed);
        if verbose {
            log_info(&format!(
                "[ItemSearch] detail fetch for '{}' -> {}",
                trimmed,
                result_kind(&result)
            ));
        }
        if result_is_cacheable(&result) {
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(key, result.clone());
            }
        }
        result
    }

    fn search_index(&self, query: &str, now: Instant) -> MxlItemSearchResult {
        let verbose = self.verbose();
        if normalize_query(query).len() < TYPEAHEAD_MIN_QUERY_LEN {
            if verbose {
                log_info(&format!(
                    "[ItemSearch] typeahead query '{}' below minimum length",
                    query.trim()
                ));
            }
            return search_index_entries(query, &[]);
        }

        match self.index_cache.lock() {
            Ok(cache) => {
                if let Some(entries) = cache.as_ref() {
                    let result = search_index_entries(query, entries);
                    if verbose {
                        log_info(&format!(
                            "[ItemSearch] typeahead '{}' against cached index ({} entries) -> {}",
                            query.trim(),
                            entries.len(),
                            result_kind(&result)
                        ));
                    }
                    return result;
                }
            }
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: query.trim().to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        }

        if verbose {
            log_info("[ItemSearch] index cache empty, loading from API");
        }

        let _load_guard = match self.index_load_lock.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: query.trim().to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        };

        match self.index_cache.lock() {
            Ok(cache) => {
                if let Some(entries) = cache.as_ref() {
                    return search_index_entries(query, entries);
                }
            }
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: query.trim().to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        }

        match self.limiter.lock() {
            Ok(mut limiter) => {
                if let Err(retry_after_ms) = limiter.check(now) {
                    if verbose {
                        log_info(&format!(
                            "[ItemSearch] rate limited index load, retry_after_ms={}",
                            retry_after_ms
                        ));
                    }
                    return MxlItemSearchResult::RateLimited {
                        message: RATE_LIMIT_MESSAGE.to_string(),
                        retry_after_ms,
                    };
                }
            }
            Err(_) => {
                return MxlItemSearchResult::Error {
                    query: query.trim().to_string(),
                    message: SEARCH_FAILED_MESSAGE.to_string(),
                }
            }
        }

        let entries = match fetch_index_from_api(&self.agent) {
            Ok(entries) => entries,
            Err(message) => {
                if verbose {
                    log_info(&format!("[ItemSearch] index fetch failed: {}", message));
                }
                return MxlItemSearchResult::Error {
                    query: query.trim().to_string(),
                    message,
                };
            }
        };

        if verbose {
            log_info(&format!(
                "[ItemSearch] index fetch succeeded, {} entries",
                entries.len()
            ));
        }

        let result = search_index_entries(query, &entries);
        if let Ok(mut cache) = self.index_cache.lock() {
            *cache = Some(entries);
        }
        result
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

fn parse_api_response(query: &str, body: &str) -> MxlItemSearchResult {
    let parsed: RawApiResponse = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            return MxlItemSearchResult::Error {
                query: query.to_string(),
                message: SEARCH_FAILED_MESSAGE.to_string(),
            }
        }
    };

    if parsed.ok == Some(false)
        || parsed
            .error
            .as_deref()
            .map(|e| e.eq_ignore_ascii_case("Item not found."))
            .unwrap_or(false)
    {
        return MxlItemSearchResult::NotFound {
            query: query.to_string(),
            message: ITEM_NOT_FOUND_MESSAGE.to_string(),
        };
    }

    if let Some(items) = parsed.items {
        let entries = items
            .into_iter()
            .map(|item| {
                let name_display = if item.name_display.is_empty() {
                    item.name.clone()
                } else {
                    item.name_display.clone()
                };
                let detail = MxlItemDetail {
                    name: item.name.clone(),
                    name_display,
                    quality: item.quality.clone(),
                    class: item.class.clone(),
                    type_name: item.type_name.clone(),
                    runeword: item.runeword,
                    runeword_level: item.runeword_level,
                    stats: item.stats,
                };
                MxlItemEntry {
                    name: item.name,
                    quality: item.quality,
                    class: item.class,
                    type_name: item.type_name,
                    detail: Some(detail),
                }
            })
            .collect();
        return MxlItemSearchResult::Results {
            query: parsed.query.unwrap_or_else(|| query.to_string()),
            entries,
            message: None,
        };
    }

    if let Some(matches) = parsed.matches {
        let entries = matches
            .into_iter()
            .map(|m| MxlItemEntry {
                name: m.name,
                quality: m.quality,
                class: m.class,
                type_name: m.type_name,
                detail: None,
            })
            .collect();
        return MxlItemSearchResult::Results {
            query: parsed.query.unwrap_or_else(|| query.to_string()),
            entries,
            message: parsed
                .truncated
                .unwrap_or(false)
                .then(|| TOO_MANY_MATCHES_MESSAGE.to_string()),
        };
    }

    MxlItemSearchResult::Error {
        query: query.to_string(),
        message: SEARCH_FAILED_MESSAGE.to_string(),
    }
}

fn parse_index_response(body: &str) -> Result<Vec<MxlItemEntry>, String> {
    let parsed: RawIndexResponse =
        serde_json::from_str(body).map_err(|_| SEARCH_FAILED_MESSAGE.to_string())?;

    let entries = parsed
        .items
        .into_iter()
        .map(|item| MxlItemEntry {
            name: item.name,
            quality: item.quality,
            class: item.class,
            type_name: item.type_name,
            detail: None,
        })
        .collect::<Vec<_>>();

    if parsed.count != entries.len() {
        return Err(SEARCH_FAILED_MESSAGE.to_string());
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_entry(name: &str, quality: &str, class: &str, type_name: &str) -> MxlItemEntry {
        MxlItemEntry {
            name: name.to_string(),
            quality: quality.to_string(),
            class: class.to_string(),
            type_name: type_name.to_string(),
            detail: None,
        }
    }

    #[test]
    fn parses_full_items_response() {
        let body = r#"{
          "ok": true,
          "query": "Azurewrath",
          "count": 1,
          "truncated": false,
          "items": [{
            "index": 1,
            "name": "Azurewrath",
            "name_display": "Azurewrath",
            "quality": "SU",
            "class": "Crystal Swords",
            "type": "Crystal Sword",
            "runeword": "",
            "runeword_level": "",
            "stats": "Required Level: 100\nSocketed (6)"
          }]
        }"#;

        let result = parse_api_response("Azurewrath", body);

        match result {
            MxlItemSearchResult::Results {
                entries, message, ..
            } => {
                assert_eq!(message, None);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "Azurewrath");
                assert_eq!(entries[0].quality, "SU");
                assert_eq!(entries[0].class, "Crystal Swords");
                assert_eq!(entries[0].type_name, "Crystal Sword");
                assert_eq!(
                    entries[0].detail.as_ref().unwrap().stats,
                    "Required Level: 100\nSocketed (6)"
                );
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn parses_matches_response_with_friendly_truncated_message() {
        let body = r#"{
          "ok": true,
          "query": "sacred",
          "count": 3,
          "truncated": true,
          "matches": [
            { "index": 1, "name": "Sacred Charge", "quality": "Sacred Set", "class": "Barbarian One-Handed Axes", "type": "Hammerhead Axe" }
          ],
          "message": "More than 3 matches found. Refine q or request one result explicitly."
        }"#;

        let result = parse_api_response("sacred", body);

        match result {
            MxlItemSearchResult::Results {
                entries, message, ..
            } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "Sacred Charge");
                assert_eq!(entries[0].detail, None);
                assert_eq!(message, Some(TOO_MANY_MATCHES_MESSAGE.to_string()));
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn parses_index_response_entries() {
        let body = r#"{
          "generated_at": "2026-05-17T10:00:00Z",
          "count": 2,
          "items": [
            { "name": "Lylia's Curse", "quality": "Quest", "class": "Quest Charms", "type": "Lylia's Curse<br>" },
            { "name": "Azurewrath", "quality": "SU", "class": "Crystal Swords", "type": "Crystal Sword" }
          ]
        }"#;

        let entries = parse_index_response(body).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Lylia's Curse");
        assert_eq!(entries[0].quality, "Quest");
        assert_eq!(entries[0].class, "Quest Charms");
        assert_eq!(entries[0].type_name, "Lylia's Curse<br>");
        assert_eq!(entries[0].detail, None);
    }

    #[test]
    fn searches_index_with_name_first_ranking() {
        let entries = vec![
            index_entry("Blade of Light", "SU", "Swords", "Sword"),
            index_entry("Questing Beast", "SU", "Swords", "Sword"),
            index_entry("Lylia's Curse", "Quest", "Quest Charms", "Charm"),
            index_entry("Curse of the Zakarum", "SU", "Maces", "Mace"),
            index_entry(
                "Arcane Hunger",
                "Effigy",
                "Occult Effigies",
                "Occult Effigy",
            ),
            index_entry("Sunstone", "Quest", "Charms", "Charm"),
        ];

        let result = search_index_entries("quest", &entries);

        match result {
            MxlItemSearchResult::Results {
                entries, message, ..
            } => {
                assert_eq!(message, None);
                assert_eq!(entries.len(), 3);
                assert_eq!(entries[0].name, "Questing Beast");
                assert_eq!(entries[1].name, "Lylia's Curse");
                assert_eq!(entries[2].name, "Sunstone");
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn short_index_query_returns_hint() {
        let entries = vec![index_entry(
            "Lylia's Curse",
            "Quest",
            "Quest Charms",
            "Charm",
        )];

        let result = search_index_entries("l", &entries);

        assert_eq!(
            result,
            MxlItemSearchResult::Results {
                query: "l".to_string(),
                entries: Vec::new(),
                message: Some(TYPEAHEAD_MIN_QUERY_MESSAGE.to_string()),
            }
        );
    }

    #[test]
    fn index_search_returns_all_matches_without_overflow_message() {
        let total_matches = 52;
        let entries = (0..total_matches)
            .map(|i| index_entry(&format!("Sacred Item {:03}", i), "SU", "Swords", "Sword"))
            .collect::<Vec<_>>();

        let result = search_index_entries("sacred", &entries);

        match result {
            MxlItemSearchResult::Results {
                entries, message, ..
            } => {
                assert_eq!(entries.len(), total_matches);
                assert_eq!(message, None);
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn parses_item_not_found_response() {
        let body = r#"{ "ok": false, "error": "Item not found." }"#;

        let result = parse_api_response("missing", body);

        assert_eq!(
            result,
            MxlItemSearchResult::NotFound {
                query: "missing".to_string(),
                message: ITEM_NOT_FOUND_MESSAGE.to_string(),
            }
        );
    }

    #[test]
    fn invalid_json_returns_controlled_error() {
        let result = parse_api_response("bad", "not json");

        assert_eq!(
            result,
            MxlItemSearchResult::Error {
                query: "bad".to_string(),
                message: SEARCH_FAILED_MESSAGE.to_string(),
            }
        );
    }

    #[test]
    fn limiter_allows_ten_requests_per_window() {
        let start = Instant::now();
        let mut limiter = RequestWindow::default();

        for _ in 0..RATE_LIMIT_COUNT {
            assert_eq!(limiter.check(start), Ok(()));
        }

        assert!(limiter.check(start).is_err());
    }

    #[test]
    fn limiter_recovers_after_window() {
        let start = Instant::now();
        let mut limiter = RequestWindow::default();

        for _ in 0..RATE_LIMIT_COUNT {
            assert_eq!(limiter.check(start), Ok(()));
        }

        assert_eq!(limiter.check(start + RATE_LIMIT_WINDOW), Ok(()));
    }

    #[test]
    fn percent_encodes_spaces_and_apostrophes() {
        assert_eq!(
            percent_encode_query("Red Vex' Curse"),
            "Red%20Vex%27%20Curse"
        );
    }

    #[test]
    fn normalize_query_trims_and_lowercases() {
        assert_eq!(normalize_query("  Azurewrath  "), "azurewrath");
    }

    #[test]
    fn rate_limited_result_serializes_retry_after_as_camel_case() {
        let json = serde_json::to_value(MxlItemSearchResult::RateLimited {
            message: RATE_LIMIT_MESSAGE.to_string(),
            retry_after_ms: 1234,
        })
        .unwrap();

        assert_eq!(json["kind"], "rateLimited");
        assert_eq!(json["retryAfterMs"], 1234);
        assert!(json.get("retry_after_ms").is_none());
    }
}
