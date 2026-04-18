//! Rule matching against a single scanned item.
//!
//! A [`MatchContext`] caches lowercase copies of the item's name/stats so
//! rules with regexes don't redo the work. Matching is pure: no per-item
//! state mutates between rules.

use regex::Regex;

use super::{ItemQuality, ItemTier, Rule};
use crate::notifier::ItemDropEvent;

pub struct MatchContext<'a> {
    pub item: &'a ItemDropEvent,
    name_lower: String,
    stats_lower: String,
}

impl<'a> MatchContext<'a> {
    pub fn new(item: &'a ItemDropEvent) -> Self {
        Self {
            item,
            name_lower: item.name.to_lowercase(),
            stats_lower: item.stats.to_lowercase(),
        }
    }

    pub fn matches(&self, rule: &Rule) -> bool {
        if !self.quality_matches(rule.quality) {
            return false;
        }
        if !self.tier_matches(rule.tier) {
            return false;
        }
        if rule.ethereal && !self.item.is_ethereal {
            return false;
        }
        if let Some(ref pattern) = rule.name_pattern {
            if !pattern_matches(pattern, &self.name_lower) {
                return false;
            }
        }
        if let Some(ref pattern) = rule.stat_pattern {
            if !pattern_matches(pattern, &self.stats_lower) {
                return false;
            }
        }
        true
    }

    fn quality_matches(&self, rule_quality: ItemQuality) -> bool {
        if rule_quality == ItemQuality::Any {
            return true;
        }
        match rule_quality.d2_quality_name() {
            Some(expected) => self.item.quality.eq_ignore_ascii_case(expected),
            None => true,
        }
    }

    fn tier_matches(&self, rule_tier: ItemTier) -> bool {
        if rule_tier == ItemTier::Any {
            return true;
        }
        match self.item.tier {
            Some(item_tier) => item_tier == rule_tier,
            None => false,
        }
    }
}

fn pattern_matches(pattern: &str, haystack_lower: &str) -> bool {
    match Regex::new(&format!("(?i){}", pattern)) {
        Ok(re) => re.is_match(haystack_lower),
        Err(_) => haystack_lower.contains(&pattern.to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, quality: &str, stats: &str, eth: bool) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: 1,
            class: 25,
            quality: quality.to_string(),
            name: name.to_string(),
            stats: stats.to_string(),
            is_ethereal: eth,
            is_identified: true,
            p_unit_data: 0,
            tier: None,
            filter: None,
        }
    }

    #[test]
    fn name_pattern_regex_and_substring_fallback() {
        let it = item("Stone of Jordan", "Unique", "", false);
        let ctx = MatchContext::new(&it);

        let rule = Rule {
            name_pattern: Some("Jordan$".into()),
            ..Rule::default()
        };
        assert!(ctx.matches(&rule));

        let bad = Rule {
            name_pattern: Some("Ring[".into()),
            ..Rule::default()
        };
        assert!(!ctx.matches(&bad));
    }

    #[test]
    fn quality_match_uses_item_quality_name() {
        let it = item("X", "Unique", "", false);
        let ctx = MatchContext::new(&it);
        let r = Rule {
            quality: ItemQuality::Unique,
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
        let r = Rule {
            quality: ItemQuality::Rare,
            ..Rule::default()
        };
        assert!(!ctx.matches(&r));
    }

    #[test]
    fn ethereal_only_required_mode() {
        let eth_it = item("X", "Unique", "", true);
        let ctx = MatchContext::new(&eth_it);
        let r = Rule {
            ethereal: true,
            ..Rule::default()
        };
        assert!(ctx.matches(&r));

        let norm_it = item("X", "Unique", "", false);
        let ctx = MatchContext::new(&norm_it);
        assert!(!ctx.matches(&r));
    }

    #[test]
    fn tier_rule_fails_when_item_tier_unknown() {
        let it = item("X", "Unique", "", false);
        let ctx = MatchContext::new(&it);
        let r = Rule {
            tier: ItemTier::Sacred,
            ..Rule::default()
        };
        assert!(!ctx.matches(&r));
    }

    #[test]
    fn tier_rule_passes_when_item_tier_matches() {
        let mut it = item("X", "Unique", "", false);
        it.tier = Some(ItemTier::Sacred);
        let ctx = MatchContext::new(&it);
        let r = Rule {
            tier: ItemTier::Sacred,
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }

    #[test]
    fn stat_pattern_regex() {
        let it = item(
            "Ring",
            "Unique",
            "+3 to All Skills\n+15% Faster Cast Rate",
            false,
        );
        let ctx = MatchContext::new(&it);
        let r = Rule {
            stat_pattern: Some(r"\+\d+ to All Skills".into()),
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }
}
