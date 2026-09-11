//! Filter preparation and full/partial last-match decisions on the existing owner.

use super::{
    FilterConfig, FilterDecision, MatchContext, Notification, PartialFilterDecision,
    PartialRuleMatch, Rule, Visibility,
};

impl FilterConfig {
    /// Prepare regex matchers once when a filter is loaded into a runtime path.
    pub fn prepare_for_matching(&mut self) {
        for rule in &mut self.rules {
            rule.prepare_for_matching();
        }
    }

    pub fn has_map_rules(&self) -> bool {
        self.rules.iter().any(|rule| rule.map)
    }

    fn decision_for_rule(&self, rule: &Rule, ctx: &MatchContext) -> FilterDecision {
        FilterDecision {
            visibility: resolve_visibility(rule.visibility, self.hide_all),
            notification: if rule.notify {
                let matched_stat_lines = if rule.stat_patterns.is_empty() {
                    Vec::new()
                } else {
                    ctx.matching_stat_lines_for_rule(rule)
                };
                Some(Notification {
                    color: rule.color,
                    sound: rule.sound.filter(|&s| s != 0),
                    display_stats: rule.display_stats || !rule.stat_patterns.is_empty(),
                    matched_stat_lines,
                })
            } else {
                None
            },
            place_on_map: rule.map,
            matched_line: (rule.source_line != 0).then_some(rule.source_line),
        }
    }

    fn default_decision(&self) -> FilterDecision {
        FilterDecision {
            visibility: if self.hide_all {
                Visibility::Hide
            } else {
                Visibility::Default
            },
            notification: None,
            place_on_map: false,
            matched_line: None,
        }
    }

    /// Decide what to do with an item: last-match wins, per spec.
    pub fn decide(&self, ctx: &MatchContext) -> FilterDecision {
        let winner = self.rules.iter().rev().find(|r| ctx.matches(r));
        match winner {
            None => self.default_decision(),
            Some(rule) => self.decision_for_rule(rule, ctx),
        }
    }

    pub fn decide_partial(&self, ctx: &MatchContext) -> PartialFilterDecision {
        for rule in self.rules.iter().rev() {
            match ctx.partial_matches(rule) {
                PartialRuleMatch::Match => {
                    return PartialFilterDecision::Ready(self.decision_for_rule(rule, ctx));
                }
                PartialRuleMatch::NoMatch => {}
                PartialRuleMatch::Needs(needs) => {
                    return PartialFilterDecision::Needs(needs);
                }
            }
        }

        PartialFilterDecision::Ready(self.default_decision())
    }
}

/// Visibility resolution table (see `docs/filter_spec/loot-filter-spec.md`).
fn resolve_visibility(rule_vis: Visibility, hide_all: bool) -> Visibility {
    match (rule_vis, hide_all) {
        (Visibility::Show, _) => Visibility::Show,
        (Visibility::Hide, _) => Visibility::Hide,
        (Visibility::Default, false) => Visibility::Default,
        (Visibility::Default, true) => Visibility::Hide,
    }
}
