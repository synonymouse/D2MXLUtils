//! JSON-based item filtering rules
//!
//! Example config:
//! ```json
//! {
//!   "default_show_items": true,
//!   "name": "SimpleFilterSoftNotify",
//!   "rules": [
//!     {
//!       "active": true,
//!       "automap": false,
//!       "ethereal": 0,
//!       "item_quality": 1,
//!       "max_clvl": 0,
//!       "max_ilvl": 0,
//!       "min_clvl": 0,
//!       "min_ilvl": 0,
//!       "notify": false,
//!       "params": {"class": 25},
//!       "rule_type": 0,
//!       "show_item": false
//!     }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::notifier::ItemDropEvent;
use crate::offsets::item_quality;

/// Rule type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    /// Match by item class (params.class)
    Class = 0,
    /// Match by quality only
    Quality = 1,
    /// Match by name substring
    Name = 2,
    /// Match all items
    All = 3,
}

impl Default for RuleType {
    fn default() -> Self {
        RuleType::Class
    }
}

/// Ethereal match mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtherealMode {
    /// Don't care about ethereal status
    Any = 0,
    /// Must be ethereal
    Required = 1,
    /// Must NOT be ethereal
    Forbidden = 2,
}

impl From<i32> for EtherealMode {
    fn from(v: i32) -> Self {
        match v {
            1 => EtherealMode::Required,
            2 => EtherealMode::Forbidden,
            _ => EtherealMode::Any,
        }
    }
}

/// Parameters for rule matching
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleParams {
    /// Item class ID (for RuleType::Class)
    #[serde(default)]
    pub class: Option<u32>,
    
    /// Item name substring (for RuleType::Name)
    #[serde(default)]
    pub name: Option<String>,
    
    /// Stat ID to match (for stat-based rules)
    #[serde(default)]
    pub stat_id: Option<u32>,
    
    /// Minimum stat value
    #[serde(default)]
    pub stat_min: Option<i32>,
    
    /// Maximum stat value
    #[serde(default)]
    pub stat_max: Option<i32>,
}

/// Action to take when rule matches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAction {
    /// Show the item notification
    pub show_item: bool,
    /// Play notification sound
    pub notify: bool,
    /// Show on automap
    pub automap: bool,
    /// Custom color (optional)
    #[serde(default)]
    pub color: Option<String>,
    /// Custom sound file (optional)
    #[serde(default)]
    pub sound: Option<String>,
}

impl Default for RuleAction {
    fn default() -> Self {
        Self {
            show_item: true,
            notify: false,
            automap: false,
            color: None,
            sound: None,
        }
    }
}

/// A single filtering rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Whether this rule is active
    #[serde(default = "default_true")]
    pub active: bool,
    
    /// Rule type (how to match)
    #[serde(default)]
    pub rule_type: i32,
    
    /// Item quality to match (0 = any)
    #[serde(default)]
    pub item_quality: i32,
    
    /// Ethereal mode (0 = any, 1 = required, 2 = forbidden)
    #[serde(default)]
    pub ethereal: i32,
    
    /// Minimum item level (0 = any)
    #[serde(default)]
    pub min_ilvl: i32,
    
    /// Maximum item level (0 = any)
    #[serde(default)]
    pub max_ilvl: i32,
    
    /// Minimum character level (0 = any)
    #[serde(default)]
    pub min_clvl: i32,
    
    /// Maximum character level (0 = any)
    #[serde(default)]
    pub max_clvl: i32,
    
    /// Rule-specific parameters
    #[serde(default)]
    pub params: RuleParams,
    
    /// Show item notification
    #[serde(default = "default_true")]
    pub show_item: bool,
    
    /// Play notification sound
    #[serde(default)]
    pub notify: bool,
    
    /// Show on automap
    #[serde(default)]
    pub automap: bool,
    
    /// Custom display name for matched items
    #[serde(default)]
    pub display_name: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Rule {
    /// Check if this rule matches the given item
    pub fn matches(&self, item: &ItemDropEvent) -> bool {
        if !self.active {
            return false;
        }
        
        // Check quality
        if self.item_quality > 0 {
            let required_quality = match self.item_quality {
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
            if !required_quality.is_empty() && item.quality != required_quality {
                return false;
            }
        }
        
        // Check ethereal
        let eth_mode = EtherealMode::from(self.ethereal);
        match eth_mode {
            EtherealMode::Required if !item.is_ethereal => return false,
            EtherealMode::Forbidden if item.is_ethereal => return false,
            _ => {}
        }
        
        // Check rule-specific match
        match self.rule_type {
            0 => {
                // RuleType::Class - match by item class
                if let Some(class) = self.params.class {
                    if item.class != class {
                        return false;
                    }
                }
            }
            2 => {
                // RuleType::Name - match by name substring
                if let Some(ref name_pattern) = self.params.name {
                    if !item.name.to_lowercase().contains(&name_pattern.to_lowercase()) {
                        return false;
                    }
                }
            }
            3 => {
                // RuleType::All - matches everything (quality/ethereal already checked)
            }
            _ => {}
        }
        
        true
    }
    
    /// Get the action for this rule
    pub fn action(&self) -> RuleAction {
        RuleAction {
            show_item: self.show_item,
            notify: self.notify,
            automap: self.automap,
            color: None,
            sound: None,
        }
    }
}

/// Filter configuration containing multiple rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    /// Filter name
    #[serde(default)]
    pub name: String,
    
    /// Default behavior: show items that don't match any rule
    #[serde(default = "default_true")]
    pub default_show_items: bool,
    
    /// Default behavior: notify for items that don't match any rule
    #[serde(default)]
    pub default_notify: bool,
    
    /// List of rules
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            default_show_items: true,
            default_notify: false,
            rules: Vec::new(),
        }
    }
}

