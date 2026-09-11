//! Attribute accumulation, rule conversion and group inheritance.

use super::tokens::{
    extract_stat_patterns, parse_level_keyword, parse_socket_keyword, parse_sound_keyword,
    LevelToken,
};
use super::{
    ItemQuality, ItemTier, NotifyColor, ParseError, PlayerClass, Rule, UniqueKind, Visibility,
};

/// `Option` distinguishes "unset" (inherit from group) from "explicitly set".
#[derive(Debug, Clone, Default)]
pub(super) struct Attrs {
    stat_patterns: Option<Vec<String>>,
    qualities: Option<Vec<ItemQuality>>,
    tiers: Option<Vec<ItemTier>>,
    unique_kinds: Option<Vec<UniqueKind>>,
    sockets: Option<Vec<u8>>,
    classes: Option<Vec<PlayerClass>>,
    min_clvl: Option<u32>,
    max_clvl: Option<u32>,
    min_ilvl: Option<u32>,
    max_ilvl: Option<u32>,
    ethereal: Option<bool>,
    quest: Option<bool>,
    visibility: Option<Visibility>,
    color: Option<NotifyColor>,
    sound: Option<u8>,
    notify: Option<bool>,
    display_stats: Option<bool>,
    map: Option<bool>,
}

impl Attrs {
    pub(super) fn apply_to(&self, rule: &mut Rule) {
        if let Some(ref sp) = self.stat_patterns {
            rule.stat_patterns = sp.clone();
        }
        if let Some(ref q) = self.qualities {
            rule.qualities = q.clone();
        }
        if let Some(ref t) = self.tiers {
            rule.tiers = t.clone();
        }
        if let Some(ref k) = self.unique_kinds {
            rule.unique_kinds = k.clone();
        }
        if let Some(ref s) = self.sockets {
            rule.sockets = s.clone();
        }
        if let Some(ref c) = self.classes {
            rule.classes = c.clone();
        }
        if let Some(n) = self.min_clvl {
            rule.min_clvl = Some(n);
        }
        if let Some(n) = self.max_clvl {
            rule.max_clvl = Some(n);
        }
        if let Some(n) = self.min_ilvl {
            rule.min_ilvl = Some(n);
        }
        if let Some(n) = self.max_ilvl {
            rule.max_ilvl = Some(n);
        }
        if let Some(e) = self.ethereal {
            rule.ethereal = e;
        }
        if let Some(q) = self.quest {
            rule.quest = q;
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
    pub(super) fn fill_from_group(&mut self, group: &Attrs) {
        if self.stat_patterns.is_none() {
            self.stat_patterns = group.stat_patterns.clone();
        }
        if self.qualities.is_none() {
            self.qualities = group.qualities.clone();
        }
        if self.tiers.is_none() {
            self.tiers = group.tiers.clone();
        }
        if self.unique_kinds.is_none() {
            self.unique_kinds = group.unique_kinds.clone();
        }
        if self.sockets.is_none() {
            self.sockets = group.sockets.clone();
        }
        if self.classes.is_none() {
            self.classes = group.classes.clone();
        }
        if self.min_clvl.is_none() {
            self.min_clvl = group.min_clvl;
        }
        if self.max_clvl.is_none() {
            self.max_clvl = group.max_clvl;
        }
        if self.min_ilvl.is_none() {
            self.min_ilvl = group.min_ilvl;
        }
        if self.max_ilvl.is_none() {
            self.max_ilvl = group.max_ilvl;
        }
        if self.ethereal.is_none() {
            self.ethereal = group.ethereal;
        }
        if self.quest.is_none() {
            self.quest = group.quest;
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

pub(super) fn parse_attrs_into(
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
        if let Some(k) = UniqueKind::from_str(&lower) {
            let set = attrs.unique_kinds.get_or_insert_with(Vec::new);
            if !set.contains(&k) {
                set.push(k);
            }
            continue;
        }
        if let Some(c) = PlayerClass::from_str(&lower) {
            let set = attrs.classes.get_or_insert_with(Vec::new);
            if !set.contains(&c) {
                set.push(c);
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
        if let Some(tok) = parse_level_keyword(&lower) {
            match tok {
                LevelToken::MinClvl(n) => attrs.min_clvl = Some(n),
                LevelToken::MaxClvl(n) => attrs.max_clvl = Some(n),
                LevelToken::MinIlvl(n) => attrs.min_ilvl = Some(n),
                LevelToken::MaxIlvl(n) => attrs.max_ilvl = Some(n),
            }
            continue;
        }
        match lower.as_str() {
            "eth" => {
                attrs.ethereal = Some(true);
                continue;
            }
            "quest" => {
                attrs.quest = Some(true);
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

pub(super) fn attrs_from_rule(rule: &Rule) -> Attrs {
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
        unique_kinds: if rule.unique_kinds.is_empty() {
            None
        } else {
            Some(rule.unique_kinds.clone())
        },
        sockets: if rule.sockets.is_empty() {
            None
        } else {
            Some(rule.sockets.clone())
        },
        classes: if rule.classes.is_empty() {
            None
        } else {
            Some(rule.classes.clone())
        },
        min_clvl: rule.min_clvl,
        max_clvl: rule.max_clvl,
        min_ilvl: rule.min_ilvl,
        max_ilvl: rule.max_ilvl,
        ethereal: if rule.ethereal { Some(true) } else { None },
        quest: if rule.quest { Some(true) } else { None },
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
