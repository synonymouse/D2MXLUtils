use super::*;
use serde_json::json;

fn fresh_settings() -> AppSettings {
    AppSettings::default()
}

#[test]
fn migrates_when_legacy_keys_present() {
    let raw = json!({ "notificationX": 42.5, "notificationY": 17.0 });
    let mut s = fresh_settings();

    let changed = apply(&raw, &mut s);

    assert!(changed, "should report changed=true");
    assert_eq!(
        s.widget_positions.get("notifications"),
        Some(&WidgetPosition { x: 42.5, y: 17.0 }),
    );
}

#[test]
fn idempotent_when_already_migrated() {
    let raw = json!({ "notificationX": 99.0, "notificationY": 99.0 });
    let mut s = fresh_settings();
    s.widget_positions
        .insert("notifications".into(), WidgetPosition { x: 5.0, y: 7.0 });

    let changed = apply(&raw, &mut s);

    assert!(!changed, "should report changed=false on second run");
    assert_eq!(
        s.widget_positions.get("notifications"),
        Some(&WidgetPosition { x: 5.0, y: 7.0 }),
        "must not overwrite an existing entry",
    );
}

#[test]
fn skips_when_neither_legacy_key_present() {
    // Fresh install: no legacy keys, no widget_positions["notifications"].
    // The helper's spec default kicks in client-side, so we should NOT
    // pollute settings.json with a redundant entry.
    let raw = json!({});
    let mut s = fresh_settings();

    let changed = apply(&raw, &mut s);

    assert!(!changed);
    assert!(s.widget_positions.get("notifications").is_none());
}
