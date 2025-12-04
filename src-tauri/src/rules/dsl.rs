//! DSL parser for D2Stats-style rule format
//!
//! # Syntax
//!
//! ```text
//! # Comment
//! "Item Pattern" [quality] [tier] [eth] [{stat pattern}] [color] [sound] [name] [stat]
//! ```
//!
//! ## Examples
//!
//! ```text
//! # Notify on all unique items
//! "." unique gold sound1
//!
//! # Hide normal items
//! "." normal hide
//!
//! # Notify on rings with +skills
//! "Ring$" unique {Skills} gold sound1 stat
//!
//! # Ethereal sacred items
//! "." sacred eth gold sound1
//! ```

use super::{FilterConfig, ItemQuality, ItemTier, NotifyColor, Rule};
use serde::{Deserialize, Serialize};

/// Error that occurred during DSL parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}: {}", self.line, self.message)
    }
}

/// Validation error (less severe than parse error)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

/// Parse DSL text into FilterConfig
pub fn parse_dsl(text: &str) -> Result<FilterConfig, Vec<ParseError>> {
    let mut rules = Vec::new();
    let mut errors = Vec::new();

    for (line_num, line) in text.lines().enumerate() {
        let line_num = line_num + 1; // 1-indexed

        // Skip empty lines and comments
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        match parse_line(trimmed, line_num) {
            Ok(rule) => rules.push(rule),
            Err(e) => errors.push(e),
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(FilterConfig {
        name: "Parsed Filter".to_string(),
        default_show_items: true,
        default_notify: false,
        rules,
        dsl_source: Some(text.to_string()),
    })
}

/// Parse a single DSL line into a Rule
fn parse_line(line: &str, line_num: usize) -> Result<Rule, ParseError> {
    let mut rule = Rule::default();
    rule.source_line = Some(line.to_string());

    let mut chars = line.chars().peekable();
    let mut pos = 0;

    // Skip leading whitespace
    while chars.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
        chars.next();
        pos += 1;
    }

    // Parse item name pattern (in quotes)
    if chars.peek() == Some(&'"') {
        chars.next(); // consume opening quote
        pos += 1;

        let mut pattern = String::new();
        let mut found_closing = false;

        while let Some(c) = chars.next() {
            pos += 1;
            if c == '"' {
                found_closing = true;
                break;
            }
            pattern.push(c);
        }

        if !found_closing {
            return Err(ParseError {
                line: line_num,
                column: pos,
                message: "Unclosed quote in item pattern".to_string(),
            });
        }

        if !pattern.is_empty() && pattern != "." {
            rule.name_pattern = Some(pattern);
        }
    }

    // Parse flags and stat patterns
    let remaining: String = chars.collect();
    let mut stat_pattern: Option<String> = None;

    // Extract stat pattern from {braces}
    let remaining = extract_stat_pattern(&remaining, &mut stat_pattern);
    rule.stat_pattern = stat_pattern;

    // Parse remaining tokens
    for token in remaining.split_whitespace() {
        let token_lower = token.to_lowercase();

        // Quality flags
        if let Some(quality) = ItemQuality::from_str(&token_lower) {
            rule.item_quality = quality as i32;
            continue;
        }

        // Tier flags
        if let Some(tier) = ItemTier::from_str(&token_lower) {
            rule.tier = Some(tier as i32);
            continue;
        }

        // Color flags
        if let Some(color) = NotifyColor::from_str(&token_lower) {
            match color {
                NotifyColor::Hide => {
                    rule.show_item = false;
                    rule.color = Some("hide".to_string());
                }
                NotifyColor::Show => {
                    rule.show_item = true;
                    rule.color = Some("show".to_string());
                }
                _ => {
                    rule.color = color.to_dsl_str().map(|s| s.to_string());
                }
            }
            continue;
        }

        // Ethereal flag
        if token_lower == "eth" {
            rule.ethereal = 1; // Required
            continue;
        }

        // Sound flags (sound1, sound2, ... sound6, sound_none)
        if token_lower == "sound_none" {
            rule.sound = Some(0);
            continue;
        }
        if token_lower.starts_with("sound") {
            if let Ok(num) = token_lower[5..].parse::<u8>() {
                if (1..=6).contains(&num) {
                    rule.sound = Some(num);
                    rule.notify = true; // Sound implies notify
                }
            }
            continue;
        }

        // Display flags
        if token_lower == "name" {
            rule.display_name = true;
            continue;
        }
        if token_lower == "stat" {
            rule.display_stats = true;
            continue;
        }

        // Unknown token - could be a warning but we'll be lenient
    }

    // If we have a sound or explicit notify color, enable notify
    if rule.sound.is_some() || rule.color.as_ref().map(|c| c != "hide").unwrap_or(false) {
        rule.notify = true;
    }

    Ok(rule)
}

/// Extract stat pattern from {braces} and return remaining string
fn extract_stat_pattern(s: &str, pattern: &mut Option<String>) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    let mut in_braces = false;
    let mut brace_content = String::new();

    while let Some(c) = chars.next() {
        if c == '{' && !in_braces {
            in_braces = true;
            brace_content.clear();
        } else if c == '}' && in_braces {
            in_braces = false;
            if !brace_content.trim().is_empty() {
                *pattern = Some(brace_content.trim().to_string());
            }
            brace_content.clear();
        } else if in_braces {
            brace_content.push(c);
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert FilterConfig back to DSL text
pub fn to_dsl(config: &FilterConfig) -> String {
    // If we have the original source, return it
    if let Some(ref source) = config.dsl_source {
        return source.clone();
    }

    let mut lines = Vec::new();

    // Add header comment
    lines.push(format!("# {}", config.name));
    lines.push(String::new());

    for rule in &config.rules {
        if !rule.active {
            continue;
        }

        let line = rule_to_dsl(rule);
        lines.push(line);
    }

    lines.join("\n")
}

/// Convert a single Rule to DSL line
fn rule_to_dsl(rule: &Rule) -> String {
    // If we have the original source line, use it
    if let Some(ref source) = rule.source_line {
        return source.clone();
    }

    let mut parts = Vec::new();

    // Name pattern
    let pattern = rule.name_pattern.as_deref().unwrap_or(".");
    parts.push(format!("\"{}\"", pattern));

    // Quality
    if rule.item_quality > 0 {
        let quality = match rule.item_quality {
            1 => "low",
            2 => "normal",
            3 => "superior",
            4 => "magic",
            5 => "set",
            6 => "rare",
            7 => "unique",
            8 => "craft",
            9 => "honor",
            _ => "",
        };
        if !quality.is_empty() {
            parts.push(quality.to_string());
        }
    }

    // Tier
    if let Some(tier) = rule.tier {
        let tier_str = match tier {
            0 => "0",
            1 => "1",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "sacred",
            6 => "angelic",
            7 => "master",
            _ => "",
        };
        if !tier_str.is_empty() {
            parts.push(tier_str.to_string());
        }
    }

    // Ethereal
    if rule.ethereal == 1 {
        parts.push("eth".to_string());
    }

    // Stat pattern
    if let Some(ref stat_pattern) = rule.stat_pattern {
        parts.push(format!("{{{}}}", stat_pattern));
    }

    // Color
    if let Some(ref color) = rule.color {
        parts.push(color.clone());
    } else if !rule.show_item {
        parts.push("hide".to_string());
    }

    // Sound
    if let Some(sound) = rule.sound {
        if sound == 0 {
            parts.push("sound_none".to_string());
        } else if sound <= 6 {
            parts.push(format!("sound{}", sound));
        }
    }

    // Display flags
    if rule.display_name {
        parts.push("name".to_string());
    }
    if rule.display_stats {
        parts.push("stat".to_string());
    }

    parts.join(" ")
}

/// Validate DSL without full parsing (for editor feedback)
pub fn validate_dsl(text: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for (line_num, line) in text.lines().enumerate() {
        let line_num = line_num + 1;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Check for unclosed quotes
        let quote_count = trimmed.chars().filter(|&c| c == '"').count();
        if quote_count % 2 != 0 {
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message: "Unclosed quote".to_string(),
                severity: ValidationSeverity::Error,
            });
        }

        // Check for unclosed braces
        let open_braces = trimmed.chars().filter(|&c| c == '{').count();
        let close_braces = trimmed.chars().filter(|&c| c == '}').count();
        if open_braces != close_braces {
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message: "Mismatched braces".to_string(),
                severity: ValidationSeverity::Error,
            });
        }

        // Check for unknown flags
        let remaining = extract_stat_pattern(trimmed, &mut None);
        // Remove the quoted part
        let remaining = if let Some(start) = remaining.find('"') {
            if let Some(end) = remaining[start + 1..].find('"') {
                format!("{}{}", &remaining[..start], &remaining[start + end + 2..])
            } else {
                remaining
            }
        } else {
            remaining
        };

        for token in remaining.split_whitespace() {
            let token_lower = token.to_lowercase();

            // Skip known tokens
            if ItemQuality::from_str(&token_lower).is_some()
                || ItemTier::from_str(&token_lower).is_some()
                || NotifyColor::from_str(&token_lower).is_some()
                || token_lower == "eth"
                || token_lower == "name"
                || token_lower == "stat"
                || token_lower == "sound_none"
                || (token_lower.starts_with("sound") && token_lower[5..].parse::<u8>().is_ok())
            {
                continue;
            }

            // Unknown token
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message: format!("Unknown flag: {}", token),
                severity: ValidationSeverity::Warning,
            });
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rule() {
        let dsl = r#""Ring$" unique gold sound1"#;
        let config = parse_dsl(dsl).unwrap();

        assert_eq!(config.rules.len(), 1);
        let rule = &config.rules[0];
        assert_eq!(rule.name_pattern, Some("Ring$".to_string()));
        assert_eq!(rule.item_quality, 7); // Unique
        assert_eq!(rule.color, Some("gold".to_string()));
        assert_eq!(rule.sound, Some(1));
        assert!(rule.notify);
    }

    #[test]
    fn test_parse_with_stat_pattern() {
        let dsl = r#""Amulet" rare {[3-5] to All Skills}"#;
        let config = parse_dsl(dsl).unwrap();

        assert_eq!(config.rules.len(), 1);
        let rule = &config.rules[0];
        assert_eq!(rule.name_pattern, Some("Amulet".to_string()));
        assert_eq!(rule.item_quality, 6); // Rare
        assert_eq!(rule.stat_pattern, Some("[3-5] to All Skills".to_string()));
    }

    #[test]
    fn test_parse_hide_rule() {
        let dsl = r#""." normal hide"#;
        let config = parse_dsl(dsl).unwrap();

        assert_eq!(config.rules.len(), 1);
        let rule = &config.rules[0];
        assert_eq!(rule.name_pattern, None); // "." means match all
        assert_eq!(rule.item_quality, 2); // Normal
        assert!(!rule.show_item);
    }

    #[test]
    fn test_parse_ethereal_sacred() {
        let dsl = r#""." sacred eth gold sound1"#;
        let config = parse_dsl(dsl).unwrap();

        let rule = &config.rules[0];
        assert_eq!(rule.tier, Some(5)); // Sacred
        assert_eq!(rule.ethereal, 1); // Required
        assert_eq!(rule.color, Some("gold".to_string()));
    }

    #[test]
    fn test_parse_with_comments() {
        let dsl = r#"
# This is a comment
"Ring$" unique gold sound1

# Another comment
"Amulet" set lime sound2
"#;
        let config = parse_dsl(dsl).unwrap();
        assert_eq!(config.rules.len(), 2);
    }

    #[test]
    fn test_parse_display_flags() {
        let dsl = r#""." unique gold name stat"#;
        let config = parse_dsl(dsl).unwrap();

        let rule = &config.rules[0];
        assert!(rule.display_name);
        assert!(rule.display_stats);
    }

    #[test]
    fn test_to_dsl_roundtrip() {
        let original = r#"# Parsed Filter

"Ring$" unique {Skills} gold sound1
"." normal hide"#;

        let config = parse_dsl(original).unwrap();
        let regenerated = to_dsl(&config);

        // Should preserve original source
        assert_eq!(regenerated, original);
    }

    #[test]
    fn test_validate_unclosed_quote() {
        let dsl = r#""Ring$ unique"#;
        let errors = validate_dsl(dsl);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.message.contains("quote")));
    }

    #[test]
    fn test_validate_mismatched_braces() {
        let dsl = r#""Ring" {Skills"#;
        let errors = validate_dsl(dsl);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.message.contains("braces")));
    }

    #[test]
    fn test_validate_unknown_flag() {
        let dsl = r#""Ring" unique unknownflag gold"#;
        let errors = validate_dsl(dsl);
        assert!(errors.iter().any(|e| e.message.contains("Unknown")));
    }
}

