//! DSL parser and serializer for the loot filter.
//!
//! Grammar (see `docs/filter_spec/loot-filter-dsl.md`):
//!
//! ```text
//! filter      := line*
//! line        := blank | comment | rule | group_open | group_close
//! comment     := '#' any*
//! rule        := [name] attr*
//! group_open  := '[' attr* ']' '{'
//! group_close := '}'
//! name        := '"' regex '"'
//! ```
//!
//! The parser is intentionally lenient: unknown tokens produce a
//! [`ValidationError::Warning`] but do not abort parsing, so an editor can
//! still render and reason about partially-typed rules.

use super::{FilterConfig, ItemQuality, ItemTier, NotifyColor, Rule, Visibility};
use serde::{Deserialize, Serialize};

// =====================================================================
// Error types
// =====================================================================

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

// =====================================================================
// Shared attribute bag
// =====================================================================

/// `Option` distinguishes "unset" (inherit from group) from "explicitly set".
#[derive(Debug, Clone, Default)]
struct Attrs {
    stat_pattern: Option<String>,
    qualities: Option<Vec<ItemQuality>>,
    tiers: Option<Vec<ItemTier>>,
    ethereal: Option<bool>,
    visibility: Option<Visibility>,
    color: Option<NotifyColor>,
    sound: Option<u8>,
    notify: Option<bool>,
    display_stats: Option<bool>,
}

impl Attrs {
    fn apply_to(&self, rule: &mut Rule) {
        if let Some(ref s) = self.stat_pattern {
            rule.stat_pattern = Some(s.clone());
        }
        if let Some(ref q) = self.qualities {
            rule.qualities = q.clone();
        }
        if let Some(ref t) = self.tiers {
            rule.tiers = t.clone();
        }
        if let Some(e) = self.ethereal {
            rule.ethereal = e;
        }
        if let Some(v) = self.visibility {
            rule.visibility = v;
        }
        if let Some(c) = self.color {
            rule.color = Some(c);
        }
        if let Some(s) = self.sound {
            rule.sound = Some(s);
        }
        if let Some(n) = self.notify {
            rule.notify = n;
        }
        if let Some(ds) = self.display_stats {
            rule.display_stats = ds;
        }
    }

    /// Merge `group` into `self` only where `self` is unset. Used to flatten
    /// `[header] { rule }` bodies: rule-level values win over the header.
    fn fill_from_group(&mut self, group: &Attrs) {
        if self.stat_pattern.is_none() {
            self.stat_pattern = group.stat_pattern.clone();
        }
        if self.qualities.is_none() {
            self.qualities = group.qualities.clone();
        }
        if self.tiers.is_none() {
            self.tiers = group.tiers.clone();
        }
        if self.ethereal.is_none() {
            self.ethereal = group.ethereal;
        }
        if self.visibility.is_none() {
            self.visibility = group.visibility;
        }
        if self.color.is_none() {
            self.color = group.color;
        }
        if self.sound.is_none() {
            self.sound = group.sound;
        }
        if self.notify.is_none() {
            self.notify = group.notify;
        }
        if self.display_stats.is_none() {
            self.display_stats = group.display_stats;
        }
    }
}

// =====================================================================
// Public API
// =====================================================================

