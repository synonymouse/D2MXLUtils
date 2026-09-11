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

mod decision;
mod dsl;
mod explain;
mod matching;

pub use dsl::{parse_dsl, validate_dsl, ParseError, ValidationError, ValidationSeverity};
pub use explain::explain_line;
pub use matching::MatchContext;

use serde::{Deserialize, Serialize};

use regex::Regex;

pub use crate::notifier::UniqueKind;

/// Parse DSL text into FilterConfig JSON
#[tauri::command]
pub(crate) fn parse_filter_dsl(text: String) -> Result<FilterConfig, Vec<ParseError>> {
    parse_dsl(&text)
}

/// Validate DSL text and return errors/warnings
#[tauri::command]
pub(crate) fn validate_filter_dsl(text: String) -> Vec<ValidationError> {
    validate_dsl(&text)
}

/// Plain-English explanation for a single rule line, used by the
/// editor's hover tooltip. Returns `None` for blank lines, comments,
/// group close `}`, and unparseable input.
#[tauri::command]
pub(crate) fn explain_filter_line(line: String) -> Option<String> {
    explain_line(&line)
}

/// Resolve the filter decision for a hypothetical item. Used by the UI
/// to preview what the current filter would do without actually dropping
/// anything in-game. See `docs/filter-preview-todo.md` for the planned UI
/// scenarios built around this command.
#[tauri::command]
pub(crate) fn get_item_filter_action(
    mut config: FilterConfig,
    item: crate::notifier::ItemDropEvent,
) -> FilterDecision {
    use crate::rules::MatchContext;
    config.prepare_for_matching();
    let ctx = MatchContext::new(&item);
    config.decide(&ctx)
}