impl FilterConfig {
    /// Load config from a JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))
    }
    
    /// Save config to a JSON file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        
        fs::write(path.as_ref(), content)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }
    
    /// Find the first matching rule for an item
    pub fn find_matching_rule(&self, item: &ItemDropEvent) -> Option<&Rule> {
        self.rules.iter().find(|rule| rule.matches(item))
    }
    
    /// Determine what action to take for an item
    pub fn get_action(&self, item: &ItemDropEvent) -> RuleAction {
        if let Some(rule) = self.find_matching_rule(item) {
            rule.action()
        } else {
            // Default action when no rule matches
            RuleAction {
                show_item: self.default_show_items,
                notify: self.default_notify,
                automap: false,
                color: None,
                sound: None,
            }
        }
    }
}

/// Create a sample filter config for testing
pub fn create_sample_config() -> FilterConfig {
    FilterConfig {
        name: "SampleFilter".to_string(),
        default_show_items: true,
        default_notify: false,
        rules: vec![
            // Hide inferior items
            Rule {
                active: true,
                rule_type: 1, // Quality
                item_quality: item_quality::INFERIOR as i32,
                ethereal: 0,
                min_ilvl: 0,
                max_ilvl: 0,
                min_clvl: 0,
                max_clvl: 0,
                params: RuleParams::default(),
                show_item: false,
                notify: false,
                automap: false,
                display_name: None,
            },
            // Notify on unique items
            Rule {
                active: true,
                rule_type: 1, // Quality
                item_quality: item_quality::UNIQUE as i32,
                ethereal: 0,
                min_ilvl: 0,
                max_ilvl: 0,
                min_clvl: 0,
                max_clvl: 0,
                params: RuleParams::default(),
                show_item: true,
                notify: true,
                automap: true,
                display_name: None,
            },
            // Notify on set items
            Rule {
                active: true,
                rule_type: 1, // Quality
                item_quality: item_quality::SET as i32,
                ethereal: 0,
                min_ilvl: 0,
                max_ilvl: 0,
                min_clvl: 0,
                max_clvl: 0,
                params: RuleParams::default(),
                show_item: true,
                notify: true,
                automap: false,
                display_name: None,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rule_matching() {
        let item = ItemDropEvent {
            unit_id: 1,
            class: 25,
            quality: "Unique".to_string(),
            name: "Test Item".to_string(),
            stats: "".to_string(),
            is_ethereal: false,
            is_identified: true,
        };
        
        let rule = Rule {
            active: true,
            rule_type: 1,
            item_quality: 7, // Unique
            ethereal: 0,
            min_ilvl: 0,
            max_ilvl: 0,
            min_clvl: 0,
            max_clvl: 0,
            params: RuleParams::default(),
            show_item: true,
            notify: true,
            automap: false,
            display_name: None,
        };
        
        assert!(rule.matches(&item));
    }
}

