use serde::Deserialize;

use super::{
    MxlItemDetail, MxlItemEntry, MxlItemSearchResult, ITEM_NOT_FOUND_MESSAGE,
    SEARCH_FAILED_MESSAGE, TOO_MANY_MATCHES_MESSAGE,
};

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

pub(super) fn parse_api_response(query: &str, body: &str) -> MxlItemSearchResult {
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

pub(super) fn parse_index_response(body: &str) -> Result<Vec<MxlItemEntry>, String> {
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
