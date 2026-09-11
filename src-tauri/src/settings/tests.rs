use super::*;

#[test]
fn legacy_settings_without_sounds_field_seeds_seven_defaults() {
    // settings.json that predates the Sounds tab — no `sounds` field.
    let json = r#"{
            "theme": "dark",
            "soundVolume": 0.5,
            "activeProfile": null,
            "notificationDuration": 5000,
            "notificationStackDirection": "up",
            "notificationFontSize": 14,
            "notificationOpacity": 0.9,
            "notificationX": 1.0,
            "notificationY": 1.0,
            "compactName": false,
            "toggleWindowHotkey": {"keyCode": 0, "modifiers": 0, "display": "None"},
            "editOverlayHotkey": {"keyCode": 0, "modifiers": 3, "display": "Ctrl+Alt"},
            "revealHiddenHotkey": {"keyCode": 90, "modifiers": 0, "display": "Z"},
            "lootHistoryHotkey": {"keyCode": 78, "modifiers": 0, "display": "N"},
            "verboseFilterLogging": false,
            "autoAlwaysShowItems": true
        }"#;
    let settings: AppSettings = serde_json::from_str(json).expect("valid legacy json");
    assert_eq!(settings.sounds.len(), 7);
    for (i, slot) in settings.sounds.iter().enumerate() {
        assert_eq!(slot.label, format!("Sound {}", i + 1));
        assert_eq!(slot.volume, 0.8);
        assert!(matches!(slot.source, SoundSource::Default));
    }
    assert_eq!(settings.sound_volume, 0.5);
    assert!(settings.auto_no_pickup);
}

#[test]
fn default_settings_enable_auto_no_pickup() {
    assert!(AppSettings::default().auto_no_pickup);
}

#[test]
fn default_settings_include_item_search_hotkey() {
    let hotkey = AppSettings::default().item_search_hotkey;
    assert_eq!(hotkey.key_code, 0x46);
    assert_eq!(hotkey.modifiers, 0x0001);
    assert_eq!(hotkey.display, "Alt+F");
}

#[test]
fn sound_source_round_trips_each_variant() {
    let slots = vec![
        SoundSlot {
            label: "Default".into(),
            volume: 0.8,
            source: SoundSource::Default,
        },
        SoundSlot {
            label: "Custom".into(),
            volume: 0.5,
            source: SoundSource::Custom {
                file_name: "slot-8.mp3".into(),
            },
        },
        SoundSlot {
            label: "Empty".into(),
            volume: 0.0,
            source: SoundSource::Empty,
        },
    ];
    let json = serde_json::to_string(&slots).unwrap();
    let back: Vec<SoundSlot> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 3);
    assert!(matches!(back[0].source, SoundSource::Default));
    match &back[1].source {
        SoundSource::Custom { file_name } => assert_eq!(file_name, "slot-8.mp3"),
        other => panic!("expected Custom, got {:?}", other),
    }
    assert!(matches!(back[2].source, SoundSource::Empty));
}

#[test]
fn sound_source_custom_uses_camel_case_on_wire() {
    let slot = SoundSlot {
        label: "Custom".into(),
        volume: 0.5,
        source: SoundSource::Custom {
            file_name: "slot-8.mp3".into(),
        },
    };
    let json = serde_json::to_string(&slot).unwrap();
    // The wire format MUST use camelCase `fileName`, otherwise the JS
    // frontend (which sends `fileName`) cannot round-trip through Tauri.
    assert!(
        json.contains("\"fileName\":\"slot-8.mp3\""),
        "expected camelCase fileName on the wire, got {}",
        json
    );
    assert!(
        !json.contains("file_name"),
        "snake_case file_name should not be on the wire, got {}",
        json
    );
}

#[test]
fn sound_source_deserialises_camel_case_payload_from_frontend() {
    // Exact JSON shape that `SoundsTab.svelte` sends through Tauri's
    // `save_settings` command. If this fails, the Tauri command rejects
    // the args before the Rust handler runs, and the slot's Custom state
    // never makes it to disk.
    let json = r#"{
            "label": "Custom",
            "volume": 0.5,
            "source": { "kind": "custom", "fileName": "slot-8.mp3" }
        }"#;
    let slot: SoundSlot = serde_json::from_str(json).expect("frontend payload must deserialise");
    match slot.source {
        SoundSource::Custom { file_name } => assert_eq!(file_name, "slot-8.mp3"),
        other => panic!("expected Custom, got {:?}", other),
    }
}
