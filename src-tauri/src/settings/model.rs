//! Settings schema and defaults shared by persistence and migrations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::hotkeys::HotkeyConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DpsMeterSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hotkey_reset: Option<HotkeyConfig>,
}

/// Position of an overlay widget, expressed as a percentage of the
/// overlay (0..=100 on each axis).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPosition {
    pub x: f64,
    pub y: f64,
}

/// One configurable drop-sound slot. Index in `AppSettings.sounds` + 1
/// equals the DSL keyword index (e.g. element 0 -> `sound1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundSlot {
    pub label: String,
    pub volume: f32,
    pub source: SoundSource,
}

/// What plays for a given slot.
/// - `Default`: bundled `public/sounds/{N}.mp3` (slots 1..=7 only).
/// - `Custom`: user-imported file in `app_data_dir/sounds/`.
/// - `Empty`: silence; only for slots >= 8 after deletion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SoundSource {
    Default,
    Custom { file_name: String },
    Empty,
}

fn default_sounds() -> Vec<SoundSlot> {
    (1..=7)
        .map(|n| SoundSlot {
            label: format!("Sound {}", n),
            volume: 0.8,
            source: SoundSource::Default,
        })
        .collect()
}

/// Application settings structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// UI theme: "dark" or "light"
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Master multiplier for drop notification sounds (0.0 - 1.0). Final played gain = `sound_volume * slot.volume`.
    #[serde(default = "default_volume")]
    pub sound_volume: f32,

    /// Active loot filter profile name
    #[serde(default)]
    pub active_profile: Option<String>,

    /// Notification display duration in milliseconds
    #[serde(default = "default_notification_duration")]
    pub notification_duration: u32,

    /// Notification stack direction: "up" or "down"
    #[serde(default = "default_stack_direction")]
    pub notification_stack_direction: String,

    /// Notification font size in pixels
    #[serde(default = "default_notification_font_size")]
    pub notification_font_size: u32,

    /// Notification background opacity (0.0 - 1.0)
    #[serde(default = "default_notification_opacity")]
    pub notification_opacity: f32,

    /// When true, show only base name for Set/TU/SU/SSU/SSSU drops
    /// (single-line layout). Stat-flagged rules ignore this.
    #[serde(default)]
    pub compact_name: bool,

    #[serde(default)]
    pub show_only_matched_stats: bool,

    /// Hotkey configuration for toggling main window
    #[serde(default)]
    pub toggle_window_hotkey: HotkeyConfig,

    /// Hotkey held to enter overlay edit mode (drag notification anchor)
    #[serde(default = "default_edit_overlay_hotkey")]
    pub edit_overlay_hotkey: HotkeyConfig,

    /// Hotkey held to reveal every item on the ground, bypassing `hide` rules
    #[serde(default = "default_reveal_hidden_hotkey")]
    pub reveal_hidden_hotkey: HotkeyConfig,

    /// Hotkey to toggle the in-game loot history overlay panel.
    #[serde(default = "default_loot_history_hotkey")]
    pub loot_history_hotkey: HotkeyConfig,

    /// Hotkey to open the in-game MXL item search overlay.
    #[serde(default = "default_item_search_hotkey")]
    pub item_search_hotkey: HotkeyConfig,

    /// Hotkey that autofills the create-game Name/Password/Description
    /// fields via synthesized keystrokes — see `game_create/input.rs`. Click
    /// into the Game Name field first; Tab order fills the rest. Unset
    /// (not `HotkeyConfig::default()`, which is Ctrl+K — already claimed
    /// by `toggle_window_hotkey`) until the user opts in, same convention
    /// as `dps_meter.hotkey_reset`.
    #[serde(default = "default_unset_hotkey")]
    pub game_create_autofill_hotkey: HotkeyConfig,

    /// Combined with an in-memory auto-incrementing counter (not
    /// persisted, resets per app launch) to form the game name:
    /// `{prefix}{index}`.
    #[serde(default)]
    pub game_create_name_prefix: String,

    /// Fixed password, used when `game_create_password_use_prefix` is off.
    #[serde(default)]
    pub game_create_password: String,

    /// Combined with the same counter as the name (`{prefix}{index}`),
    /// used instead of `game_create_password` when
    /// `game_create_password_use_prefix` is on.
    #[serde(default)]
    pub game_create_password_prefix: String,

    #[serde(default)]
    pub game_create_password_use_prefix: bool,

    #[serde(default)]
    pub game_create_description: String,

    /// When true, scanner logs per-item filter decisions (noisy; opt-in for debugging).
    #[serde(default)]
    pub verbose_filter_logging: bool,

    /// How long the Loot Filter tab's "show matches" mode keeps a rule line
    /// flashed after it decides a drop, in milliseconds.
    #[serde(default = "default_live_match_highlight_duration_ms")]
    pub live_match_highlight_duration_ms: u32,

    #[serde(default = "default_auto_always_show_items")]
    pub auto_always_show_items: bool,

    #[serde(default = "default_auto_no_pickup")]
    pub auto_no_pickup: bool,

    /// Whether to show the "Items hidden — press Alt" overlay indicator
    /// when the in-game item highlight toggle is off.
    #[serde(default = "default_show_items_hidden_indicator")]
    pub show_items_hidden_indicator: bool,

    /// Per-slot drop sounds. Slot index = element position + 1.
    /// Final played gain = `sound_volume * slot.volume`.
    #[serde(default = "default_sounds")]
    pub sounds: Vec<SoundSlot>,

    /// 1-based sound-slot index played when a goblin appears in the
    /// scanner's view. `None` disables the feature.
    #[serde(default)]
    pub goblin_alert_slot: Option<u32>,

    #[serde(default)]
    pub dps_meter: DpsMeterSettings,

    /// Centralized positions for repositionable overlay widgets, keyed by
    /// widget id (see `src/lib/overlay-widgets.ts`). Percent of overlay size.
    #[serde(default)]
    pub widget_positions: HashMap<String, WidgetPosition>,

    /// Collapsed group-rule line numbers in the Loot Filter editor, keyed by
    /// profile name, so folds survive switching tabs and restarting the app.
    #[serde(default)]
    pub folded_lines: HashMap<String, Vec<u32>>,
}

