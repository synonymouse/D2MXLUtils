//! Rule matching against a single scanned item.

use super::{
    CompiledPattern, EnrichmentNeeds, ItemQuality, ItemTier, PartialRuleMatch, PlayerClass, Rule,
};
use crate::notifier::ItemDropEvent;

pub struct MatchContext<'a> {
    pub item: &'a ItemDropEvent,
    name_lower: String,
    base_name_lower: String,
    category_lower: String,
    stats_lower: String,
}

impl<'a> MatchContext<'a> {
    pub fn new(item: &'a ItemDropEvent) -> Self {
        Self {
            item,
            name_lower: item.name.to_lowercase(),
            base_name_lower: item.base_name.to_lowercase(),
            category_lower: item
                .category
                .as_deref()
                .map(str::to_lowercase)
                .unwrap_or_default(),
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
        if !self.sockets_match(&rule.sockets) {
            return false;
        }
        if !self.classes_match(&rule.classes) {
            return false;
        }
        if !self.level_match(rule.min_clvl, rule.max_clvl, self.item.clvl) {
            return false;
        }
        if !self.level_match(rule.min_ilvl, rule.max_ilvl, self.item.ilvl) {
            return false;
        }
        if rule.quest
            && !(self.base_name_lower.contains("quest item")
                || self.category_lower.contains("quest item"))
        {
            return false;
        }
        if rule.ethereal && !self.item.is_ethereal {
            return false;
        }

        if let Some(ref pattern) = rule.name_pattern {
            let compiled = rule.compiled_name_pattern();
            let is_runtime_rare_name =
                self.item.name_is_runtime && self.item.quality.eq_ignore_ascii_case("Rare");
            let name_hit = !is_runtime_rare_name
                && !self.name_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.name_lower);
            let base_hit = !self.base_name_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.base_name_lower);
            let category_hit = !self.category_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.category_lower);

            if !(name_hit || base_hit || category_hit) {
                return false;
            }
        }

        let compiled_stat_patterns = rule.compiled_stat_patterns();
        for (index, pattern) in rule.stat_patterns.iter().enumerate() {
            let compiled = compiled_stat_patterns.and_then(|patterns| patterns.get(index));
            if !pattern_matches(pattern, compiled, &self.stats_lower) {
                return false;
            }
        }
        true
    }

    pub fn partial_matches(&self, rule: &Rule) -> PartialRuleMatch {
        if !self.qualities_match(&rule.qualities) {
            return PartialRuleMatch::NoMatch;
        }
        if !self.tiers_match(&rule.tiers) {
            return PartialRuleMatch::NoMatch;
        }
        if !self.sockets_match(&rule.sockets) {
            return PartialRuleMatch::NoMatch;
        }
        if !self.classes_match(&rule.classes) {
            return PartialRuleMatch::NoMatch;
        }
        if !self.level_match(rule.min_clvl, rule.max_clvl, self.item.clvl) {
            return PartialRuleMatch::NoMatch;
        }
        if !self.level_match(rule.min_ilvl, rule.max_ilvl, self.item.ilvl) {
            return PartialRuleMatch::NoMatch;
        }
        if rule.quest
            && !(self.base_name_lower.contains("quest item")
                || self.category_lower.contains("quest item"))
        {
            return PartialRuleMatch::NoMatch;
        }
        if rule.ethereal && !self.item.is_ethereal {
            return PartialRuleMatch::NoMatch;
        }

        if let Some(ref pattern) = rule.name_pattern {
            let compiled = rule.compiled_name_pattern();
            let is_runtime_rare_name =
                self.item.name_is_runtime && self.item.quality.eq_ignore_ascii_case("Rare");
            let name_hit = !is_runtime_rare_name
                && !self.name_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.name_lower);
            let base_hit = !self.base_name_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.base_name_lower);
            let category_hit = !self.category_lower.is_empty()
                && pattern_matches(pattern, compiled, &self.category_lower);

            if !(name_hit || base_hit || category_hit) {
                return PartialRuleMatch::NoMatch;
            }
        }

        if !rule.stat_patterns.is_empty() && !self.item.runtime_stats_loaded {
            return PartialRuleMatch::Needs(EnrichmentNeeds::stats());
        }

        let compiled_stat_patterns = rule.compiled_stat_patterns();
        for (index, pattern) in rule.stat_patterns.iter().enumerate() {
            let compiled = compiled_stat_patterns.and_then(|patterns| patterns.get(index));
            if !pattern_matches(pattern, compiled, &self.stats_lower) {
                return PartialRuleMatch::NoMatch;
            }
        }

        PartialRuleMatch::Match
    }

    /// Empty for patterns that only match across line boundaries (e.g.
    /// `(?s)a.*b`), even if the rule matched the blob as a whole.
    pub fn matching_stat_lines(&self, patterns: &[String]) -> Vec<usize> {
        self.matching_stat_lines_with(patterns, None)
    }

    pub fn matching_stat_lines_for_rule(&self, rule: &Rule) -> Vec<usize> {
        self.matching_stat_lines_with(&rule.stat_patterns, rule.compiled_stat_patterns())
    }

    fn matching_stat_lines_with(
        &self,
        patterns: &[String],
        compiled_patterns: Option<&[CompiledPattern]>,
    ) -> Vec<usize> {
        if patterns.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<usize> = self
            .stats_lower
            .split('\n')
            .enumerate()
            .filter(|(_, line)| {
                patterns.iter().enumerate().any(|(index, pattern)| {
                    let compiled = compiled_patterns.and_then(|patterns| patterns.get(index));
                    pattern_matches(pattern, compiled, line)
                })
            })
            .map(|(i, _)| i)
            .collect();
        hits.sort_unstable();
        hits.dedup();
        hits
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

    fn sockets_match(&self, rule_sockets: &[u8]) -> bool {
        if rule_sockets.is_empty() {
            return true;
        }
        rule_sockets.iter().any(|&n| n == self.item.sockets)
    }

    fn classes_match(&self, rule_classes: &[PlayerClass]) -> bool {
        if rule_classes.is_empty() {
            return true;
        }
        match PlayerClass::from_id(self.item.player_class) {
            Some(class) => rule_classes.iter().any(|&c| c == class),
            None => false,
        }
    }

    fn level_match(&self, min: Option<u32>, max: Option<u32>, value: u32) -> bool {
        if let Some(min) = min {
            if value < min {
                return false;
            }
        }
        if let Some(max) = max {
            if value > max {
                return false;
            }
        }
        true
    }
}

