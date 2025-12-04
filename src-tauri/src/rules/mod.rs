//! JSON-based item filtering rules with DSL support
//!
//! # Architecture
//!
//! - **JSON** is the storage/exchange format (compatible with game loot filter)
//! - **DSL** is the human-readable format shown in the editor
//!
//! # Example JSON config:
//! ```json
//! {
//!   "default_show_items": true,
//!   "name": "MyFilter",
//!   "rules": [
//!     {
//!       "active": true,
//!       "name_pattern": "Ring$",
//!       "item_quality": 7,
//!       "stat_pattern": "Skills",
//!       "show_item": true,
//!       "notify": true
//!     }
//!   ]
//! }
//! ```
//!
//! # Example DSL:
//! ```text
//! # Notify on unique rings with +skills
//! "Ring$" unique {Skills} gold sound1
//! ```

mod dsl;
mod matching;

pub use dsl::{parse_dsl, to_dsl, validate_dsl, ParseError, ValidationError};
pub use matching::MatchContext;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Rule type enum (for backwards compatibility with game format)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    /// Match by item class (params.class)
    #[default]
    Class = 0,
    /// Match by quality only
    Quality = 1,
    /// Match by name pattern (regex)
    Name = 2,
    /// Match all items
    All = 3,
}

/// Ethereal match mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EtherealMode {
    /// Don't care about ethereal status
    #[default]
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

/// Item quality values (matches D2 quality enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ItemQuality {
    #[default]
    Any = 0,
    Inferior = 1,
    Normal = 2,
    Superior = 3,
    Magic = 4,
    Set = 5,
    Rare = 6,
    Unique = 7,
    Crafted = 8,
    Honorific = 9,
}

impl ItemQuality {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" | "inferior" => Some(Self::Inferior),
            "normal" => Some(Self::Normal),
            "superior" => Some(Self::Superior),
            "magic" => Some(Self::Magic),
            "set" => Some(Self::Set),
            "rare" => Some(Self::Rare),
            "unique" => Some(Self::Unique),
            "craft" | "crafted" => Some(Self::Crafted),
            "honor" | "honorific" => Some(Self::Honorific),
            _ => None,
        }
    }

    pub fn to_dsl_str(&self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Inferior => Some("low"),
            Self::Normal => Some("normal"),
            Self::Superior => Some("superior"),
            Self::Magic => Some("magic"),
            Self::Set => Some("set"),
            Self::Rare => Some("rare"),
            Self::Unique => Some("unique"),
            Self::Crafted => Some("craft"),
            Self::Honorific => Some("honor"),
        }
    }

    pub fn to_display_str(&self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Inferior => "Inferior",
            Self::Normal => "Normal",
            Self::Superior => "Superior",
            Self::Magic => "Magic",
            Self::Set => "Set",
            Self::Rare => "Rare",
            Self::Unique => "Unique",
            Self::Crafted => "Crafted",
            Self::Honorific => "Honorific",
        }
    }
}

/// Item tier (MedianXL specific)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ItemTier {
    #[default]
    Any = -1,
    Tier0 = 0,
    Tier1 = 1,
    Tier2 = 2,
    Tier3 = 3,
    Tier4 = 4,
    Sacred = 5,
    Angelic = 6,
    Master = 7,
}

impl ItemTier {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "0" => Some(Self::Tier0),
            "1" => Some(Self::Tier1),
            "2" => Some(Self::Tier2),
            "3" => Some(Self::Tier3),
            "4" => Some(Self::Tier4),
            "sacred" => Some(Self::Sacred),
            "angelic" => Some(Self::Angelic),
            "master" | "mastercrafted" => Some(Self::Master),
            _ => None,
        }
    }

    pub fn to_dsl_str(&self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Tier0 => Some("0"),
            Self::Tier1 => Some("1"),
            Self::Tier2 => Some("2"),
            Self::Tier3 => Some("3"),
            Self::Tier4 => Some("4"),
            Self::Sacred => Some("sacred"),
            Self::Angelic => Some("angelic"),
            Self::Master => Some("master"),
        }
    }
}

