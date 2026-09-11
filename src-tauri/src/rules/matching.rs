//! Rule matching against a single scanned item.

use super::{
    CompiledPattern, EnrichmentNeeds, ItemQuality, ItemTier, PartialRuleMatch, PlayerClass, Rule,
    UniqueKind,
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
        if !self.unique_kinds_match(&rule.unique_kinds) {
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
        if !self.unique_kinds_match(&rule.unique_kinds) {
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

    fn unique_kinds_match(&self, rule_unique_kinds: &[UniqueKind]) -> bool {
        if rule_unique_kinds.is_empty() {
            return true;
        }
        match self.item.unique_kind {
            Some(kind) => rule_unique_kinds.iter().any(|&k| k == kind),
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
#[path = "matching_tests.rs"]
mod tests;
