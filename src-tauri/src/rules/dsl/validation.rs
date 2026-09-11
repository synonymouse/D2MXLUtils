//! Editor diagnostics, notification hints and source-ordered subsumption.

use super::parsing::collect_rules_with_lines;
use super::tokens::{
    extract_stat_patterns, is_known_token, parse_default_mode, parse_group_open,
    parse_sound_keyword, strip_inline_comment, DefaultModeParse,
};
use super::{NotifyColor, Rule, ValidationError, ValidationSeverity};

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
// Subsumption analysis
// =====================================================================

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
    if later.quest && !earlier.quest {
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
