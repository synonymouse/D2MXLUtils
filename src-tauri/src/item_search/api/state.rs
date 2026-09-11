use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::index::{normalize_query, search_index_entries};
use super::transport::{default_agent, fetch_from_api, fetch_index_from_api};
use super::{
    result_kind, MxlItemEntry, MxlItemSearchResult, RATE_LIMIT_MESSAGE, SEARCH_FAILED_MESSAGE,
    TYPEAHEAD_MIN_QUERY_LEN,
};
use crate::logger::info as log_info;

const RATE_LIMIT_COUNT: usize = 10;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(10);

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

    pub(super) fn verbose(&self) -> bool {
        self.verbose_logging.load(Ordering::SeqCst)
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

fn result_is_cacheable(result: &MxlItemSearchResult) -> bool {
    !matches!(
        result,
        MxlItemSearchResult::RateLimited { .. } | MxlItemSearchResult::Error { .. }
    )
}

impl MxlItemApiState {
    pub(super) fn cached_or_fetch(&self, query: &str, now: Instant) -> MxlItemSearchResult {
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

    pub(super) fn search_index(&self, query: &str, now: Instant) -> MxlItemSearchResult {
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

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
