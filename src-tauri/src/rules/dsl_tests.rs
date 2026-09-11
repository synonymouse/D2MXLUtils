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
fn parses_quest_keyword() {
    let cfg = parse_dsl("quest orange notify").unwrap();
    assert_eq!(cfg.rules.len(), 1);
    let r = &cfg.rules[0];
    assert!(r.quest);
    assert_eq!(r.color, Some(NotifyColor::Orange));
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
fn bare_rarity_keyword_parses_into_unique_kinds() {
    let cfg = parse_dsl("sssu map").unwrap();
    assert_eq!(cfg.rules[0].unique_kinds, vec![UniqueKind::Sssu]);
    assert!(cfg.rules[0].map);
    assert!(validate_dsl("sssu map").is_empty());
}

#[test]
fn multi_rarity_tokens_accumulate_into_set() {
    let cfg = parse_dsl("tu su ssu sssu notify").unwrap();
    assert_eq!(
        cfg.rules[0].unique_kinds,
        vec![
            UniqueKind::Tu,
            UniqueKind::Su,
            UniqueKind::Ssu,
            UniqueKind::Sssu,
        ]
    );
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
            e.severity == ValidationSeverity::Error && e.message.contains("Only one name pattern")
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
