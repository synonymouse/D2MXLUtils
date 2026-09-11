use std::time::Duration;

use super::response::{parse_api_response, parse_index_response};
use super::{MxlItemEntry, MxlItemSearchResult, ITEM_NOT_FOUND_MESSAGE, SEARCH_FAILED_MESSAGE};

const API_URL: &str = "https://tsw.vn.cz/stats/api_item.php";

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

pub(super) fn default_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_recv_body(Some(Duration::from_secs(5)))
        .build()
        .into()
}

pub(super) fn fetch_from_api(agent: &ureq::Agent, trimmed: &str) -> MxlItemSearchResult {
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

pub(super) fn fetch_index_from_api(agent: &ureq::Agent) -> Result<Vec<MxlItemEntry>, String> {
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

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
