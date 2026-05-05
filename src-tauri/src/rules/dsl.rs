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
    stat_patterns: Option<Vec<String>>,
    qualities: Option<Vec<ItemQuality>>,
    tiers: Option<Vec<ItemTier>>,
    sockets: Option<Vec<u8>>,
    ethereal: Option<bool>,
    visibility: Option<Visibility>,
    color: Option<NotifyColor>,
    sound: Option<u8>,
    notify: Option<bool>,
    display_stats: Option<bool>,
    map: Option<bool>,
}

impl Attrs {
    fn apply_to(&self, rule: &mut Rule) {
        if let Some(ref sp) = self.stat_patterns {
            rule.stat_patterns = sp.clone();
        }
        if let Some(ref q) = self.qualities {
            rule.qualities = q.clone();
        }
        if let Some(ref t) = self.tiers {
            rule.tiers = t.clone();
        }
        if let Some(ref s) = self.sockets {
            rule.sockets = s.clone();
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
        if let Some(m) = self.map {
            rule.map = m;
        }
    }

    /// Merge `group` into `self` only where `self` is unset. Used to flatten
    /// `[header] { rule }` bodies: rule-level values win over the header.
    fn fill_from_group(&mut self, group: &Attrs) {
        if self.stat_patterns.is_none() {
            self.stat_patterns = group.stat_patterns.clone();
        }
        if self.qualities.is_none() {
            self.qualities = group.qualities.clone();
        }
        if self.tiers.is_none() {
            self.tiers = group.tiers.clone();
        }
        if self.sockets.is_none() {
            self.sockets = group.sockets.clone();
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
        if self.map.is_none() {
            self.map = group.map;
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
    let mut group_flags = NotifyFlags::default();

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
            group_flags = NotifyFlags::default();
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
            group_flags = scan_notify_flags(header);
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

        let starts_with_quote = trimmed.starts_with('"');
        let (after_name, _name_ok) = strip_leading_name(trimmed);
        let after_braces = strip_stat_brace(after_name);

        if after_braces.contains('"') {
            let message = if starts_with_quote {
                "Only one name pattern is allowed per rule; extra \"...\" must be removed"
                    .to_string()
            } else {
                "Name pattern \"...\" must be the first token on the line (before quality/flags)"
                    .to_string()
            };
            errors.push(ValidationError {
                line: line_num,
                column: 0,
                message,
                severity: ValidationSeverity::Error,
            });
        }

        let cleaned = strip_quoted_segments(&after_braces);
        validate_tokens(&cleaned, line_num, false, &mut errors);

        // Info: color/sound present without notify is legal but usually a mistake.
        let inherited = if in_group {
            group_flags
        } else {
            NotifyFlags::default()
        };
        info_warn_notify_independence(&cleaned, line_num, inherited, &mut errors);
    }

    if in_group {
        errors.push(ValidationError {
            line: group_open_line,
            column: 0,
            message: "Unterminated group (missing '}')".to_string(),
            severity: ValidationSeverity::Error,
        });
    }

    let rules = collect_rules_with_lines(text);
    check_subsumption(&rules, &mut errors);

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

    let (remainder, stats) = extract_stat_patterns(src);
    if !stats.is_empty() {
        attrs.stat_patterns = Some(stats);
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
        if let Some(n) = parse_socket_keyword(&lower) {
            let set = attrs.sockets.get_or_insert_with(Vec::new);
            if !set.contains(&n) {
                set.push(n);
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
            "map" => {
                attrs.map = Some(true);
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
    let suffix = lower.strip_prefix("sound")?;
    suffix.parse::<u8>().ok().filter(|&n| n >= 1)
}

fn parse_socket_keyword(lower: &str) -> Option<u8> {
    let rest = lower.strip_prefix("sockets")?;
    let n: u8 = rest.parse().ok()?;
    if n <= 6 {
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

/// Extract every `{...}` from `s`, in source order. Braces are balanced
/// (so regex quantifiers like `{n,m}` survive as part of an outer pattern);
/// `\{` / `\}` / `\\` escape literals. Empty `{}` groups and unterminated
/// `{...<EOF>` are silently dropped (validator warns separately).
fn extract_stat_patterns(s: &str) -> (String, Vec<String>) {
    let mut remainder = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut patterns: Vec<String> = Vec::new();

    while let Some(c) = chars.next() {
        if c == '{' {
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
                        let trimmed = inner.trim();
                        if !trimmed.is_empty() {
                            patterns.push(trimmed.to_string());
                        }
                        break;
                    }
                    inner.push(nc);
                    continue;
                }
                inner.push(nc);
            }
        } else {
            remainder.push(c);
        }
    }

    (remainder, patterns)
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
    let (rest, _) = extract_stat_patterns(&s);
    rest
}

fn strip_quoted_segments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_quote = false;
    for c in s.chars() {
        if c == '"' {
            in_quote = !in_quote;
            out.push(' ');
            continue;
        }
        if !in_quote {
            out.push(c);
        }
    }
    out
}

fn attrs_from_rule(rule: &Rule) -> Attrs {
    Attrs {
        stat_patterns: if rule.stat_patterns.is_empty() {
            None
        } else {
            Some(rule.stat_patterns.clone())
        },
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
        sockets: if rule.sockets.is_empty() {
            None
        } else {
            Some(rule.sockets.clone())
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
        map: if rule.map { Some(true) } else { None },
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

    let (remainder, _) = extract_stat_patterns(src);
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
        || parse_socket_keyword(lower).is_some()
    {
        return true;
    }
    matches!(
        lower,
        "eth" | "show" | "hide" | "notify" | "stat" | "sound_none" | "map"
    ) || parse_sound_keyword(lower).is_some()
}

#[derive(Debug, Clone, Copy, Default)]
struct NotifyFlags {
    color: bool,
    sound: bool,
    notify: bool,
}

impl NotifyFlags {
    fn merge(self, other: NotifyFlags) -> NotifyFlags {
        NotifyFlags {
            color: self.color || other.color,
            sound: self.sound || other.sound,
            notify: self.notify || other.notify,
        }
    }
}

fn scan_notify_flags(src: &str) -> NotifyFlags {
    let (remainder, _) = extract_stat_patterns(src);
    let mut flags = NotifyFlags::default();
    for token in remainder.split_whitespace() {
        let lower = token.to_lowercase();
        if NotifyColor::from_str(&lower).is_some() {
            flags.color = true;
        } else if lower == "sound_none" || parse_sound_keyword(&lower).is_some() {
            flags.sound = true;
        } else if lower == "notify" {
            flags.notify = true;
        }
    }
    flags
}

fn info_warn_notify_independence(
    src: &str,
    line_num: usize,
    inherited: NotifyFlags,
    errors: &mut Vec<ValidationError>,
) {
    let effective = scan_notify_flags(src).merge(inherited);
    if (effective.color || effective.sound) && !effective.notify {
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
// Line classification (for the explainer module)
// =====================================================================

pub(super) enum ParsedLine {
    Empty,
    GroupClose,
    Directive(bool),
    GroupHeader(Rule),
    Rule(Rule),
    Unparseable,
}

pub(super) fn classify_line(line: &str) -> ParsedLine {
    let trimmed = strip_inline_comment(line).trim();
    if trimmed.is_empty() {
        return ParsedLine::Empty;
    }
    if trimmed == "}" {
        return ParsedLine::GroupClose;
    }
    if let DefaultModeParse::Directive(hide) = parse_default_mode(trimmed) {
        return ParsedLine::Directive(hide);
    }
    if let Some(header_src) = parse_group_open(trimmed) {
        let mut attrs = Attrs::default();
        let mut sink: Vec<ParseError> = Vec::new();
        parse_attrs_into(header_src, &mut attrs, true, 0, &mut sink);
        let mut rule = Rule::default();
        attrs.apply_to(&mut rule);
        return ParsedLine::GroupHeader(rule);
    }
    match parse_rule_line(trimmed, 0) {
        Ok(r) => ParsedLine::Rule(r),
        Err(_) => ParsedLine::Unparseable,
    }
}

// =====================================================================
// Subsumption analysis
// =====================================================================

fn collect_rules_with_lines(text: &str) -> Vec<(Rule, usize)> {
    let mut rules: Vec<(Rule, usize)> = Vec::new();
    let mut current_group: Option<Attrs> = None;

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;
        let trimmed = strip_inline_comment(line).trim();

        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "}" {
            current_group = None;
            continue;
        }
        match parse_default_mode(trimmed) {
            DefaultModeParse::NotDirective => {}
            DefaultModeParse::Directive(_) | DefaultModeParse::ExtraTokens(_) => continue,
        }

        if let Some(header_src) = parse_group_open(trimmed) {
            let mut attrs = Attrs::default();
            let mut _sink: Vec<ParseError> = Vec::new();
            parse_attrs_into(header_src, &mut attrs, true, line_num, &mut _sink);
            current_group = Some(attrs);
            continue;
        }

        let mut rule = match parse_rule_line(trimmed, line_num) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Some(ref group_attrs) = current_group {
            let mut merged = attrs_from_rule(&rule);
            merged.fill_from_group(group_attrs);
            let mut fresh = Rule::default();
            fresh.name_pattern = rule.name_pattern.take();
            merged.apply_to(&mut fresh);
            rules.push((fresh, line_num));
        } else {
            rules.push((rule, line_num));
        }
    }
    rules
}

fn rule_subsumes(later: &Rule, earlier: &Rule) -> bool {
    if !later.tiers.is_empty() {
        if earlier.tiers.is_empty() {
            return false;
        }
        if !earlier.tiers.iter().all(|t| later.tiers.contains(t)) {
            return false;
        }
    }
    if !later.qualities.is_empty() {
        if earlier.qualities.is_empty() {
            return false;
        }
        if !earlier
            .qualities
            .iter()
            .all(|q| later.qualities.contains(q))
        {
            return false;
        }
    }
    if later.ethereal && !earlier.ethereal {
        return false;
    }
    if let Some(ref l) = later.name_pattern {
        match &earlier.name_pattern {
            Some(e) if e == l => {}
            _ => return false,
        }
    }
    if !later
        .stat_patterns
        .iter()
        .all(|p| earlier.stat_patterns.contains(p))
    {
        return false;
    }
    true
}

fn effects_differ(a: &Rule, b: &Rule) -> bool {
    a.visibility != b.visibility
        || a.notify != b.notify
        || a.color != b.color
        || a.sound != b.sound
        || a.map != b.map
        || a.display_stats != b.display_stats
}

fn check_subsumption(rules: &[(Rule, usize)], errors: &mut Vec<ValidationError>) {
    for (i, (earlier, earlier_line)) in rules.iter().enumerate() {
        for (later, later_line) in rules.iter().skip(i + 1) {
            if rule_subsumes(later, earlier) && effects_differ(earlier, later) {
                errors.push(ValidationError {
                    line: *earlier_line,
                    column: 0,
                    message: format!(
                        "Shadowed by rule on line {} — its broader match overrides this rule's effect",
                        later_line
                    ),
                    severity: ValidationSeverity::Warning,
                });
                break;
            }
        }
    }
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
    fn validator_notify_inherited_from_group_header_suppresses_info() {
        let src = r#"[notify] {
  "Cycle"
  "Medium Cycle" sound1
  "Large Cycle" sound2
}"#;
        let errors = validate_dsl(src);
        assert!(errors
            .iter()
            .all(|e| !(e.severity == ValidationSeverity::Info && e.message.contains("notify"))));
    }

    #[test]
    fn validator_group_flags_clear_after_close() {
        let src = r#"[notify] {
  "Cycle"
}
"foo" sound1"#;
        let errors = validate_dsl(src);
        let infos: Vec<_> = errors
            .iter()
            .filter(|e| e.severity == ValidationSeverity::Info && e.message.contains("notify"))
            .collect();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].line, 4);
    }

    #[test]
    fn stat_pattern_extraction_handles_escapes() {
        let cfg = parse_dsl("rare {test\\}inside}").unwrap();
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec!["test\\}inside".to_string()]
        );
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
    fn multi_socket_tokens_accumulate_into_set() {
        let cfg = parse_dsl("sockets0 sockets4 sockets6 notify").unwrap();
        assert_eq!(cfg.rules[0].sockets, vec![0, 4, 6]);
        assert!(validate_dsl("sockets0 sockets4 sockets6 notify").is_empty());
    }

    #[test]
    fn socket_token_out_of_range_is_unknown() {
        let cfg = parse_dsl("sockets7 hide").unwrap();
        assert!(cfg.rules[0].sockets.is_empty());
        assert!(validate_dsl("sockets7 hide")
            .iter()
            .any(|w| w.message.contains("sockets7")));
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
            .filter(|e| {
                e.message
                    .contains("Group headers cannot contain a name pattern")
            })
            .collect();
        assert_eq!(name_errs.len(), 1);
    }

    #[test]
    fn stat_pattern_allows_regex_quantifier() {
        let cfg = parse_dsl("rare {All Skills.{2,5}}").unwrap();
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec!["All Skills.{2,5}".to_string()]
        );
    }

    #[test]
    fn parses_map_token() {
        let cfg = parse_dsl("unique map").unwrap();
        assert!(cfg.rules[0].map);
    }

    #[test]
    fn map_survives_group_flatten() {
        let src = r#"[unique map] {
  "Jordan"
  "Tyrael"
}"#;
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(cfg.rules.len(), 2);
        assert!(cfg.rules.iter().all(|r| r.map));
    }

    #[test]
    fn validator_accepts_map() {
        let errors = validate_dsl("unique map notify");
        assert!(
            errors.iter().all(|e| !e.message.contains("Unknown flag")),
            "`map` should not be an unknown token: {:?}",
            errors
        );
    }

    #[test]
    fn map_serializes_only_when_true() {
        use super::super::Rule;
        let r = Rule {
            map: false,
            ..Rule::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            !json.contains("\"map\""),
            "map=false must not serialize: {}",
            json
        );

        let r = Rule {
            map: true,
            ..Rule::default()
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(
            json.contains("\"map\":true"),
            "map=true must serialize: {}",
            json
        );
    }

    #[test]
    fn hide_default_with_extras_is_error() {
        let errs = parse_dsl("hide default unique").unwrap_err();
        assert!(errs
            .iter()
            .any(|e| e.message.contains("cannot have additional tokens")));
    }

    #[test]
    fn multi_stat_patterns_parsed_as_vec_in_source_order() {
        let cfg = parse_dsl("rare {All Skills} {Faster Cast} {Resist}").unwrap();
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec![
                "All Skills".to_string(),
                "Faster Cast".to_string(),
                "Resist".to_string(),
            ]
        );
    }

    #[test]
    fn empty_braces_silently_dropped_between_valid_groups() {
        let cfg = parse_dsl("rare {} {foo}").unwrap();
        assert_eq!(cfg.rules[0].stat_patterns, vec!["foo".to_string()]);
    }

    #[test]
    fn stat_patterns_preserve_escapes_per_group() {
        let cfg = parse_dsl(r#"rare {a\}} {b}"#).unwrap();
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec![r"a\}".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn multi_stat_in_group_header_inherited_by_child_unset() {
        let src = "[rare {X} {Y}] {\n  \"foo\"\n}";
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(cfg.rules.len(), 1);
        assert_eq!(cfg.rules[0].name_pattern.as_deref(), Some("foo"));
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec!["X".to_string(), "Y".to_string()]
        );
    }

    #[test]
    fn child_stat_patterns_fully_replace_group() {
        let src = "[rare {X}] {\n  \"foo\" {Y} {Z}\n}";
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec!["Y".to_string(), "Z".to_string()]
        );
    }

    #[test]
    fn child_without_stat_inherits_group_patterns() {
        let src = "[rare {X} {Y}] {\n  \"foo\"\n  \"bar\" {Z}\n}";
        let cfg = parse_dsl(src).unwrap();
        assert_eq!(cfg.rules.len(), 2);
        assert_eq!(
            cfg.rules[0].stat_patterns,
            vec!["X".to_string(), "Y".to_string()]
        );
        assert_eq!(cfg.rules[1].stat_patterns, vec!["Z".to_string()]);
    }

    #[test]
    fn validator_errors_on_name_pattern_not_at_start() {
        let errors = validate_dsl(r#"unique set "Ring$" gold notify"#);
        let name_errs: Vec<_> = errors
            .iter()
            .filter(|e| {
                e.severity == ValidationSeverity::Error
                    && e.message.contains("Name pattern")
                    && e.message.contains("first token")
            })
            .collect();
        assert_eq!(name_errs.len(), 1, "got: {:?}", errors);
        assert!(errors.iter().all(|e| !e.message.contains("Unknown flag")));
    }

    #[test]
    fn validator_errors_on_second_name_pattern_after_leading_one() {
        let errors = validate_dsl(r#""Ring$" unique "Foo" gold notify"#);
        let extra_errs: Vec<_> = errors
            .iter()
            .filter(|e| {
                e.severity == ValidationSeverity::Error
                    && e.message.contains("Only one name pattern")
            })
            .collect();
        assert_eq!(extra_errs.len(), 1, "got: {:?}", errors);
    }

    #[test]
    fn single_braced_pattern_still_parses_as_one_element_vec() {
        // Regression guard: old profiles using `{(?s)a.*b.*c}` workarounds
        // must keep parsing identically under the new Vec-based model.
        let cfg = parse_dsl("rare {(?s)a.*b.*c}").unwrap();
        assert_eq!(cfg.rules[0].stat_patterns, vec!["(?s)a.*b.*c".to_string()]);
    }

    fn shadow_warnings(errors: &[ValidationError]) -> Vec<&ValidationError> {
        errors
            .iter()
            .filter(|e| e.message.contains("Shadowed by rule on line"))
            .collect()
    }

    #[test]
    fn subsumption_warns_when_broader_rule_below_with_different_effect() {
        let src = "\"Stone of Jordan\" unique gold notify\nunique";
        let errors = validate_dsl(src);
        let shadows = shadow_warnings(&errors);
        assert_eq!(shadows.len(), 1, "got: {:?}", errors);
        assert_eq!(shadows[0].line, 1);
        assert_eq!(shadows[0].severity, ValidationSeverity::Warning);
        assert!(shadows[0].message.contains("line 2"));
    }

    #[test]
    fn subsumption_silent_when_effects_identical() {
        let src = "unique gold notify\nunique gold notify";
        let errors = validate_dsl(src);
        assert!(shadow_warnings(&errors).is_empty(), "got: {:?}", errors);
    }

    #[test]
    fn subsumption_silent_when_predicates_disjoint() {
        // Real-world case from the user: hide sacred trash, then highlight
        // sacred uniques. Quality sets are disjoint — no shadowing.
        let src = "sacred low normal superior magic hide\nsacred unique notify map";
        let errors = validate_dsl(src);
        assert!(
            shadow_warnings(&errors).is_empty(),
            "disjoint quality sets must not warn: {:?}",
            errors
        );
    }

    #[test]
    fn subsumption_only_emits_first_shadower_per_rule() {
        // Line 1 is shadowed by every later `unique` line. Lines 2..=4 have
        // identical (empty) effects, so they don't shadow each other. We
        // expect exactly one warning, pointing at line 2.
        let src = "unique gold notify\nunique\nunique\nunique";
        let errors = validate_dsl(src);
        let shadows = shadow_warnings(&errors);
        assert_eq!(shadows.len(), 1, "got: {:?}", errors);
        assert_eq!(shadows[0].line, 1);
        assert!(shadows[0].message.contains("line 2"));
    }

    #[test]
    fn subsumption_handles_group_inheritance() {
        // The child rule effectively reads `"Jordan" unique gold notify`.
        // The top-level `unique show` matches all uniques (predicate is a
        // superset because it has no name pattern) and changes visibility,
        // so the child should be flagged as shadowed.
        let src = "[unique gold notify] {\n  \"Jordan\"\n}\nunique show";
        let errors = validate_dsl(src);
        let shadows = shadow_warnings(&errors);
        assert_eq!(shadows.len(), 1, "got: {:?}", errors);
        assert_eq!(shadows[0].line, 2);
        assert!(shadows[0].message.contains("line 4"));
    }

    #[test]
    fn subsumption_warns_when_later_rule_drops_name_pattern() {
        // Later rule has no name pattern, so it matches every name —
        // strict superset of the earlier `"Ring$"` rule.
        let src = "\"Ring$\" unique gold notify\nunique hide";
        let errors = validate_dsl(src);
        let shadows = shadow_warnings(&errors);
        assert_eq!(shadows.len(), 1, "got: {:?}", errors);
        assert_eq!(shadows[0].line, 1);
    }

    #[test]
    fn subsumption_stat_patterns_subset_logic() {
        // Earlier rule constrains All Skills + FCR; later rule only
        // constrains All Skills. Later's stat list is a subset, so it
        // matches strictly more items — should subsume.
        let src = "rare {All Skills} {Faster Cast} gold notify\nrare {All Skills} hide";
        let errors = validate_dsl(src);
        let shadows = shadow_warnings(&errors);
        assert_eq!(shadows.len(), 1, "got: {:?}", errors);
        assert_eq!(shadows[0].line, 1);
    }

    #[test]
    fn parse_sound_accepts_above_seven() {
        let cfg = parse_dsl("\"X\" notify sound8").unwrap();
        assert_eq!(cfg.rules[0].sound, Some(8));
        let cfg = parse_dsl("\"X\" notify sound99").unwrap();
        assert_eq!(cfg.rules[0].sound, Some(99));
        let cfg = parse_dsl("\"X\" notify sound255").unwrap();
        assert_eq!(cfg.rules[0].sound, Some(255));
    }

    #[test]
    fn parse_sound_rejects_zero_and_overflow() {
        // sound0 is not a valid keyword.
        let cfg = parse_dsl("\"X\" notify sound0").unwrap();
        assert_eq!(cfg.rules[0].sound, None);
        // 256 overflows u8 → unknown token, not parsed as a sound.
        let cfg = parse_dsl("\"X\" notify sound256").unwrap();
        assert_eq!(cfg.rules[0].sound, None);
    }
}
