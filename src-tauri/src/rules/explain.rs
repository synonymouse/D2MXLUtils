//! Plain-English explainer for a single line of the loot-filter DSL.

use super::dsl::{classify_line, ParsedLine};
use super::{ItemQuality, ItemTier, NotifyColor, Rule, UniqueKind, Visibility};

pub fn explain_line(line: &str) -> Option<String> {
    match classify_line(line) {
        ParsedLine::Empty | ParsedLine::GroupClose | ParsedLine::Unparseable => None,
        ParsedLine::Directive(hide) => Some(format_directive(hide)),
        ParsedLine::GroupHeader(rule) => Some(format_group_header(&rule)),
        ParsedLine::Rule(rule) => Some(format_rule(&rule)),
    }
}

// =====================================================================
// Top-level formatters
// =====================================================================

fn format_directive(hide: bool) -> String {
    if hide {
        "File directive: hide every item that no rule matches. Rules with 'show' override this for individual items.".to_string()
    } else {
        "File directive: defer to the game's built-in filter for items that no rule matches."
            .to_string()
    }
}

fn format_rule(rule: &Rule) -> String {
    let predicate_bullets = predicate_lines(rule);
    let action_bullets = action_lines(rule);

    let mut out = String::new();

    if predicate_bullets.is_empty() {
        out.push_str("Matches every item.");
    } else if predicate_bullets.len() == 1 {
        out.push_str("Matches when:\n");
        out.push_str("  • ");
        out.push_str(&predicate_bullets[0]);
        if let Some(note) = unrestricted_categories(rule) {
            out.push_str("\n\n");
            out.push_str(&note);
        }
    } else {
        out.push_str("Matches when ALL of these are true:");
        for bullet in &predicate_bullets {
            out.push_str("\n  • ");
            out.push_str(bullet);
        }
    }

    if !action_bullets.is_empty() {
        out.push_str("\n\nActions:");
        for bullet in &action_bullets {
            out.push_str("\n  • ");
            out.push_str(bullet);
        }
    }

    out
}

fn format_group_header(rule: &Rule) -> String {
    let mut bullets = predicate_lines(rule);
    bullets.extend(action_lines(rule));

    let mut out = String::from(
        "Group header — these defaults apply to every rule inside the braces (unless the rule overrides them):",
    );
    if bullets.is_empty() {
        out.push_str("\n  (no defaults set)");
    } else {
        for bullet in &bullets {
            out.push_str("\n  • ");
            out.push_str(bullet);
        }
    }
    out
}

// =====================================================================
// Predicate lines
// =====================================================================

fn predicate_lines(rule: &Rule) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(ref pat) = rule.name_pattern {
        out.push(format!("Name matches the pattern \"{}\"", pat));
    }
    if !rule.tiers.is_empty() {
        out.push(tier_bullet(&rule.tiers));
    }
    if !rule.qualities.is_empty() {
        out.push(quality_bullet(&rule.qualities));
    }
    if !rule.unique_kinds.is_empty() {
        out.push(unique_kind_bullet(&rule.unique_kinds));
    }
    if rule.ethereal {
        out.push("Item is ethereal".to_string());
    }
    if rule.quest {
        out.push("Item is a quest item".to_string());
    }
    if !rule.stat_patterns.is_empty() {
        out.push(stat_bullet(&rule.stat_patterns));
    }
    out
}

fn tier_bullet(tiers: &[ItemTier]) -> String {
    if tiers.len() == 1 {
        format!("Tier is {}", tier_label(tiers[0]))
    } else {
        let labels: Vec<&str> = tiers.iter().map(|t| tier_label(*t)).collect();
        format!("Tier is one of: {}", labels.join(", "))
    }
}

fn quality_bullet(qualities: &[ItemQuality]) -> String {
    if qualities.len() == 1 {
        format!("Quality is {}", quality_label(qualities[0]))
    } else {
        let labels: Vec<&str> = qualities.iter().map(|q| quality_label(*q)).collect();
        format!("Quality is one of: {}", labels.join(", "))
    }
}

fn unique_kind_bullet(kinds: &[UniqueKind]) -> String {
    if kinds.len() == 1 {
        format!("Rarity is {}", kinds[0].label())
    } else {
        let labels: Vec<&str> = kinds.iter().map(|k| k.label()).collect();
        format!("Rarity is one of: {}", labels.join(", "))
    }
}

