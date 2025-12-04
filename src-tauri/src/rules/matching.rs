//! Rule matching logic

use regex::Regex;

use super::{EtherealMode, Rule};
use crate::notifier::ItemDropEvent;

/// Context for matching rules against items
/// Holds compiled regex patterns for efficient repeated matching
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

    /// Check if a rule matches the item in this context
    pub fn matches(&self, rule: &Rule) -> bool {
        if !rule.active {
            return false;
        }

        // Check quality
        if rule.item_quality > 0 {
            let required_quality = match rule.item_quality {
                1 => "Inferior",
                2 => "Normal",
                3 => "Superior",
                4 => "Magic",
                5 => "Set",
                6 => "Rare",
                7 => "Unique",
                8 => "Crafted",
                9 => "Honorific",
                _ => "",
            };
            if !required_quality.is_empty()
                && !self.item.quality.eq_ignore_ascii_case(required_quality)
            {
                return false;
            }
        }

        // Check ethereal
        let eth_mode = EtherealMode::from(rule.ethereal);
        match eth_mode {
            EtherealMode::Required if !self.item.is_ethereal => return false,
            EtherealMode::Forbidden if self.item.is_ethereal => return false,
            _ => {}
        }

        // Check name pattern (regex)
        if let Some(ref pattern) = rule.name_pattern {
            match Regex::new(&format!("(?i){}", pattern)) {
                Ok(re) => {
                    if !re.is_match(&self.name_lower) {
                        return false;
                    }
                }
                Err(_) => {
                    // Invalid regex, try simple substring match
                    if !self.name_lower.contains(&pattern.to_lowercase()) {
                        return false;
                    }
                }
            }
        }

        // Check stat pattern (regex)
        if let Some(ref pattern) = rule.stat_pattern {
            match Regex::new(&format!("(?i){}", pattern)) {
                Ok(re) => {
                    if !re.is_match(&self.stats_lower) {
                        return false;
                    }
                }
                Err(_) => {
                    // Invalid regex, try simple substring match
                    if !self.stats_lower.contains(&pattern.to_lowercase()) {
                        return false;
                    }
                }
            }
        }

        // Check legacy rule types for backwards compatibility
        match rule.rule_type {
            0 => {
                // RuleType::Class - match by item class
                if let Some(class) = rule.params.class {
                    if self.item.class != class {
                        return false;
                    }
                }
            }
            2 => {
                // RuleType::Name - match by name substring (legacy)
                if let Some(ref name_substr) = rule.params.name {
                    if !self.name_lower.contains(&name_substr.to_lowercase()) {
                        return false;
                    }
                }
            }
            3 => {
                // RuleType::All - matches everything (other checks already done)
            }
            _ => {}
        }

        // TODO: Check tier when we have tier info from items
        // TODO: Check ilvl/clvl when we have that info

        true
    }
}

/// Compile a regex pattern, returning None if invalid
pub fn compile_pattern(pattern: &str) -> Option<Regex> {
    Regex::new(&format!("(?i){}", pattern)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(name: &str, quality: &str, stats: &str, ethereal: bool) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: 1,
            class: 25,
            quality: quality.to_string(),
            name: name.to_string(),
            stats: stats.to_string(),
            is_ethereal: ethereal,
            is_identified: true,
        }
    }

    #[test]
    fn test_name_pattern_matching() {
        let item = make_item("Stone of Jordan", "Unique", "+1 to All Skills", false);
        let ctx = MatchContext::new(&item);

        let rule = Rule {
            name_pattern: Some("Jordan$".to_string()),
            ..Default::default()
        };
        assert!(ctx.matches(&rule));

        let rule = Rule {
            name_pattern: Some("Ring$".to_string()),
            ..Default::default()
        };
        assert!(!ctx.matches(&rule));
    }

    #[test]
    fn test_quality_matching() {
        let item = make_item("Test Ring", "Unique", "", false);
        let ctx = MatchContext::new(&item);

        let rule = Rule {
            item_quality: 7, // Unique
            ..Default::default()
        };
        assert!(ctx.matches(&rule));

        let rule = Rule {
            item_quality: 6, // Rare
            ..Default::default()
        };
        assert!(!ctx.matches(&rule));
    }

    #[test]
    fn test_stat_pattern_matching() {
        let item = make_item("Ring", "Unique", "+3 to All Skills\n+15% Faster Cast Rate", false);
        let ctx = MatchContext::new(&item);

        let rule = Rule {
            stat_pattern: Some("All Skills".to_string()),
            ..Default::default()
        };
        assert!(ctx.matches(&rule));

        let rule = Rule {
            stat_pattern: Some(r"\+\d+ to All Skills".to_string()),
            ..Default::default()
        };
        assert!(ctx.matches(&rule));

        let rule = Rule {
            stat_pattern: Some("Life Stolen".to_string()),
            ..Default::default()
        };
        assert!(!ctx.matches(&rule));
    }

    #[test]
    fn test_ethereal_matching() {
        let eth_item = make_item("Eth Sword", "Unique", "", true);
        let normal_item = make_item("Sword", "Unique", "", false);

        let eth_ctx = MatchContext::new(&eth_item);
        let normal_ctx = MatchContext::new(&normal_item);

        // eth required
        let rule = Rule {
            ethereal: 1,
            ..Default::default()
        };
        assert!(eth_ctx.matches(&rule));
        assert!(!normal_ctx.matches(&rule));

        // eth forbidden
        let rule = Rule {
            ethereal: 2,
            ..Default::default()
        };
        assert!(!eth_ctx.matches(&rule));
        assert!(normal_ctx.matches(&rule));
    }

    #[test]
    fn test_combined_matching() {
        let item = make_item(
            "Stone of Jordan",
            "Unique",
            "+1 to All Skills\n+25% Lightning Resist",
            false,
        );
        let ctx = MatchContext::new(&item);

        let rule = Rule {
            name_pattern: Some("Jordan".to_string()),
            item_quality: 7,
            stat_pattern: Some("Skills".to_string()),
            ..Default::default()
        };
        assert!(ctx.matches(&rule));

        // Fail on quality
        let rule = Rule {
            name_pattern: Some("Jordan".to_string()),
            item_quality: 6, // Rare, but item is Unique
            stat_pattern: Some("Skills".to_string()),
            ..Default::default()
        };
        assert!(!ctx.matches(&rule));
    }
}

