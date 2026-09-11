use super::index::{normalize_query, search_index_entries};
use super::response::{parse_api_response, parse_index_response};
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