// =====================================================================
// Enums
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemQuality {
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
    pub fn d2_quality_name(&self) -> &'static str {
        match self {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemTier {
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

/// Character class of the player currently in-game. Matched against
/// `UnitAny.class` (offset `unit::CLASS`) read from the player's own unit —
/// the same struct field D2 reuses for item file-index on item units.
/// Ids empirically confirmed live: `2` = Necromancer, matching the standard
/// D2 class ordering (Amazon, Sorceress, Necromancer, Paladin, Barbarian,
/// Druid, Assassin) used by the `.d2s` save format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerClass {
    Amazon,
    Sorceress,
    Necromancer,
    Paladin,
    Barbarian,
    Druid,
    Assassin,
}

impl PlayerClass {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "amazon" | "zon" => Some(Self::Amazon),
            "sorceress" | "sorc" => Some(Self::Sorceress),
            "necromancer" | "necro" => Some(Self::Necromancer),
            "paladin" | "pal" | "pally" => Some(Self::Paladin),
            "barbarian" | "barb" => Some(Self::Barbarian),
            "druid" | "dru" => Some(Self::Druid),
            "assassin" | "sin" => Some(Self::Assassin),
            _ => None,
        }
    }

    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Amazon),
            1 => Some(Self::Sorceress),
            2 => Some(Self::Necromancer),
            3 => Some(Self::Paladin),
            4 => Some(Self::Barbarian),
            5 => Some(Self::Druid),
            6 => Some(Self::Assassin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyColor {
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

    /// Stable lowercase keyword (matches the Serialize representation).
    /// Used by loot history to record the rule color without going through
    /// `serde_json::to_value`.
    pub fn lowercase_name(&self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Red => "red",
            Self::Lime => "lime",
            Self::Blue => "blue",
            Self::Gold => "gold",
            Self::Grey => "grey",
            Self::Black => "black",
            Self::Pink => "pink",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Purple => "purple",
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

#[derive(Debug, Clone)]
struct CompiledPattern {
    source: String,
    regex: Option<Regex>,
    fallback_lower: String,
}

impl CompiledPattern {
    fn new(pattern: &str) -> Self {
        Self {
            source: pattern.to_string(),
            regex: Regex::new(&format!("(?i){}", pattern)).ok(),
            fallback_lower: pattern.to_lowercase(),
        }
    }

    fn source_matches(&self, pattern: &str) -> bool {
        self.source == pattern
    }

    fn is_match(&self, haystack_lower: &str) -> bool {
        match &self.regex {
            Some(re) => re.is_match(haystack_lower),
            None => haystack_lower.contains(&self.fallback_lower),
        }
    }

    #[cfg(test)]
    fn is_regex(&self) -> bool {
        self.regex.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_pattern: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stat_patterns: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualities: Vec<ItemQuality>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiers: Vec<ItemTier>,

    /// Unique-item rarity tier (`tu`/`su`/`ssu`/`sssu`), independent of
    /// `qualities` — specifying one implies the item must be Unique quality
    /// with that wLvl band, so `sssu` alone is a valid, complete rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_kinds: Vec<UniqueKind>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sockets: Vec<u8>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<PlayerClass>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_clvl: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_clvl: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ilvl: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ilvl: Option<u32>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub ethereal: bool,

    /// Matched against `base_name`/`category` (Median XL's "Quest Item"
    /// items.txt type), the same mechanism the default profile already
    /// used for its `"Quest Item|Cube Reagent"` name-pattern rule — not a
    /// separate memory-read flag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub quest: bool,

    #[serde(default, skip_serializing_if = "is_default_visibility")]
    pub visibility: Visibility,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<NotifyColor>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<u8>,

    #[serde(default, skip_serializing_if = "is_false")]
    pub notify: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub display_stats: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub map: bool,

    /// 1-based source line this rule was parsed from (its own line inside a
    /// group, not the group header). Used to highlight the rule that fired
    /// for a drop in the editor; `0` for rules built outside the DSL parser
    /// (e.g. in tests).
    #[serde(default, skip_serializing_if = "is_default_source_line")]
    pub source_line: usize,

    #[serde(skip)]
    compiled_name_pattern: Option<CompiledPattern>,

    #[serde(skip)]
    compiled_stat_patterns: Vec<CompiledPattern>,
}

impl Rule {
    fn prepare_for_matching(&mut self) {
        self.compiled_name_pattern = self.name_pattern.as_deref().map(CompiledPattern::new);
        self.compiled_stat_patterns = self
            .stat_patterns
            .iter()
            .map(|pattern| CompiledPattern::new(pattern))
            .collect();
    }

    fn compiled_name_pattern(&self) -> Option<&CompiledPattern> {
        let pattern = self.name_pattern.as_deref()?;
        self.compiled_name_pattern
            .as_ref()
            .filter(|compiled| compiled.source_matches(pattern))
    }

    fn compiled_stat_patterns(&self) -> Option<&[CompiledPattern]> {
        if self.compiled_stat_patterns.len() != self.stat_patterns.len() {
            return None;
        }
        if self
            .compiled_stat_patterns
            .iter()
            .zip(&self.stat_patterns)
            .all(|(compiled, pattern)| compiled.source_matches(pattern))
        {
            Some(&self.compiled_stat_patterns)
        } else {
            None
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default_source_line(line: &usize) -> bool {
    *line == 0
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
    pub display_stats: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_stat_lines: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterDecision {
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<Notification>,
    /// `map` flag from the winning rule — drop an automap marker at the
    /// item's world position. Independent of `notify` on purpose: a silent
    /// map ping is a valid use case.
    #[serde(default, skip_serializing_if = "is_false")]
    pub place_on_map: bool,
    /// Source line of the rule that decided this outcome (`None` when no
    /// rule matched). Powers the "show matched lines" live highlight mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_line: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnrichmentNeeds {
    pub runtime_stats: bool,
}

impl EnrichmentNeeds {
    pub fn stats() -> Self {
        Self {
            runtime_stats: true,
        }
    }

    pub fn is_empty(self) -> bool {
        !self.runtime_stats
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialRuleMatch {
    Match,
    NoMatch,
    Needs(EnrichmentNeeds),
}

#[derive(Debug, Clone)]
pub enum PartialFilterDecision {
    Ready(FilterDecision),
    Needs(EnrichmentNeeds),
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

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests;