/// Window state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

// Default value functions
fn default_theme() -> String {
    "dark".to_string()
}

fn default_volume() -> f32 {
    0.8
}

fn default_notification_duration() -> u32 {
    5000
}

fn default_live_match_highlight_duration_ms() -> u32 {
    900
}

fn default_stack_direction() -> String {
    "up".to_string()
}

fn default_notification_font_size() -> u32 {
    14
}

fn default_notification_opacity() -> f32 {
    0.9
}

fn default_auto_always_show_items() -> bool {
    true
}

fn default_auto_no_pickup() -> bool {
    true
}

fn default_show_items_hidden_indicator() -> bool {
    true
}

fn default_edit_overlay_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0,
        modifiers: 0x0001 | 0x0002, // MOD_ALT | MOD_CONTROL
        display: "Ctrl+Alt".to_string(),
    }
}

fn default_reveal_hidden_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x5A, // 'Z'
        modifiers: 0,
        display: "Z".to_string(),
    }
}

fn default_loot_history_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x4E,
        modifiers: 0x0001,
        display: "Alt+N".to_string(),
    }
}

fn default_unset_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0,
        modifiers: 0,
        display: "None".to_string(),
    }
}

fn default_item_search_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key_code: 0x46,
        modifiers: 0x0001,
        display: "Alt+F".to_string(),
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            sound_volume: default_volume(),
            active_profile: None,
            notification_duration: default_notification_duration(),
            notification_stack_direction: default_stack_direction(),
            notification_font_size: default_notification_font_size(),
            notification_opacity: default_notification_opacity(),
            compact_name: false,
            show_only_matched_stats: false,
            toggle_window_hotkey: HotkeyConfig::default(),
            edit_overlay_hotkey: default_edit_overlay_hotkey(),
            reveal_hidden_hotkey: default_reveal_hidden_hotkey(),
            loot_history_hotkey: default_loot_history_hotkey(),
            item_search_hotkey: default_item_search_hotkey(),
            game_create_autofill_hotkey: default_unset_hotkey(),
            game_create_name_prefix: String::new(),
            game_create_password: String::new(),
            game_create_password_prefix: String::new(),
            game_create_password_use_prefix: false,
            game_create_description: String::new(),
            verbose_filter_logging: false,
            live_match_highlight_duration_ms: default_live_match_highlight_duration_ms(),
            auto_always_show_items: default_auto_always_show_items(),
            auto_no_pickup: default_auto_no_pickup(),
            show_items_hidden_indicator: default_show_items_hidden_indicator(),
            sounds: default_sounds(),
            goblin_alert_slot: None,
            dps_meter: DpsMeterSettings::default(),
            widget_positions: HashMap::new(),
            folded_lines: HashMap::new(),
        }
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 1024,
            height: 640,
            maximized: false,
        }
    }
}
