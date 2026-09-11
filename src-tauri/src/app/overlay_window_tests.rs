use super::*;

#[test]
fn visual_and_edit_modes_reuse_one_overlay_window_with_different_styles() {
    let visual = overlay_window_spec(OverlayWindowKind::Visual);
    let edit = overlay_window_spec(OverlayWindowKind::Edit);

    assert_eq!(visual.label, "overlay");
    assert_eq!(visual.title, "D2MXLUtils Overlay");
    assert!(visual.layered);
    assert!(visual.click_through);

    assert_eq!(edit.label, "overlay");
    assert_eq!(edit.title, "D2MXLUtils Overlay");
    assert!(!edit.layered);
    assert!(!edit.click_through);
}

#[test]
fn overlay_is_hidden_when_game_is_minimized_even_if_foreground_still_matches() {
    assert!(overlay_should_be_visible(true, false, false, false));
    assert!(!overlay_should_be_visible(false, false, false, false));
    assert!(!overlay_should_be_visible(true, false, true, false));
}

#[test]
fn overlay_stays_visible_when_keyboard_panel_has_focus() {
    assert!(overlay_should_be_visible(false, true, false, true));
    assert!(!overlay_should_be_visible(false, true, false, false));
    assert!(!overlay_should_be_visible(false, true, true, true));
}

#[test]
fn keyboard_interactive_overlay_can_activate() {
    assert!(overlay_should_use_noactivate(false));
    assert!(!overlay_should_use_noactivate(true));
}

#[test]
fn keyboard_interactive_overlay_forces_foreground_on_open() {
    assert!(overlay_should_force_foreground(true, true));
    assert!(!overlay_should_force_foreground(true, false));
    assert!(!overlay_should_force_foreground(false, true));
}

#[test]
fn keyboard_interactive_change_reapplies_overlay_style() {
    assert!(overlay_style_needs_update(false, 0, 0, 0, 0, 0, 1));
    assert!(!overlay_style_needs_update(false, 0, 0, 0, 0, 1, 1));
}

#[test]
fn interactive_overlay_uses_non_layered_mode() {
    assert_eq!(
        overlay_window_kind_for_state(false, false),
        OverlayWindowKind::Edit
    );
}

#[test]
fn overlay_chrome_is_stripped_even_without_style_transition() {
    assert!(overlay_should_strip_chrome(true, true));
    assert!(overlay_should_strip_chrome(false, true));
    assert!(!overlay_should_strip_chrome(false, false));
}
