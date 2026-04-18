//! Loot filter rule engine (spec-aligned).
//!
//! See `docs/filter_spec/` for the authoritative DSL and semantics.
//!
//! # Data model
//!
//! - [`FilterConfig`] owns a flat list of [`Rule`]s and a `hide_all` flag.
//!   `hide_all` is set by the file-scope `hide default` / `show default`
//!   directive (absent = `show default`).
//! - [`Rule::visibility`] is `Default` / `Show` / `Hide`. There are no
//!   `"hide"` / `"show"` pseudo-colors anymore — they live on `visibility`.
//! - Notifications only fire when `rule.notify == true`. `color` / `sound`
//!   alone never imply a notification.
//! - Rule selection is **last-match wins** (source order). There is no
//!   priority / flag-count tie-breaking.

mod dsl;
mod matching;

pub use dsl::{parse_dsl, validate_dsl, ParseError, ValidationError, ValidationSeverity};
pub use matching::MatchContext;

use serde::{Deserialize, Serialize};

// =====================================================================
// Enums
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemQuality {
    #[default]
    Any,
    Inferior,
    Normal,
    Superior,
    Magic,
    Set,
    Rare,
    Unique,
    Crafted,
    Honorific,
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

    /// Canonical name emitted by the scanner in [`ItemDropEvent::quality`].
    pub fn d2_quality_name(&self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Inferior => Some("Inferior"),
            Self::Normal => Some("Normal"),
            Self::Superior => Some("Superior"),
            Self::Magic => Some("Magic"),
            Self::Set => Some("Set"),
            Self::Rare => Some("Rare"),
            Self::Unique => Some("Unique"),
            Self::Crafted => Some("Crafted"),
            Self::Honorific => Some("Honorific"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemTier {
    #[default]
    Any,
    Tier0,
    Tier1,
    Tier2,
    Tier3,
    Tier4,
    Sacred,
    Angelic,
    Master,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyColor {
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
            _ => None,
        }
    }

    pub fn to_hex(&self) -> &'static str {
        match self {
            Self::Transparent => "#00000000",
            Self::White => "#FFFFFF",
            Self::Red => "#FF0000",
            Self::Lime => "#15FF00",
            Self::Blue => "#7878F5",
            Self::Gold => "#F0CD8C",
            Self::Grey => "#9D9D9D",
            Self::Black => "#000000",
            Self::Pink => "#FF00FF",
            Self::Orange => "#FFBF00",
            Self::Yellow => "#FFFF00",
            Self::Green => "#008000",
            Self::Purple => "#9D00FF",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Default,
    Show,
    Hide,
}

// =====================================================================
// Rule
// =====================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_pattern: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stat_pattern: Option<String>,

    #[serde(default, skip_serializing_if = "is_any_quality")]
    pub quality: ItemQuality,

    #[serde(default, skip_serializing_if = "is_any_tier")]
    pub tier: ItemTier,

    #[serde(default, skip_serializing_if = "is_false")]
    pub ethereal: bool,

    #[serde(default, skip_serializing_if = "is_default_visibility")]
    pub visibility: Visibility,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<NotifyColor>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<u8>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub notify: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub display_name: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub display_stats: bool,
}

fn is_any_quality(q: &ItemQuality) -> bool {
    *q == ItemQuality::Any
}

fn is_any_tier(t: &ItemTier) -> bool {
    *t == ItemTier::Any
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default_visibility(v: &Visibility) -> bool {
    *v == Visibility::Default
}

// =====================================================================
// FilterDecision
// =====================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<NotifyColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound: Option<u8>,
    pub display_name: bool,
    pub display_stats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterDecision {
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<Notification>,
}

// =====================================================================
// FilterConfig
// =====================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default)]
    pub name: String,

    #[serde(default, skip_serializing_if = "is_false")]
    pub hide_all: bool,

    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            hide_all: false,
            rules: Vec::new(),
        }
    }
}