fn pattern_matches(
    pattern: &str,
    compiled: Option<&CompiledPattern>,
    haystack_lower: &str,
) -> bool {
    match compiled {
        Some(compiled) => compiled.is_match(haystack_lower),
        None => CompiledPattern::new(pattern).is_match(haystack_lower),
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
            category: None,
            stats: stats.to_string(),
            name_is_runtime: false,
            runtime_stats_loaded: !stats.is_empty(),
            is_ethereal: eth,
            is_identified: true,
            p_unit_data: 0,
            seed: 0,
            history_pushed: false,
            tier: None,
            unique_kind: None,
            sockets: 0,
            clvl: 0,
            ilvl: 0,
            player_class: 0,
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
            category: None,
            stats: stats.to_string(),
            name_is_runtime: false,
            runtime_stats_loaded: !stats.is_empty(),
            is_ethereal: false,
            is_identified: true,
            p_unit_data: 0,
            seed: 0,
            history_pushed: false,
            tier: None,
            unique_kind: None,
            sockets: 0,
            clvl: 0,
            ilvl: 0,
            player_class: 0,
            filter: None,
        }
    }

    fn cheap_item(name: &str, base: &str, quality: &str, stats: &str) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: 1,
            class: 0,
            quality: quality.to_string(),
            name: name.to_string(),
            base_name: base.to_string(),
            category: None,
            stats: stats.to_string(),
            name_is_runtime: false,
            runtime_stats_loaded: !stats.is_empty(),
            is_ethereal: true,
            is_identified: true,
            p_unit_data: 0,
            seed: 0,
            history_pushed: false,
            tier: Some(ItemTier::Sacred),
            unique_kind: None,
            sockets: 0,
            clvl: 0,
            ilvl: 0,
            player_class: 0,
            filter: None,
        }
    }

    #[test]
    fn partial_match_static_unique_name_needs_no_get_item_name() {
        let item = cheap_item("Shamanka", "Long Staff (Sacred)", "Unique", "");
        let ctx = MatchContext::new(&item);
        let rule = Rule {
            name_pattern: Some("Shamanka".into()),
            qualities: vec![ItemQuality::Unique],
            ..Rule::default()
        };
        assert!(matches!(
            ctx.partial_matches(&rule),
            PartialRuleMatch::Match
        ));
    }

    #[test]
    fn partial_match_base_name_rule_needs_no_get_item_name() {
        let item = cheap_item("Large Axe (Sacred)", "Large Axe (Sacred)", "Superior", "");
        let ctx = MatchContext::new(&item);
        let rule = Rule {
            name_pattern: Some("Large Axe".into()),
            qualities: vec![ItemQuality::Superior],
            tiers: vec![ItemTier::Sacred],
            ethereal: true,
            ..Rule::default()
        };
        assert!(matches!(
            ctx.partial_matches(&rule),
            PartialRuleMatch::Match
        ));
    }

    #[test]
    fn partial_match_stat_pattern_requests_stats_after_static_name_match() {
        let item = cheap_item("Amulet", "Amulet", "Rare", "");
        let ctx = MatchContext::new(&item);
        let rule = Rule {
            name_pattern: Some("Amulet$".into()),
            qualities: vec![ItemQuality::Rare],
            stat_patterns: vec!["[3-9] to All Skills".into()],
            ..Rule::default()
        };
        assert!(matches!(
            ctx.partial_matches(&rule),
            PartialRuleMatch::Needs(EnrichmentNeeds {
                runtime_stats: true
            })
        ));
    }

    #[test]
    fn partial_match_stat_pattern_does_not_request_stats_when_base_misses() {
        let item = cheap_item("Large Axe (Sacred)", "Large Axe (Sacred)", "Rare", "");
        let ctx = MatchContext::new(&item);
        let rule = Rule {
            name_pattern: Some("Amulet$".into()),
            qualities: vec![ItemQuality::Rare],
            stat_patterns: vec!["[3-9] to All Skills".into()],
            ..Rule::default()
        };
        assert!(matches!(
            ctx.partial_matches(&rule),
            PartialRuleMatch::NoMatch
        ));
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
    fn quest_rule_matches_only_quest_item_base_name() {
        let quest_it = item_with_base("Tome of Possession", "Quest Item", "Normal", "");
        let ctx = MatchContext::new(&quest_it);
        let r = Rule {
            quest: true,
            ..Rule::default()
        };
        assert!(ctx.matches(&r));

        let ring_it = item_with_base("Ring of the Five", "Ring", "Normal", "");
        let ctx = MatchContext::new(&ring_it);
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
    fn socket_rule_matches_listed_counts_and_rejects_others() {
        let r = Rule {
            sockets: vec![0, 4, 6],
            ..Rule::default()
        };
        for n in [0u8, 4, 6] {
            let mut it = item("X", "Normal", "", false);
            it.sockets = n;
            assert!(
                MatchContext::new(&it).matches(&r),
                "sockets={} should match",
                n
            );
        }
        for n in [1u8, 2, 3, 5] {
            let mut it = item("X", "Normal", "", false);
            it.sockets = n;
            assert!(
                !MatchContext::new(&it).matches(&r),
                "sockets={} must NOT match",
                n
            );
        }
    }

    #[test]
    fn class_rule_matches_listed_classes_and_rejects_others() {
        let r = Rule {
            classes: vec![PlayerClass::Necromancer, PlayerClass::Barbarian],
            ..Rule::default()
        };
        for id in [2u32, 4] {
            let mut it = item("X", "Normal", "", false);
            it.player_class = id;
            assert!(
                MatchContext::new(&it).matches(&r),
                "class id={} should match",
                id
            );
        }
        for id in [0u32, 1, 3, 5, 6] {
            let mut it = item("X", "Normal", "", false);
            it.player_class = id;
            assert!(
                !MatchContext::new(&it).matches(&r),
                "class id={} must NOT match",
                id
            );
        }
    }

    #[test]
    fn clvl_range_matches_inclusive_bounds_and_rejects_outside() {
        let r = Rule {
            min_clvl: Some(20),
            max_clvl: Some(99),
            ..Rule::default()
        };
        for clvl in [20u32, 50, 99] {
            let mut it = item("X", "Normal", "", false);
            it.clvl = clvl;
            assert!(
                MatchContext::new(&it).matches(&r),
                "clvl={} should match",
                clvl
            );
        }
        for clvl in [0u32, 19, 100] {
            let mut it = item("X", "Normal", "", false);
            it.clvl = clvl;
            assert!(
                !MatchContext::new(&it).matches(&r),
                "clvl={} must NOT match",
                clvl
            );
        }
    }

    #[test]
    fn ilvl_range_open_ended_bounds() {
        let min_only = Rule {
            min_ilvl: Some(40),
            ..Rule::default()
        };
        let mut low = item("X", "Normal", "", false);
        low.ilvl = 39;
        assert!(!MatchContext::new(&low).matches(&min_only));
        let mut high = item("X", "Normal", "", false);
        high.ilvl = 999;
        assert!(MatchContext::new(&high).matches(&min_only));

        let max_only = Rule {
            max_ilvl: Some(99),
            ..Rule::default()
        };
        let mut ok = item("X", "Normal", "", false);
        ok.ilvl = 99;
        assert!(MatchContext::new(&ok).matches(&max_only));
        let mut over = item("X", "Normal", "", false);
        over.ilvl = 100;
        assert!(!MatchContext::new(&over).matches(&max_only));
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
            stat_patterns: vec!["Skills".into()],
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
    fn name_pattern_matches_against_class_category() {
        let mut rhal = item_with_base("Rhal Rune", "Rhal Rune", "Normal", "");
        rhal.category = Some("Great Rune".to_string());
        let ctx = MatchContext::new(&rhal);

        let r = Rule {
            name_pattern: Some("Great Rune".into()),
            ..Rule::default()
        };
        assert!(ctx.matches(&r));

        let unrelated = Rule {
            name_pattern: Some("Enchanted Rune".into()),
            ..Rule::default()
        };
        assert!(!ctx.matches(&unrelated));

        let plain = item_with_base("Random Item", "Random Item", "Normal", "");
        assert!(!MatchContext::new(&plain).matches(&r));
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
    fn name_pattern_does_not_match_runtime_name_for_rare() {
        let mut it = item_with_base("Rune Turn", "Ring", "Rare", "");
        it.name_is_runtime = true;
        let ctx = MatchContext::new(&it);
        let r = Rule {
            name_pattern: Some("Rune Turn".into()),
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
            stat_patterns: vec![r"\+\d+ to All Skills".into()],
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
    }

    #[test]
    fn matching_stat_lines_finds_later_line() {
        let it = item(
            "Ring",
            "Unique",
            "+3 to All Skills\n+15% Faster Cast Rate",
            false,
        );
        let ctx = MatchContext::new(&it);
        assert_eq!(
            ctx.matching_stat_lines(&["Faster Cast".to_string()]),
            vec![1]
        );
    }

    #[test]
    fn matching_stat_lines_empty_when_no_line_matches() {
        let it = item(
            "Ring",
            "Unique",
            "+3 to All Skills\n+15% Faster Cast Rate",
            false,
        );
        let ctx = MatchContext::new(&it);
        let hits = ctx.matching_stat_lines(&["Life Steal".to_string()]);
        assert!(hits.is_empty());
    }

    #[test]
    fn multi_stat_patterns_all_must_match() {
        let it = item(
            "Ring",
            "Unique",
            "+3 to All Skills\n+15% Faster Cast Rate",
            false,
        );
        let ctx = MatchContext::new(&it);

        let both_present = Rule {
            stat_patterns: vec!["All Skills".into(), "Faster Cast".into()],
            ..Rule::default()
        };
        assert!(ctx.matches(&both_present));

        let one_missing = Rule {
            stat_patterns: vec!["All Skills".into(), "Life Steal".into()],
            ..Rule::default()
        };
        assert!(!ctx.matches(&one_missing));
    }

    #[test]
    fn matching_stat_lines_returns_union_sorted_deduped() {
        let it = item(
            "Ring",
            "Unique",
            "+10 to Strength\n+5 to Strength\n+1 to All Skills",
            false,
        );
        let ctx = MatchContext::new(&it);

        assert_eq!(
            ctx.matching_stat_lines(&["Strength".into(), "Skills".into()]),
            vec![0, 1, 2]
        );
        assert!(ctx.matching_stat_lines(&["nothing".into()]).is_empty());
        assert!(ctx.matching_stat_lines(&[]).is_empty());
    }

    #[test]
    fn multi_line_regex_pattern_contributes_no_line_highlight() {
        let it = item("Ring", "Unique", "+3 to All Skills\n+15% FCR", false);
        let ctx = MatchContext::new(&it);
        let r = Rule {
            stat_patterns: vec!["(?s)All Skills.*FCR".into()],
            ..Rule::default()
        };
        assert!(ctx.matches(&r));
        assert!(ctx.matching_stat_lines(&r.stat_patterns).is_empty());
    }
}
