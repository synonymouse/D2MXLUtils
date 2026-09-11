use super::{
    MxlItemEntry, MxlItemSearchResult, TYPEAHEAD_MIN_QUERY_LEN, TYPEAHEAD_MIN_QUERY_MESSAGE,
};

pub(super) fn normalize_query(query: &str) -> String {
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

pub(super) fn search_index_entries(query: &str, entries: &[MxlItemEntry]) -> MxlItemSearchResult {
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
