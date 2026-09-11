//! Shared lexical handling for parsing, validation and line classification.

use super::{ItemQuality, ItemTier, NotifyColor, PlayerClass, UniqueKind};

pub(super) fn parse_sound_keyword(lower: &str) -> Option<u8> {
    let suffix = lower.strip_prefix("sound")?;
    suffix.parse::<u8>().ok().filter(|&n| n >= 1)
}

pub(super) fn parse_socket_keyword(lower: &str) -> Option<u8> {
    let rest = lower.strip_prefix("sockets")?;
    let n: u8 = rest.parse().ok()?;
    if n <= 6 {
        Some(n)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum LevelToken {
    MinClvl(u32),
    MaxClvl(u32),
    MinIlvl(u32),
    MaxIlvl(u32),
}

/// `min_clvl<N>` / `max_clvl<N>` (character level) and `min_ilvl<N>` /
/// `max_ilvl<N>` (item level) — numeric-suffix keywords, same shape as
/// `sound<N>`/`sockets<N>`.
pub(super) fn parse_level_keyword(lower: &str) -> Option<LevelToken> {
    if let Some(rest) = lower.strip_prefix("min_clvl") {
        return rest.parse().ok().map(LevelToken::MinClvl);
    }
    if let Some(rest) = lower.strip_prefix("max_clvl") {
        return rest.parse().ok().map(LevelToken::MaxClvl);
    }
    if let Some(rest) = lower.strip_prefix("min_ilvl") {
        return rest.parse().ok().map(LevelToken::MinIlvl);
    }
    if let Some(rest) = lower.strip_prefix("max_ilvl") {
        return rest.parse().ok().map(LevelToken::MaxIlvl);
    }
    None
}

pub(super) fn parse_group_open(trimmed: &str) -> Option<&str> {
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

pub(super) fn strip_inline_comment(line: &str) -> &str {
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
pub(super) fn extract_stat_patterns(s: &str) -> (String, Vec<String>) {
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

pub(super) fn is_known_token(lower: &str) -> bool {
    if ItemQuality::from_str(lower).is_some()
        || ItemTier::from_str(lower).is_some()
        || UniqueKind::from_str(lower).is_some()
        || PlayerClass::from_str(lower).is_some()
        || NotifyColor::from_str(lower).is_some()
        || parse_socket_keyword(lower).is_some()
        || parse_level_keyword(lower).is_some()
    {
        return true;
    }
    matches!(
        lower,
        "eth" | "quest" | "show" | "hide" | "notify" | "stat" | "sound_none" | "map"
    ) || parse_sound_keyword(lower).is_some()
}

// =====================================================================
// Directive parsing
// =====================================================================

pub(super) enum DefaultModeParse {
    NotDirective,
    /// `true` = `hide default`, `false` = `show default`.
    Directive(bool),
    /// Payload carries `"hide"` or `"show"` for the error message.
    ExtraTokens(&'static str),
}

pub(super) fn parse_default_mode(trimmed: &str) -> DefaultModeParse {
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