/// Parse DSL text into a [`FilterConfig`].
///
/// Returns the flattened rule list (groups already expanded, source order
/// preserved). Parse errors abort; unknown tokens become warnings but still
/// produce a rule so the editor stays responsive.
pub fn parse_dsl(text: &str) -> Result<FilterConfig, Vec<ParseError>> {
    let mut rules: Vec<Rule> = Vec::new();
    let mut errors: Vec<ParseError> = Vec::new();
    let mut current_group: Option<(Attrs, usize)> = None;
    let mut hide_all = false;
    let mut default_mode_line: Option<usize> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = strip_inline_comment(line).trim();

        if trimmed.is_empty() {
            continue;
        }

        // Group closer
        if trimmed == "}" {
            if current_group.is_none() {
                errors.push(ParseError {
                    line: line_num,
                    column: 0,
                    message: "Unexpected '}' outside of a group".to_string(),
                });
            }
            current_group = None;
            continue;
        }

        // File-scope directive: `hide default` / `show default`.
        match parse_default_mode(trimmed) {
            DefaultModeParse::NotDirective => {}
            DefaultModeParse::ExtraTokens(keyword) => {
                errors.push(ParseError {
                    line: line_num,
                    column: 0,
                    message: format!(
                        "'{} default' is a file-scope directive and cannot have additional tokens",
                        keyword
                    ),
                });
                continue;
            }
            DefaultModeParse::Directive(mode) => {
                if current_group.is_some() {
                    errors.push(ParseError {
                        line: line_num,
                        column: 0,
                        message: "'hide default' / 'show default' cannot appear inside a group"
                            .to_string(),
                    });
                    continue;
                }
                if default_mode_line.is_some() {
                    errors.push(ParseError {
                        line: line_num,
                        column: 0,
                        message: "Duplicate 'hide default' / 'show default' directive".to_string(),
                    });
                    continue;
                }
                hide_all = mode;
                default_mode_line = Some(line_num);
                continue;
            }
        }

        // Group opener: [attrs] {
        if let Some(header_src) = parse_group_open(trimmed) {
            if current_group.is_some() {
                errors.push(ParseError {
                    line: line_num,
                    column: 0,
                    message: "Nested groups are not allowed".to_string(),
                });
                continue;
            }
            let mut attrs = Attrs::default();
            parse_attrs_into(
                header_src,
                &mut attrs,
                /*in_group_header=*/ true,
                line_num,
                &mut errors,
            );
            current_group = Some((attrs, line_num));
            continue;
        }

        // Regular rule line
        match parse_rule_line(trimmed, line_num) {
            Ok(mut rule) => {
                if let Some((ref group_attrs, _)) = current_group {
                    // The rule's Attrs representation is whatever it set
                    // during parsing. We need to merge the group over the
                    // un-set fields. Easiest: build an Attrs from the rule,
                    // fill from group, then re-apply to a fresh rule.
                    let mut merged = attrs_from_rule(&rule);
                    merged.fill_from_group(group_attrs);
                    let mut fresh = Rule::default();
                    fresh.name_pattern = rule.name_pattern.take();
                    merged.apply_to(&mut fresh);
                    rules.push(fresh);
                } else {
                    rules.push(rule);
                }
            }
            Err(e) => errors.push(e),
        }
    }

    if let Some((_, opened_line)) = current_group {
        errors.push(ParseError {
            line: opened_line,
            column: 0,
            message: "Unterminated group (missing '}')".to_string(),
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(FilterConfig {
        name: "Parsed Filter".to_string(),
        hide_all,
        rules,
    })
}

/// Validate DSL text without building a FilterConfig. Produces warnings for
/// unknown tokens, bracket mismatches, and common mistakes (e.g. color/sound
/// without `notify`).
pub fn validate_dsl(text: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut in_group = false;
    let mut group_open_line = 0usize;
    let mut default_mode_line: Option<usize> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = strip_inline_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "}" {
            if !in_group {
                errors.push(ValidationError {
                    line: line_num,
                    column: 0,
                    message: "Unexpected '}' outside of a group".to_string(),
                    severity: ValidationSeverity::Error,
                });
            }
            in_group = false;
            continue;
        }

        // File-scope directive: `hide default` / `show default`.
        match parse_default_mode(trimmed) {
            DefaultModeParse::NotDirective => {}
            DefaultModeParse::ExtraTokens(keyword) => {
                errors.push(ValidationError {
                    line: line_num,
                    column: 0,
                    message: format!(
                        "'{} default' is a file-scope directive and cannot have additional tokens",
                        keyword
                    ),
                    severity: ValidationSeverity::Error,
                });
                continue;
            }
            DefaultModeParse::Directive(_) => {
                if in_group {
                    errors.push(ValidationError {
                        line: line_num,
                        column: 0,
                        message: "'hide default' / 'show default' cannot appear inside a group"
                            .to_string(),
                        severity: ValidationSeverity::Error,
                    });
                    continue;
                }
                if default_mode_line.is_some() {
                    errors.push(ValidationError {
                        line: line_num,
                        column: 0,
                        message: "Duplicate 'hide default' / 'show default' directive".to_string(),
                        severity: ValidationSeverity::Error,
                    });
                    continue;
                }
                default_mode_line = Some(line_num);
                continue;
            }
        }

        if let Some(header) = parse_group_open(trimmed) {
            if in_group {
                errors.push(ValidationError {
                    line: line_num,
                    column: 0,
                    message: "Nested groups are not allowed".to_string(),
                    severity: ValidationSeverity::Error,
                });
            }
            in_group = true;
            group_open_line = line_num;
            validate_tokens(
                header,
                line_num,
                /*in_group_header=*/ true,
                &mut errors,
            );
            continue;
        }

        // Basic lexical sanity on the line.
        let quote_count = trimmed.chars().filter(|&c| c == '"').count();
        if quote_count % 2 != 0 {
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message: "Unclosed quote".to_string(),
                severity: ValidationSeverity::Error,
            });
            continue;
        }

        let opens = trimmed.chars().filter(|&c| c == '{').count();
        let closes = trimmed.chars().filter(|&c| c == '}').count();
        if opens != closes {
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message: "Mismatched braces".to_string(),
                severity: ValidationSeverity::Error,
            });
        }

        // Strip quoted name pattern and stat-pattern braces, then scan flags.
        let (after_name, _name_ok) = strip_leading_name(trimmed);
        let after_braces = strip_stat_brace(after_name);
        validate_tokens(&after_braces, line_num, false, &mut errors);

        // Info: color/sound present without notify is legal but usually a mistake.
        info_warn_notify_independence(&after_braces, line_num, &mut errors);
    }

    if in_group {
        errors.push(ValidationError {
            line: group_open_line,
            column: 0,
            message: "Unterminated group (missing '}')".to_string(),
            severity: ValidationSeverity::Error,
        });
    }

    errors
}

