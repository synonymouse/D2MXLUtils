//! Rule matching against a single scanned item.

use regex::Regex;

use super::{ItemQuality, ItemTier, Rule};
use crate::notifier::ItemDropEvent;

pub struct MatchContext<'a> {
    pub item: &'a ItemDropEvent,
    name_lower: String,
    base_name_lower: String,
    stats_lower: String,
}

impl<'a> MatchContext<'a> {
    pub fn new(item: &'a ItemDropEvent) -> Self {
        Self {
            item,
            name_lower: item.name.to_lowercase(),
            base_name_lower: item.base_name.to_lowercase(),
            stats_lower: item.stats.to_lowercase(),
        }
    }

    pub fn matches(&self, rule: &Rule) -> bool {
        if !self.qualities_match(&rule.qualities) {
            return false;
        }
        if !self.tiers_match(&rule.tiers) {
            return false;
        }
        if rule.ethereal && !self.item.is_ethereal {
            return false;
        }
        if let Some(ref pattern) = rule.name_pattern {
            // OR across runtime display name and items.txt base type:
            // a rare's affix name wouldn't otherwise match `"Ring$"`.
            let name_hit = pattern_matches(pattern, &self.name_lower);
            let base_hit =
                !self.base_name_lower.is_empty() && pattern_matches(pattern, &self.base_name_lower);
            if !(name_hit || base_hit) {
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

    fn qualities_match(&self, rule_qualities: &[ItemQuality]) -> bool {
        if rule_qualities.is_empty() {
            return true;
        }
        rule_qualities
            .iter()
            .any(|q| self.item.quality.eq_ignore_ascii_case(q.d2_quality_name()))
    }

    fn tiers_match(&self, rule_tiers: &[ItemTier]) -> bool {
        if rule_tiers.is_empty() {
            return true;
        }
        match self.item.tier {
            Some(item_tier) => rule_tiers.iter().any(|&t| t == item_tier),
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
            base_name: String::new(),
            stats: stats.to_string(),
            is_ethereal: eth,
            is_identified: true,
            p_unit_data: 0,
            tier: None,
            filter: None,
        }
    }

    fn item_with_base(name: &str, base: &str, quality: &str, stats: &str) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: 1,
            class: 25,
            quality: quality.to_string(),
            name: name.to_string(),
            base_name: base.to_string(),
            stats: stats.to_string(),
            is_ethereal: false,
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
            qualities: vec![ItemQuality::Unique],
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
        let r = Rule {
            qualities: vec![ItemQuality::Rare],
            ..Rule::default()
        };
        assert!(!ctx.matches(&r));
    }

    #[test]
    fn multi_quality_rule_matches_any_listed_quality() {
        let r = Rule {
            qualities: vec![ItemQuality::Magic, ItemQuality::Rare, ItemQuality::Unique],
            ..Rule::default()
        };
        for q in ["Magic", "Rare", "Unique"] {
            let it = item("X", q, "", false);
            let ctx = MatchContext::new(&it);
            assert!(ctx.matches(&r), "quality {} should match", q);
        }
        let it = item("X", "Normal", "", false);
        let ctx = MatchContext::new(&it);
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
            tiers: vec![ItemTier::Sacred],
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
            tiers: vec![ItemTier::Sacred],
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }

    #[test]
    fn tier_zero_matches_untiered_items() {
        let mut it = item("Ist Rune", "Normal", "", false);
        it.tier = Some(ItemTier::Tier0);
        let ctx = MatchContext::new(&it);
        let r = Rule {
            tiers: vec![ItemTier::Tier0],
            ..Rule::default()
        };
        assert!(ctx.matches(&r));

        let mut sacred = item("Sacred Axe", "Unique", "", false);
        sacred.tier = Some(ItemTier::Sacred);
        let sctx = MatchContext::new(&sacred);
        assert!(!sctx.matches(&r));
    }

    #[test]
    fn multi_tier_rule_matches_any_listed_tier() {
        let r = Rule {
            tiers: vec![
                ItemTier::Tier1,
                ItemTier::Tier2,
                ItemTier::Tier3,
                ItemTier::Tier4,
            ],
            ..Rule::default()
        };
        for t in [
            ItemTier::Tier1,
            ItemTier::Tier2,
            ItemTier::Tier3,
            ItemTier::Tier4,
        ] {
            let mut it = item("X", "Normal", "", false);
            it.tier = Some(t);
            let ctx = MatchContext::new(&it);
            assert!(ctx.matches(&r), "tier {:?} should match", t);
        }

        let mut t0 = item("X", "Normal", "", false);
        t0.tier = Some(ItemTier::Tier0);
        assert!(!MatchContext::new(&t0).matches(&r));
        let mut sc = item("X", "Unique", "", false);
        sc.tier = Some(ItemTier::Sacred);
        assert!(!MatchContext::new(&sc).matches(&r));
    }

    #[test]
    fn multi_tier_plus_quality_intersects() {
        let r = Rule {
            tiers: vec![
                ItemTier::Tier1,
                ItemTier::Tier2,
                ItemTier::Tier3,
                ItemTier::Tier4,
            ],
            qualities: vec![ItemQuality::Unique],
            ..Rule::default()
        };

        let mut u2 = item("X", "Unique", "", false);
        u2.tier = Some(ItemTier::Tier2);
        assert!(MatchContext::new(&u2).matches(&r));

        let mut n2 = item("X", "Normal", "", false);
        n2.tier = Some(ItemTier::Tier2);
        assert!(!MatchContext::new(&n2).matches(&r));

        let mut usc = item("X", "Unique", "", false);
        usc.tier = Some(ItemTier::Sacred);
        assert!(!MatchContext::new(&usc).matches(&r));
    }

    #[test]
    fn name_pattern_matches_against_base_name_for_rare_affix() {
        let it = item_with_base(
            "Rune Turn",
            "Ring",
            "Rare",
            "+1 to All Skills\n+10% to Fire Spell Damage",
        );
        let ctx = MatchContext::new(&it);
        let r = Rule {
            name_pattern: Some("Ring$".into()),
            qualities: vec![ItemQuality::Rare],
            stat_pattern: Some("Skills".into()),
            notify: true,
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }

    #[test]
    fn name_pattern_still_matches_against_runtime_name() {
        let it = item_with_base("Stone of Jordan", "Ring", "Unique", "");
        let ctx = MatchContext::new(&it);
        let r = Rule {
            name_pattern: Some("Stone of Jordan".into()),
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }

    #[test]
    fn name_pattern_fails_when_neither_name_nor_base_match() {
        let it = item_with_base("Rune Turn", "Ring", "Rare", "");
        let ctx = MatchContext::new(&it);
        let r = Rule {
            name_pattern: Some("Amulet".into()),
            ..Rule::default()
        };
        assert!(!ctx.matches(&r));
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