/// Notification color
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotifyColor {
    #[default]
    Default,
    Transparent,
    White,
    Red,
    Lime,
    Blue,
    Gold,
    Grey,
    Black,
    Pink,
    Orange,
    Yellow,
    Green,
    Purple,
    /// Hide item (special color that means don't show)
    Hide,
    /// Show item explicitly
    Show,
}

impl NotifyColor {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "transparent" => Some(Self::Transparent),
            "white" => Some(Self::White),
            "red" => Some(Self::Red),
            "lime" => Some(Self::Lime),
            "blue" => Some(Self::Blue),
            "gold" => Some(Self::Gold),
            "grey" | "gray" => Some(Self::Grey),
            "black" => Some(Self::Black),
            "pink" => Some(Self::Pink),
            "orange" => Some(Self::Orange),
            "yellow" => Some(Self::Yellow),
            "green" => Some(Self::Green),
            "purple" => Some(Self::Purple),
            "hide" => Some(Self::Hide),
            "show" => Some(Self::Show),
            _ => None,
        }
    }

    pub fn to_dsl_str(&self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Transparent => Some("transparent"),
            Self::White => Some("white"),
            Self::Red => Some("red"),
            Self::Lime => Some("lime"),
            Self::Blue => Some("blue"),
            Self::Gold => Some("gold"),
            Self::Grey => Some("grey"),
            Self::Black => Some("black"),
            Self::Pink => Some("pink"),
            Self::Orange => Some("orange"),
            Self::Yellow => Some("yellow"),
            Self::Green => Some("green"),
            Self::Purple => Some("purple"),
            Self::Hide => Some("hide"),
            Self::Show => Some("show"),
        }
    }

    pub fn to_hex(&self) -> Option<&'static str> {
        match self {
            Self::Default | Self::Hide | Self::Show => None,
            Self::Transparent => Some("#00000000"),
            Self::White => Some("#FFFFFF"),
            Self::Red => Some("#FF0000"),
            Self::Lime => Some("#15FF00"),
            Self::Blue => Some("#7878F5"),
            Self::Gold => Some("#F0CD8C"),
            Self::Grey => Some("#9D9D9D"),
            Self::Black => Some("#000000"),
            Self::Pink => Some("#FF00FF"),
            Self::Orange => Some("#FFBF00"),
            Self::Yellow => Some("#FFFF00"),
            Self::Green => Some("#008000"),
            Self::Purple => Some("#9D00FF"),
        }
    }
}

/// Parameters for rule matching (for backwards compatibility)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleParams {
    /// Item class ID (for RuleType::Class)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<u32>,

    /// Item name substring (legacy, use name_pattern instead)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stat ID to match (for stat-based rules, legacy)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_id: Option<u32>,

    /// Minimum stat value
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_min: Option<i32>,

    /// Maximum stat value
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    /// Notification color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Sound number (1-6) or custom sound file
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

fn default_true() -> bool {
    true
}

/// A single filtering rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Whether this rule is active
    #[serde(default = "default_true")]
    pub active: bool,

    /// Rule type (how to match) - for backwards compatibility
    #[serde(default)]
    pub rule_type: i32,

    // ===== Matching criteria =====
    /// Regex pattern for item name (DSL: "pattern")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_pattern: Option<String>,

    /// Regex pattern for item stats (DSL: {pattern})
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_pattern: Option<String>,

    /// Item quality to match (0 = any)
    #[serde(default)]
    pub item_quality: i32,

    /// Item tier (MedianXL specific)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<i32>,

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

    /// Legacy rule-specific parameters
    #[serde(default, skip_serializing_if = "is_params_empty")]
    pub params: RuleParams,

    // ===== Actions =====
    /// Show item notification (DSL: show/hide via color)
    #[serde(default = "default_true")]
    pub show_item: bool,

    /// Play notification sound
    #[serde(default)]
    pub notify: bool,

    /// Show on automap
    #[serde(default)]
    pub automap: bool,

    /// Notification color (DSL: white, red, gold, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Sound number 1-6 or custom file (DSL: sound1, sound2, etc.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<u8>,

    /// Display item name in notification (DSL: name flag)
    #[serde(default)]
    pub display_name: bool,

    /// Display item stats in notification (DSL: stat flag)
    #[serde(default)]
    pub display_stats: bool,

    /// Original DSL line (for reference/debugging)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line: Option<String>,
}

