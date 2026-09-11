//! Rule/group parsing and the line classification used by explanations.

use super::attributes::{attrs_from_rule, parse_attrs_into, Attrs};
use super::tokens::{parse_default_mode, parse_group_open, strip_inline_comment, DefaultModeParse};
use super::{FilterConfig, ParseError, Rule};

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
                    fresh.source_line = line_num;
                    rules.push(fresh);
                } else {
                    rule.source_line = line_num;
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

// =====================================================================
// Line classification (for the explainer module)
// =====================================================================

pub(in crate::rules) enum ParsedLine {
    Empty,
    GroupClose,
    Directive(bool),
    GroupHeader(Rule),
    Rule(Rule),
    Unparseable,
}

pub(in crate::rules) fn classify_line(line: &str) -> ParsedLine {
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

// Lenient rule collection for validation's subsumption analysis.
pub(super) fn collect_rules_with_lines(text: &str) -> Vec<(Rule, usize)> {
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