// =====================================================================
// Line-level parsing
// =====================================================================

fn parse_rule_line(trimmed: &str, line_num: usize) -> Result<Rule, ParseError> {
    let mut rule = Rule::default();

    let (after_name, name_pattern) = extract_name_pattern(trimmed, line_num)?;
    rule.name_pattern = name_pattern;

    let mut attrs = Attrs::default();
    let mut errors = Vec::new();
    parse_attrs_into(&after_name, &mut attrs, false, line_num, &mut errors);
    if let Some(first) = errors.into_iter().next() {
        return Err(first);
    }
    attrs.apply_to(&mut rule);
    Ok(rule)
}

/// Split `"name" rest` into `(rest, Some(pattern))`. A `.` pattern means
/// "match any" and is treated as if omitted. Returns the whole string as
/// `rest` when no quoted prefix is present.
fn extract_name_pattern(s: &str, line_num: usize) -> Result<(String, Option<String>), ParseError> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return Ok((s.to_string(), None));
    }
    let after_open = &s[1..];
    let close = after_open.find('"').ok_or_else(|| ParseError {
        line: line_num,
        column: 0,
        message: "Unclosed quote in item pattern".to_string(),
    })?;
    let pattern = &after_open[..close];
    let rest = &after_open[close + 1..];
    let name = if pattern.is_empty() || pattern == "." {
        None
    } else {
        Some(pattern.to_string())
    };
    Ok((rest.to_string(), name))
}

