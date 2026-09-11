use super::*;

#[test]
fn lines_with_no_explanation_return_none() {
    for src in ["", "   ", "# a comment", "   # spaced", "}"] {
        assert!(explain_line(src).is_none(), "expected None for {:?}", src);
    }
}

#[test]
fn directives_explained() {
    let hide = explain_line("hide default").unwrap();
    assert!(hide.contains("hide every item"));
    assert!(hide.contains("'show'"));

    let show = explain_line("show default").unwrap();
    assert!(show.contains("game's built-in filter"));
}

#[test]
fn single_predicate_includes_unrestricted_note() {
    let s = explain_line("1 2 3 4 hide").unwrap();
    assert!(s.contains("Tier is one of: 1, 2, 3, 4"));
    assert!(s.contains("unrestricted"));
    assert!(s.contains("Actions:"));
    assert!(s.contains("Hide the item"));
}

#[test]
fn two_predicates_omit_unrestricted_note() {
    let s = explain_line("sacred superior magic rare hide").unwrap();
    assert!(s.contains("ALL of these"));
    assert!(s.contains("Tier is sacred"));
    assert!(s.contains("Quality is one of: superior, magic, rare"));
    assert!(!s.contains("unrestricted"));
    assert!(s.contains("Actions:"));
    assert!(s.contains("Hide the item"));
}

#[test]
fn name_pattern_in_quotes() {
    let s = explain_line("\"Ring$\" unique gold notify").unwrap();
    assert!(s.contains("Name matches the pattern \"Ring$\""));
    assert!(s.contains("Quality is unique"));
    assert!(s.contains("Show overlay notification"));
    assert!(s.contains("color: gold"));
}

#[test]
fn no_predicate_says_matches_every_item() {
    let s = explain_line("gold notify").unwrap();
    assert!(s.starts_with("Matches every item."));
    assert!(s.contains("Show overlay notification"));
}

#[test]
fn show_visibility_describes_override() {
    let s = explain_line("unique show").unwrap();
    assert!(s.contains("Actions:"));
    assert!(s.contains("Force-show the item"));
    assert!(s.contains("game's built-in hide"));
    assert!(!s.contains("'hide default'"));
}

#[test]
fn visibility_and_notification_under_one_actions_section() {
    let s = explain_line("unique gold notify map").unwrap();
    assert_eq!(s.matches("Actions:").count(), 1);
    assert!(!s.contains("Effects:"));
    assert!(s.contains("Show overlay notification"));
    assert!(s.contains("Drop a marker on the automap"));
}

#[test]
fn group_header_lists_defaults() {
    let s = explain_line("[unique gold notify] {").unwrap();
    assert!(s.starts_with("Group header"));
    assert!(s.contains("Quality is unique"));
    assert!(s.contains("Show overlay notification"));
}

#[test]
fn group_header_with_no_defaults() {
    let s = explain_line("[] {").unwrap();
    assert!(s.contains("(no defaults set)"));
}

#[test]
fn rarity_predicate_rendered() {
    let s = explain_line("sssu map").unwrap();
    assert!(s.contains("Rarity is SSSU"));
    assert!(s.contains("Drop a marker on the automap"));
}

#[test]
fn multi_rarity_predicate_rendered() {
    let s = explain_line("tu su hide").unwrap();
    assert!(s.contains("Rarity is one of: TU, SU"));
}

#[test]
fn ethereal_predicate_rendered() {
    let s = explain_line("eth unique").unwrap();
    assert!(s.contains("Item is ethereal"));
    assert!(s.contains("Quality is unique"));
}

#[test]
fn single_stat_pattern_rendered() {
    let s = explain_line("rare {All Skills} notify").unwrap();
    assert!(s.contains("Has stat pattern: \"All Skills\""));
    assert!(s.contains("includes item stats"));
}

#[test]
fn multi_stat_patterns_use_list_phrase() {
    let s = explain_line("rare {All Skills} {Faster Cast} notify").unwrap();
    assert!(s.contains("Has all stat patterns: \"All Skills\", \"Faster Cast\""));
}

#[test]
fn map_effect_listed() {
    let s = explain_line("unique map").unwrap();
    assert!(s.contains("Drop a marker on the automap"));
}

#[test]
fn color_without_notify_warns_in_tooltip() {
    let s = explain_line("unique gold").unwrap();
    assert!(s.contains("Color/sound flags are set but no notification will fire"));
}

#[test]
fn sound_modes_rendered() {
    assert!(explain_line("unique notify sound_none")
        .unwrap()
        .contains("silent"));
    assert!(explain_line("unique notify sound1")
        .unwrap()
        .contains("sound 1"));
    assert!(explain_line("unique notify sound3")
        .unwrap()
        .contains("sound 3"));
    assert!(explain_line("unique notify sound7")
        .unwrap()
        .contains("sound 7"));
}

#[test]
fn sound_modes_above_seven_rendered() {
    // After widening parse_sound_keyword to accept 1..=255, the explain
    // output must surface those numbers too — not silently drop them.
    assert!(explain_line("unique notify sound8")
        .unwrap()
        .contains("sound 8"));
    assert!(explain_line("unique notify sound99")
        .unwrap()
        .contains("sound 99"));
    assert!(explain_line("unique notify sound255")
        .unwrap()
        .contains("sound 255"));
}
