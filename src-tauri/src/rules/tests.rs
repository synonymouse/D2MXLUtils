use super::*;
use crate::notifier::ItemDropEvent;

fn item(name: &str, quality: ItemQuality, eth: bool) -> ItemDropEvent {
    ItemDropEvent {
        unit_id: 1,
        class: 0,
        quality: quality.d2_quality_name().to_string(),
        name: name.to_string(),
        base_name: String::new(),
        category: None,
        stats: String::new(),
        name_is_runtime: false,
        runtime_stats_loaded: false,
        is_ethereal: eth,
        is_identified: true,
        p_unit_data: 0,
        seed: 0,
        history_pushed: false,
        tier: None,
        unique_kind: None,
        sockets: 0,
        clvl: 0,
        ilvl: 0,
        player_class: 0,
        filter: None,
    }
}

#[test]
fn last_match_wins() {
    let config = FilterConfig {
        rules: vec![
            Rule {
                qualities: vec![ItemQuality::Unique],
                color: Some(NotifyColor::Gold),
                ..Rule::default()
            },
            Rule {
                name_pattern: Some("Ring$".into()),
                qualities: vec![ItemQuality::Unique],
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
            qualities: vec![ItemQuality::Unique],
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
            qualities: vec![ItemQuality::Unique],
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

#[test]
fn normal_hide_rule_hides_normal_items() {
    let config = crate::rules::parse_dsl("normal hide").expect("valid DSL");
    assert_eq!(config.rules.len(), 1, "should parse one rule");
    assert_eq!(config.rules[0].qualities, vec![ItemQuality::Normal]);
    assert_eq!(config.rules[0].visibility, Visibility::Hide);

    let it = item("Sash", ItemQuality::Normal, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert_eq!(d.visibility, Visibility::Hide);
}

#[test]
fn hide_default_directive_hides_unmatched() {
    let config = crate::rules::parse_dsl("hide default").expect("valid DSL");
    assert!(config.hide_all, "hide default sets hide_all");

    let it = item("Any Item", ItemQuality::Normal, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert_eq!(d.visibility, Visibility::Hide);
}

#[test]
fn stat_pattern_rule_implicitly_shows_stats_and_reports_matched_line() {
    let config = FilterConfig {
        rules: vec![Rule {
            name_pattern: Some("Ring$".into()),
            qualities: vec![ItemQuality::Rare],
            stat_patterns: vec!["Skills".into()],
            notify: true,
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };

    let mut ring = item("Rune Turn", ItemQuality::Rare, false);
    ring.base_name = "Ring".to_string();
    ring.stats = "+10% Faster Cast Rate\n+1 to All Skills".to_string();
    let ctx = MatchContext::new(&ring);
    let d = config.decide(&ctx);
    let n = d.notification.expect("rule should notify");
    assert!(
        n.display_stats,
        "stat_patterns implies display_stats even without explicit flag"
    );
    assert_eq!(n.matched_stat_lines, vec![1]);
}

#[test]
fn name_only_rule_does_not_set_matched_stat_line() {
    let config = FilterConfig {
        rules: vec![Rule {
            name_pattern: Some("Ring$".into()),
            notify: true,
            display_stats: true,
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };

    let mut ring = item("Stone of Jordan Ring", ItemQuality::Unique, false);
    ring.stats = "+1 to All Skills".to_string();
    let ctx = MatchContext::new(&ring);
    let d = config.decide(&ctx);
    let n = d.notification.expect("rule should notify");
    assert!(n.display_stats);
    assert!(n.matched_stat_lines.is_empty());
}

#[test]
fn map_flag_independent_of_notify() {
    let config = FilterConfig {
        rules: vec![Rule {
            qualities: vec![ItemQuality::Unique],
            map: true,
            // no notify
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };
    let it = item("Unique Ring", ItemQuality::Unique, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert!(d.place_on_map, "map flag must fire without notify");
    assert!(d.notification.is_none(), "notify should not be auto-set");
}

#[test]
fn map_false_when_no_rule_matches() {
    let config = FilterConfig::default();
    let it = item("Anything", ItemQuality::Normal, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert!(!d.place_on_map);
}

#[test]
fn last_match_wins_for_map() {
    // Two rules both match a Unique Ring; the later one (no map) should win.
    let config = FilterConfig {
        rules: vec![
            Rule {
                qualities: vec![ItemQuality::Unique],
                map: true,
                ..Rule::default()
            },
            Rule {
                name_pattern: Some("Ring$".into()),
                qualities: vec![ItemQuality::Unique],
                // map defaults to false
                ..Rule::default()
            },
        ],
        ..FilterConfig::default()
    };
    let it = item("Stone of Jordan Ring", ItemQuality::Unique, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert!(!d.place_on_map, "later matching rule overrides map flag");
}

#[test]
fn sound_none_overrides_group_and_normalizes_to_no_sound() {
    let dsl = "[unique notify sound1] {\n  \"Jordan\" sound_none\n}\n";
    let config = crate::rules::parse_dsl(dsl).expect("valid DSL");
    assert_eq!(config.rules[0].sound, Some(0));

    let it = item("Stone of Jordan", ItemQuality::Unique, false);
    let ctx = MatchContext::new(&it);
    let n = config.decide(&ctx).notification.expect("should notify");
    assert_eq!(n.sound, None);
}

#[test]
fn group_hide_flattens_into_rules() {
    let dsl = "[hide] {\n  normal\n  superior\n}\n";
    let config = crate::rules::parse_dsl(dsl).expect("valid DSL");
    assert_eq!(config.rules.len(), 2, "group should flatten into 2 rules");

    let norm = item("Sash", ItemQuality::Normal, false);
    let ctx = MatchContext::new(&norm);
    let d = config.decide(&ctx);
    assert_eq!(d.visibility, Visibility::Hide, "normal item hidden");

    let sup = item("Superior Sash", ItemQuality::Superior, false);
    let ctx = MatchContext::new(&sup);
    let d = config.decide(&ctx);
    assert_eq!(d.visibility, Visibility::Hide, "superior item hidden");
}

#[test]
fn multi_stat_rule_highlights_all_matching_lines() {
    let config = FilterConfig {
        rules: vec![Rule {
            qualities: vec![ItemQuality::Unique],
            stat_patterns: vec!["All Skills".into(), "Faster Cast".into()],
            notify: true,
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };
    let mut ring = item("Ring", ItemQuality::Unique, false);
    ring.stats = "+3 to All Skills\n+15% Faster Cast Rate\n+30 to Strength".to_string();
    let ctx = MatchContext::new(&ring);
    let n = config.decide(&ctx).notification.expect("should notify");
    assert!(n.display_stats, "multi-stat implies display_stats");
    assert_eq!(n.matched_stat_lines, vec![0, 1]);
}

#[test]
fn multi_stat_rule_with_partial_match_does_not_fire() {
    let config = FilterConfig {
        rules: vec![Rule {
            qualities: vec![ItemQuality::Unique],
            stat_patterns: vec!["All Skills".into(), "Life Steal".into()],
            notify: true,
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };
    let mut ring = item("Ring", ItemQuality::Unique, false);
    ring.stats = "+3 to All Skills\n+15% Faster Cast Rate".to_string();
    let ctx = MatchContext::new(&ring);
    assert!(config.decide(&ctx).notification.is_none());
}

#[test]
fn partial_decide_later_cheap_rule_wins_without_stats() {
    let mut item = item("Amulet", ItemQuality::Rare, false);
    item.base_name = "Amulet".into();
    item.name_is_runtime = false;
    item.runtime_stats_loaded = false;

    let cfg = FilterConfig {
        rules: vec![
            Rule {
                name_pattern: Some("Amulet$".into()),
                qualities: vec![ItemQuality::Rare],
                stat_patterns: vec!["[3-9] to All Skills".into()],
                notify: true,
                ..Rule::default()
            },
            Rule {
                qualities: vec![ItemQuality::Rare],
                visibility: Visibility::Hide,
                ..Rule::default()
            },
        ],
        ..FilterConfig::default()
    };

    let ctx = MatchContext::new(&item);
    match cfg.decide_partial(&ctx) {
        PartialFilterDecision::Ready(decision) => {
            assert_eq!(decision.visibility, Visibility::Hide);
        }
        PartialFilterDecision::Needs(_) => panic!("later cheap hide rule should avoid stats"),
    }
}

#[test]
fn partial_decide_later_stat_rule_requests_stats_before_lower_priority_rule() {
    let mut item = item("Amulet", ItemQuality::Rare, false);
    item.base_name = "Amulet".into();
    item.name_is_runtime = false;
    item.runtime_stats_loaded = false;

    let cfg = FilterConfig {
        rules: vec![
            Rule {
                qualities: vec![ItemQuality::Rare],
                notify: true,
                ..Rule::default()
            },
            Rule {
                name_pattern: Some("Amulet$".into()),
                qualities: vec![ItemQuality::Rare],
                stat_patterns: vec!["[3-9] to All Skills".into()],
                notify: true,
                ..Rule::default()
            },
        ],
        ..FilterConfig::default()
    };

    let ctx = MatchContext::new(&item);
    match cfg.decide_partial(&ctx) {
        PartialFilterDecision::Needs(needs) => assert!(needs.runtime_stats),
        PartialFilterDecision::Ready(_) => {
            panic!("expected stats before lower-priority rare notify")
        }
    }
}

#[test]
fn full_decide_matches_stats_text_without_runtime_stats_flag() {
    let mut item = item("Amulet", ItemQuality::Rare, false);
    item.base_name = "Amulet".into();
    item.name_is_runtime = false;
    item.stats = "+3 to All Skills".into();
    item.runtime_stats_loaded = false;

    let cfg = FilterConfig {
        rules: vec![Rule {
            name_pattern: Some("Amulet$".into()),
            qualities: vec![ItemQuality::Rare],
            stat_patterns: vec!["[3-9] to All Skills".into()],
            notify: true,
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };

    let ctx = MatchContext::new(&item);
    assert!(cfg.decide(&ctx).notification.is_some());
}

#[test]
fn prepare_for_matching_compiles_rule_patterns_once() {
    let mut config = FilterConfig {
        rules: vec![Rule {
            name_pattern: Some("Ring$".into()),
            stat_patterns: vec![r"\+\d+ to All Skills".into(), "Ring[".into()],
            ..Rule::default()
        }],
        ..FilterConfig::default()
    };

    config.prepare_for_matching();

    let rule = &config.rules[0];
    assert!(rule.compiled_name_pattern.is_some());
    assert_eq!(rule.compiled_stat_patterns.len(), 2);
    assert!(rule.compiled_stat_patterns[0].is_regex());
    assert!(!rule.compiled_stat_patterns[1].is_regex());
}

#[test]
fn parse_dsl_does_not_prepare_runtime_matchers() {
    let config = crate::rules::parse_dsl(r#""Ring$" unique {\+\d+ to All Skills} notify"#)
        .expect("valid DSL");

    let rule = &config.rules[0];
    assert!(rule.compiled_name_pattern.is_none());
    assert!(rule.compiled_stat_patterns.is_empty());
}

#[test]
fn parse_dsl_records_source_line_including_inside_groups() {
    let config =
        crate::rules::parse_dsl("# comment\nunique gold\n\n[hide] {\n  normal\n  low\n}\n")
            .expect("valid DSL");

    assert_eq!(config.rules[0].source_line, 2);
    assert_eq!(
        config.rules[1].source_line, 5,
        "group child keeps its own line"
    );
    assert_eq!(config.rules[2].source_line, 6);
}

#[test]
fn decide_reports_matched_line_of_winning_rule() {
    let config = crate::rules::parse_dsl("unique gold\nset lime\n").expect("valid DSL");
    let it = item("Some Unique", ItemQuality::Unique, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert_eq!(d.matched_line, Some(1));
}

#[test]
fn decide_reports_no_matched_line_when_no_rule_matches() {
    let config = crate::rules::parse_dsl("unique gold\n").expect("valid DSL");
    let it = item("Something Normal", ItemQuality::Normal, false);
    let ctx = MatchContext::new(&it);
    let d = config.decide(&ctx);
    assert_eq!(d.matched_line, None);
}

#[test]
fn has_map_rules_detects_any_map_rule() {
    let mut config = FilterConfig::default();
    assert!(!config.has_map_rules());

    config.rules.push(Rule {
        qualities: vec![ItemQuality::Unique],
        notify: true,
        ..Rule::default()
    });
    assert!(!config.has_map_rules());

    config.rules.push(Rule {
        name_pattern: Some("Ring".into()),
        map: true,
        ..Rule::default()
    });
    assert!(config.has_map_rules());
}