fn is_params_empty(params: &RuleParams) -> bool {
    params.class.is_none()
        && params.name.is_none()
        && params.stat_id.is_none()
        && params.stat_min.is_none()
        && params.stat_max.is_none()
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            active: true,
            rule_type: 0,
            name_pattern: None,
            stat_pattern: None,
            item_quality: 0,
            tier: None,
            ethereal: 0,
            min_ilvl: 0,
            max_ilvl: 0,
            min_clvl: 0,
            max_clvl: 0,
            params: RuleParams::default(),
            show_item: true,
            notify: false,
            automap: false,
            color: None,
            sound: None,
            display_name: false,
            display_stats: false,
            source_line: None,
        }
    }
}

impl Rule {
    /// Get the action for this rule
    pub fn action(&self) -> RuleAction {
        RuleAction {
            show_item: self.show_item,
            notify: self.notify,
            automap: self.automap,
            color: self.color.clone(),
            sound: self.sound.map(|n| format!("sound{}", n)),
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

    /// Original DSL source (if parsed from DSL)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dsl_source: Option<String>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            default_show_items: true,
            default_notify: false,
            rules: Vec::new(),
            dsl_source: None,
        }
    }
}

impl FilterConfig {
    /// Load config from a JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config JSON: {}", e))
    }

    /// Save config to a JSON file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(path.as_ref(), content)
            .map_err(|e| format!("Failed to write config file: {}", e))
    }

    /// Determine what action to take for an item (using MatchContext)
    pub fn get_action(&self, ctx: &MatchContext) -> RuleAction {
        if let Some(rule) = self.rules.iter().find(|r| r.active && ctx.matches(r)) {
            rule.action()
        } else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_parsing() {
        assert_eq!(ItemQuality::from_str("unique"), Some(ItemQuality::Unique));
        assert_eq!(ItemQuality::from_str("RARE"), Some(ItemQuality::Rare));
        assert_eq!(ItemQuality::from_str("craft"), Some(ItemQuality::Crafted));
        assert_eq!(ItemQuality::from_str("invalid"), None);
    }

    #[test]
    fn test_tier_parsing() {
        assert_eq!(ItemTier::from_str("sacred"), Some(ItemTier::Sacred));
        assert_eq!(ItemTier::from_str("0"), Some(ItemTier::Tier0));
        assert_eq!(ItemTier::from_str("master"), Some(ItemTier::Master));
    }

    #[test]
    fn test_color_parsing() {
        assert_eq!(NotifyColor::from_str("gold"), Some(NotifyColor::Gold));
        assert_eq!(NotifyColor::from_str("hide"), Some(NotifyColor::Hide));
        assert_eq!(NotifyColor::from_str("grey"), Some(NotifyColor::Grey));
        assert_eq!(NotifyColor::from_str("gray"), Some(NotifyColor::Grey));
    }

    #[test]
    fn test_rule_serialization() {
        let rule = Rule {
            name_pattern: Some("Ring$".to_string()),
            item_quality: 7,
            stat_pattern: Some("Skills".to_string()),
            color: Some("gold".to_string()),
            sound: Some(1),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&rule).unwrap();
        let parsed: Rule = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name_pattern, Some("Ring$".to_string()));
        assert_eq!(parsed.item_quality, 7);
        assert_eq!(parsed.sound, Some(1));
    }
}