fn parse_attrs_into(
    src: &str,
    attrs: &mut Attrs,
    in_group_header: bool,
    line_num: usize,
    errors: &mut Vec<ParseError>,
) {
    if in_group_header && src.contains('"') {
        errors.push(ParseError {
            line: line_num,
            column: 0,
            message: "Group headers cannot contain a name pattern".to_string(),
        });
        return;
    }

    // Pull out stat pattern from any {...} occurrence.
    let (remainder, stat) = extract_stat_pattern(src);
    if let Some(s) = stat {
        attrs.stat_pattern = Some(s);
    }

    for token in remainder.split_whitespace() {
        let lower = token.to_lowercase();

        if let Some(q) = ItemQuality::from_str(&lower) {
            let set = attrs.qualities.get_or_insert_with(Vec::new);
            if !set.contains(&q) {
                set.push(q);
            }
            continue;
        }
        if let Some(t) = ItemTier::from_str(&lower) {
            let set = attrs.tiers.get_or_insert_with(Vec::new);
            if !set.contains(&t) {
                set.push(t);
            }
            continue;
        }
        match lower.as_str() {
            "eth" => {
                attrs.ethereal = Some(true);
                continue;
            }
            "show" => {
                attrs.visibility = Some(Visibility::Show);
                continue;
            }
            "hide" => {
                attrs.visibility = Some(Visibility::Hide);
                continue;
            }
            "notify" => {
                attrs.notify = Some(true);
                continue;
            }
            "stat" => {
                attrs.display_stats = Some(true);
                continue;
            }
            // `Some(0)` = silence marker; normalized to `None` in
            // `FilterConfig::decide`. Lets a rule override group-level sound.
            "sound_none" => {
                attrs.sound = Some(0);
                continue;
            }
            _ => {}
        }
        if let Some(c) = NotifyColor::from_str(&lower) {
            attrs.color = Some(c);
            continue;
        }
        if let Some(num) = parse_sound_keyword(&lower) {
            attrs.sound = Some(num);
            continue;
        }
        // Unknown tokens are lenient — see `validate_dsl` for warnings.
    }
}

fn parse_sound_keyword(lower: &str) -> Option<u8> {
    if !lower.starts_with("sound") {
        return None;
    }
    let suffix = &lower[5..];
    let n: u8 = suffix.parse().ok()?;
    if (1..=6).contains(&n) {
        Some(n)
    } else {
        None
    }
}

fn parse_group_open(trimmed: &str) -> Option<&str> {
    // Shape: `[ ... ] {`   (trailing `{` required)
    if !trimmed.starts_with('[') {
        return None;
    }
    let close = trimmed.find(']')?;
    let after = trimmed[close + 1..].trim_start();
    if after != "{" {
        return None;
    }
    Some(trimmed[1..close].trim())
}

fn strip_inline_comment(line: &str) -> &str {
    // Simple rule: `#` only starts a comment when it's not inside quotes or braces.
    let mut in_quote = false;
    let mut in_brace = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' if !in_brace => in_quote = !in_quote,
            b'{' if !in_quote => in_brace = true,
            b'}' if !in_quote => in_brace = false,
            b'#' if !in_quote && !in_brace => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// Extract the first `{...}` from `s`. Braces are balanced (so regex
/// quantifiers like `{n,m}` survive); `\{` / `\}` / `\\` escape literals.
fn extract_stat_pattern(s: &str) -> (String, Option<String>) {
    let mut remainder = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut pattern = None;
    let mut captured = false;

    while let Some(c) = chars.next() {
        if c == '{' && !captured {
            let mut inner = String::new();
            let mut depth = 1usize;
            while let Some(nc) = chars.next() {
                if nc == '\\' {
                    inner.push(nc);
                    if let Some(escaped) = chars.next() {
                        inner.push(escaped);
                    }
                    continue;
                }
                if nc == '{' {
                    depth += 1;
                    inner.push(nc);
                    continue;
                }
                if nc == '}' {
                    depth -= 1;
                    if depth == 0 {
                        if !inner.trim().is_empty() {
                            pattern = Some(inner.trim().to_string());
                        }
                        break;
                    }
                    inner.push(nc);
                    continue;
                }
                inner.push(nc);
            }
            captured = true;
        } else {
            remainder.push(c);
        }
    }

    (remainder, pattern)
}

fn strip_leading_name(s: &str) -> (String, bool) {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return (s.to_string(), true);
    }
    if let Some(end) = s[1..].find('"') {
        return (s[end + 2..].to_string(), true);
    }
    (s.to_string(), false)
}

fn strip_stat_brace(s: String) -> String {
    let (rest, _) = extract_stat_pattern(&s);
    rest
}