fn stat_bullet(patterns: &[String]) -> String {
    if patterns.len() == 1 {
        format!("Has stat pattern: \"{}\"", patterns[0])
    } else {
        let quoted: Vec<String> = patterns.iter().map(|p| format!("\"{}\"", p)).collect();
        format!("Has all stat patterns: {}", quoted.join(", "))
    }
}

fn unrestricted_categories(rule: &Rule) -> Option<String> {
    let mut missing: Vec<&str> = Vec::new();
    if rule.name_pattern.is_none() {
        missing.push("name");
    }
    if rule.tiers.is_empty() {
        missing.push("tier");
    }
    if rule.qualities.is_empty() {
        missing.push("quality");
    }
    if rule.unique_kinds.is_empty() {
        missing.push("rarity");
    }
    if !rule.ethereal {
        missing.push("ethereal");
    }
    if !rule.quest {
        missing.push("quest");
    }
    if rule.stat_patterns.is_empty() {
        missing.push("stats");
    }
    let list = match missing.len() {
        0 => return None,
        1 => missing[0].to_string(),
        2 => format!("{} and {}", missing[0], missing[1]),
        _ => format!(
            "{}, and {}",
            missing[..missing.len() - 1].join(", "),
            missing.last().unwrap()
        ),
    };
    Some(format!("(Other categories — {} — are unrestricted.)", list))
}

// =====================================================================
// Effect lines
// =====================================================================

fn visibility_line(v: Visibility) -> Option<String> {
    match v {
        Visibility::Default => None,
        Visibility::Hide => Some("Hide the item on the ground".to_string()),
        Visibility::Show => {
            Some("Force-show the item (overrides the game's built-in hide)".to_string())
        }
    }
}

fn action_lines(rule: &Rule) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(line) = visibility_line(rule.visibility) {
        out.push(line);
    }
    if rule.notify {
        out.push(notification_bullet(rule));
    } else if rule.color.is_some() || rule.sound.is_some() {
        out.push(
            "Color/sound flags are set but no notification will fire — needs 'notify'.".to_string(),
        );
    }
    if rule.map {
        out.push("Drop a marker on the automap at the item's position".to_string());
    }
    if rule.display_stats && !rule.notify {
        out.push("'stat' flag is set but won't show — needs 'notify' to fire.".to_string());
    }
    out
}

fn notification_bullet(rule: &Rule) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = rule.color {
        parts.push(format!("color: {}", color_label(c)));
    }
    match rule.sound {
        Some(0) => parts.push("silent".to_string()),
        Some(n) => parts.push(format!("sound {}", n)),
        None => {}
    }
    if rule.display_stats || !rule.stat_patterns.is_empty() {
        parts.push("includes item stats".to_string());
    }
    if parts.is_empty() {
        "Show overlay notification".to_string()
    } else {
        format!("Show overlay notification ({})", parts.join(", "))
    }
}

// =====================================================================
// Token labels
// =====================================================================

fn quality_label(q: ItemQuality) -> &'static str {
    match q {
        ItemQuality::Inferior => "low",
        ItemQuality::Normal => "normal",
        ItemQuality::Superior => "superior",
        ItemQuality::Magic => "magic",
        ItemQuality::Set => "set",
        ItemQuality::Rare => "rare",
        ItemQuality::Unique => "unique",
        ItemQuality::Crafted => "crafted",
        ItemQuality::Honorific => "honorific",
    }
}

fn tier_label(t: ItemTier) -> &'static str {
    match t {
        ItemTier::Tier0 => "0",
        ItemTier::Tier1 => "1",
        ItemTier::Tier2 => "2",
        ItemTier::Tier3 => "3",
        ItemTier::Tier4 => "4",
        ItemTier::Sacred => "sacred",
        ItemTier::Angelic => "angelic",
        ItemTier::Master => "mastercrafted",
    }
}

fn color_label(c: NotifyColor) -> &'static str {
    match c {
        NotifyColor::White => "white",
        NotifyColor::Red => "red",
        NotifyColor::Lime => "lime",
        NotifyColor::Blue => "blue",
        NotifyColor::Gold => "gold",
        NotifyColor::Grey => "grey",
        NotifyColor::Black => "black",
        NotifyColor::Pink => "pink",
        NotifyColor::Orange => "orange",
        NotifyColor::Yellow => "yellow",
        NotifyColor::Green => "green",
        NotifyColor::Purple => "purple",
    }
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;