impl FilterConfig {
    /// Decide what to do with an item: last-match wins, per spec.
    pub fn decide(&self, ctx: &MatchContext) -> FilterDecision {
        let winner = self.rules.iter().rev().find(|r| ctx.matches(r));
        match winner {
            None => FilterDecision {
                visibility: if self.hide_all {
                    Visibility::Hide
                } else {
                    Visibility::Default
                },
                notification: None,
            },
            Some(rule) => FilterDecision {
                visibility: resolve_visibility(rule.visibility, self.hide_all),
                notification: if rule.notify {
                    Some(Notification {
                        color: rule.color,
                        sound: rule.sound,
                        display_name: rule.display_name,
                        display_stats: rule.display_stats,
                    })
                } else {
                    None
                },
            },
        }
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

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifier::ItemDropEvent;

    fn item(name: &str, quality: ItemQuality, eth: bool) -> ItemDropEvent {
        ItemDropEvent {
            unit_id: 1,
            class: 0,
            quality: quality.d2_quality_name().unwrap_or("").to_string(),
            name: name.to_string(),
            stats: String::new(),
            is_ethereal: eth,
            is_identified: true,
            p_unit_data: 0,
            tier: None,
            filter: None,
        }
    }

    #[test]
    fn last_match_wins() {
        let config = FilterConfig {
            rules: vec![
                Rule {
                    quality: ItemQuality::Unique,
                    color: Some(NotifyColor::Gold),
                    ..Rule::default()
                },
                Rule {
                    name_pattern: Some("Ring$".into()),
                    quality: ItemQuality::Unique,
                    color: Some(NotifyColor::Red),
                    notify: true,
                    ..Rule::default()
                },
            ],
            ..FilterConfig::default()
        };

        let amulet = item("Unique Amulet", ItemQuality::Unique, false);
        let ctx = MatchContext::new(&amulet);
        let d = config.decide(&ctx);
        assert!(d.notification.is_none());

        let ring = item("Stone of Jordan Ring", ItemQuality::Unique, false);
        let ctx = MatchContext::new(&ring);
        let d = config.decide(&ctx);
        let n = d.notification.expect("ring rule should notify");
        assert_eq!(n.color, Some(NotifyColor::Red));
    }

    #[test]
    fn notify_is_independent_of_color_and_sound() {
        let config = FilterConfig {
            rules: vec![Rule {
                quality: ItemQuality::Unique,
                color: Some(NotifyColor::Gold),
                sound: Some(1),
                // no notify!
                ..Rule::default()
            }],
            ..FilterConfig::default()
        };
        let it = item("Unique Boots", ItemQuality::Unique, false);
        let ctx = MatchContext::new(&it);
        let d = config.decide(&ctx);
        assert!(d.notification.is_none());
    }

    #[test]
    fn hide_all_with_no_match_hides() {
        let config = FilterConfig {
            hide_all: true,
            rules: vec![],
            ..FilterConfig::default()
        };
        let it = item("Magic Sword", ItemQuality::Magic, false);
        let ctx = MatchContext::new(&it);
        let d = config.decide(&ctx);
        assert_eq!(d.visibility, Visibility::Hide);
    }

    #[test]
    fn show_overrides_hide_all() {
        let config = FilterConfig {
            hide_all: true,
            rules: vec![Rule {
                quality: ItemQuality::Unique,
                visibility: Visibility::Show,
                ..Rule::default()
            }],
            ..FilterConfig::default()
        };
        let it = item("Unique Ring", ItemQuality::Unique, false);
        let ctx = MatchContext::new(&it);
        let d = config.decide(&ctx);
        assert_eq!(d.visibility, Visibility::Show);
    }

    #[test]
    fn quality_parsing() {
        assert_eq!(ItemQuality::from_str("unique"), Some(ItemQuality::Unique));
        assert_eq!(ItemQuality::from_str("RARE"), Some(ItemQuality::Rare));
        assert_eq!(ItemQuality::from_str("craft"), Some(ItemQuality::Crafted));
        assert_eq!(ItemQuality::from_str("invalid"), None);
    }
}