fn attrs_from_rule(rule: &Rule) -> Attrs {
    Attrs {
        stat_pattern: rule.stat_pattern.clone(),
        qualities: if rule.qualities.is_empty() {
            None
        } else {
            Some(rule.qualities.clone())
        },
        tiers: if rule.tiers.is_empty() {
            None
        } else {
            Some(rule.tiers.clone())
        },
        ethereal: if rule.ethereal { Some(true) } else { None },
        visibility: if rule.visibility == Visibility::Default {
            None
        } else {
            Some(rule.visibility)
        },
        color: rule.color,
        sound: rule.sound,
        notify: if rule.notify { Some(true) } else { None },
        display_stats: if rule.display_stats { Some(true) } else { None },
    }
}

// =====================================================================
// Validation helpers
// =====================================================================

fn validate_tokens(
    src: &str,
    line_num: usize,
    in_group_header: bool,
    errors: &mut Vec<ValidationError>,
) {
    if in_group_header && src.contains('"') {
        errors.push(ValidationError {
            line: line_num,
            column: 0,
            message: "Group headers cannot contain a name pattern".to_string(),
            severity: ValidationSeverity::Error,
        });
        return;
    }

    let (remainder, _) = extract_stat_pattern(src);
    for token in remainder.split_whitespace() {
        let lower = token.to_lowercase();
        if is_known_token(&lower) {
            continue;
        }
        errors.push(ValidationError {
            line: line_num,
            column: 0,
            message: format!("Unknown flag: {}", token),
            severity: ValidationSeverity::Warning,
        });
    }
}

fn is_known_token(lower: &str) -> bool {
    if ItemQuality::from_str(lower).is_some()
        || ItemTier::from_str(lower).is_some()
        || NotifyColor::from_str(lower).is_some()
    {
        return true;
    }
    matches!(
        lower,
        "eth" | "show" | "hide" | "notify" | "stat" | "sound_none"
    ) || parse_sound_keyword(lower).is_some()
}

fn info_warn_notify_independence(src: &str, line_num: usize, errors: &mut Vec<ValidationError>) {
    let (remainder, _) = extract_stat_pattern(src);
    let mut has_color = false;
    let mut has_sound = false;
    let mut has_notify = false;
    for token in remainder.split_whitespace() {
        let lower = token.to_lowercase();
        if NotifyColor::from_str(&lower).is_some() {
            has_color = true;
        } else if lower == "sound_none" || parse_sound_keyword(&lower).is_some() {
            has_sound = true;
        } else if lower == "notify" {
            has_notify = true;
        }
    }
    if (has_color || has_sound) && !has_notify {
        errors.push(ValidationError {
            line: line_num,
            column: 0,
            message: "color/sound without 'notify' produces no notification".to_string(),
            severity: ValidationSeverity::Info,
        });
    }
}

// =====================================================================
// Directive parsing
// =====================================================================

enum DefaultModeParse {
    NotDirective,
    /// `true` = `hide default`, `false` = `show default`.
    Directive(bool),
    /// Payload carries `"hide"` or `"show"` for the error message.
    ExtraTokens(&'static str),
}

fn parse_default_mode(trimmed: &str) -> DefaultModeParse {
    let lowered = trimmed.to_ascii_lowercase();
    let mut tokens = lowered.split_whitespace();
    let first = match tokens.next() {
        Some(t) => t.to_string(),
        None => return DefaultModeParse::NotDirective,
    };
    let second = match tokens.next() {
        Some(t) => t.to_string(),
        None => return DefaultModeParse::NotDirective,
    };
    if second != "default" {
        return DefaultModeParse::NotDirective;
    }
    let keyword: &'static str = match first.as_str() {
        "hide" => "hide",
        "show" => "show",
        _ => return DefaultModeParse::NotDirective,
    };
    if tokens.next().is_some() {
        return DefaultModeParse::ExtraTokens(keyword);
    }
    DefaultModeParse::Directive(keyword == "hide")
}

// =====================================================================
// Tests
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Visibility;

    #[test]
    fn parses_bare_quality_rule_without_quotes() {
        let cfg = parse_dsl("unique gold").unwrap();
        assert_eq!(cfg.rules.len(), 1);
        let r = &cfg.rules[0];
        assert_eq!(r.name_pattern, None);
        assert_eq!(r.qualities, vec![ItemQuality::Unique]);
        assert_eq!(r.color, Some(NotifyColor::Gold));
        assert!(!r.notify);
    }

    #[test]
    fn notify_is_not_auto_set_from_color_or_sound() {
        let cfg = parse_dsl("\"Ring$\" unique gold sound1").unwrap();
        assert!(!cfg.rules[0].notify);
        assert_eq!(cfg.rules[0].color, Some(NotifyColor::Gold));
        assert_eq!(cfg.rules[0].sound, Some(1));
    }

    #[test]
    fn explicit_notify_sets_flag() {
        let cfg = parse_dsl("\"Ring$\" unique gold notify sound1").unwrap();
        assert!(cfg.rules[0].notify);
    }

    #[test]
    fn hide_show_goes_to_visibility_not_color() {
        let cfg = parse_dsl("normal hide").unwrap();
        let r = &cfg.rules[0];
        assert_eq!(r.visibility, Visibility::Hide);
        assert!(r.color.is_none());

        let cfg = parse_dsl("unique show gold").unwrap();
        let r = &cfg.rules[0];
        assert_eq!(r.visibility, Visibility::Show);
        assert_eq!(r.color, Some(NotifyColor::Gold));
    }

    #[test]
    fn group_flattens_and_merges_header_into_rules() {
        let src = r#"[unique gold notify sound1] {
  "Jordan"
  "Tyrael"
  "Windforce"
}"#;
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(cfg.rules.len(), 3);
        for r in &cfg.rules {
            assert_eq!(r.qualities, vec![ItemQuality::Unique]);
            assert_eq!(r.color, Some(NotifyColor::Gold));
            assert_eq!(r.sound, Some(1));
            assert!(r.notify);
        }
        assert_eq!(cfg.rules[0].name_pattern.as_deref(), Some("Jordan"));
        assert_eq!(cfg.rules[2].name_pattern.as_deref(), Some("Windforce"));
    }

    #[test]
    fn rule_overrides_group_visibility() {
        let src = r#"[hide] {
  normal
  unique show gold notify
}"#;
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(cfg.rules[0].visibility, Visibility::Hide);
        assert_eq!(cfg.rules[1].visibility, Visibility::Show);
        assert_eq!(cfg.rules[1].color, Some(NotifyColor::Gold));
    }

    #[test]
    fn nested_groups_rejected() {
        let src = r#"[unique] {
  [gold] {
    "X"
  }
}"#;
        assert!(parse_dsl(src).is_err());
    }

    #[test]
    fn unterminated_group_rejected() {
        let src = r#"[unique gold] {
  "Jordan"
"#;
        assert!(parse_dsl(src).is_err());
    }

    #[test]
    fn empty_dot_name_is_match_all() {
        let cfg = parse_dsl("\".\" gold notify").unwrap();
        assert_eq!(cfg.rules[0].name_pattern, None);
    }

    #[test]
    fn inline_comment_stripped() {
        let cfg = parse_dsl("unique gold notify  # highlight").unwrap();
        assert!(cfg.rules[0].notify);
    }

    #[test]
    fn validator_warns_on_unknown_flag() {
        let errors = validate_dsl("unique wat");
        assert!(errors.iter().any(|e| e.message.contains("Unknown flag")));
    }

    #[test]
    fn validator_warns_on_removed_name_flag() {
        let errors = validate_dsl("unique notify name");
        assert!(errors
            .iter()
            .any(|e| e.severity == ValidationSeverity::Warning
                && e.message.contains("Unknown flag: name")));
    }

    #[test]
    fn validator_info_on_color_without_notify() {
        let errors = validate_dsl("unique gold");
        assert!(errors
            .iter()
            .any(|e| e.severity == ValidationSeverity::Info && e.message.contains("notify")));
    }

    #[test]
    fn stat_pattern_extraction_handles_escapes() {
        let cfg = parse_dsl("rare {test\\}inside}").unwrap();
        assert_eq!(cfg.rules[0].stat_pattern.as_deref(), Some("test\\}inside"));
    }

    #[test]
    fn parses_hide_default_directive() {
        let cfg = parse_dsl("hide default\nunique gold notify").unwrap();
        assert!(cfg.hide_all);
        assert_eq!(cfg.rules.len(), 1);
    }

    #[test]
    fn parses_show_default_directive() {
        let cfg = parse_dsl("show default\nunique gold notify").unwrap();
        assert!(!cfg.hide_all);
        assert_eq!(cfg.rules.len(), 1);
    }

    #[test]
    fn absent_directive_defaults_to_show() {
        let cfg = parse_dsl("unique gold notify").unwrap();
        assert!(!cfg.hide_all);
    }

    #[test]
    fn directive_position_in_file_is_free() {
        let cfg = parse_dsl("unique gold notify\nhide default\nrare lime notify").unwrap();
        assert!(cfg.hide_all);
        assert_eq!(cfg.rules.len(), 2);
    }

    #[test]
    fn duplicate_default_directive_is_error() {
        let errs = parse_dsl("hide default\nshow default").unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("Duplicate")));
    }

    #[test]
    fn directive_inside_group_is_error() {
        let src = "[unique] {\n  hide default\n  \"X\"\n}";
        let errs = parse_dsl(src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("inside a group")));
    }

    #[test]
    fn validator_flags_duplicate_directive() {
        let errors = validate_dsl("hide default\nshow default");
        assert!(errors
            .iter()
            .any(|e| e.severity == ValidationSeverity::Error && e.message.contains("Duplicate")));
    }

    #[test]
    fn multi_tier_tokens_accumulate_into_set() {
        let cfg = parse_dsl("1 2 3 4 hide").unwrap();
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(
            cfg.rules[0].tiers,
            vec![
                ItemTier::Tier1,
                ItemTier::Tier2,
                ItemTier::Tier3,
                ItemTier::Tier4,
            ]
        );
        assert_eq!(cfg.rules[0].visibility, Visibility::Hide);
        assert!(cfg.rules[0].qualities.is_empty());
    }

    #[test]
    fn multi_quality_tokens_accumulate_into_set() {
        let cfg = parse_dsl("magic rare unique hide").unwrap();
        assert_eq!(
            cfg.rules[0].qualities,
            vec![ItemQuality::Magic, ItemQuality::Rare, ItemQuality::Unique]
        );
        assert_eq!(cfg.rules[0].visibility, Visibility::Hide);
    }

    #[test]
    fn mixed_multi_tier_and_quality_rule() {
        let cfg = parse_dsl("1 2 3 4 unique hide").unwrap();
        assert_eq!(
            cfg.rules[0].tiers,
            vec![
                ItemTier::Tier1,
                ItemTier::Tier2,
                ItemTier::Tier3,
                ItemTier::Tier4,
            ]
        );
        assert_eq!(cfg.rules[0].qualities, vec![ItemQuality::Unique]);
        assert_eq!(cfg.rules[0].visibility, Visibility::Hide);
    }

    #[test]
    fn duplicate_tier_tokens_are_deduplicated() {
        let cfg = parse_dsl("1 1 2 2 hide").unwrap();
        assert_eq!(cfg.rules[0].tiers, vec![ItemTier::Tier1, ItemTier::Tier2]);
    }

    #[test]
    fn group_header_with_quoted_name_emits_single_error() {
        let src = "[\"Stone of Jordan\" unique gold] {\n  \"X\"\n}";
        let errs = parse_dsl(src).unwrap_err();
        let name_errs: Vec<_> = errs
            .iter()
            .filter(|e| e.message.contains("Group headers cannot contain a name pattern"))
            .collect();
        assert_eq!(name_errs.len(), 1);
    }

    #[test]
    fn stat_pattern_allows_regex_quantifier() {
        let cfg = parse_dsl("rare {All Skills.{2,5}}").unwrap();
        assert_eq!(
            cfg.rules[0].stat_pattern.as_deref(),
            Some("All Skills.{2,5}")
        );
    }

    #[test]
    fn hide_default_with_extras_is_error() {
        let errs = parse_dsl("hide default unique").unwrap_err();
        assert!(errs
            .iter()
            .any(|e| e.message.contains("cannot have additional tokens")));
    }
}
