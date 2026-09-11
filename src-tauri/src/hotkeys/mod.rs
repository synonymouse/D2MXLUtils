//! Shared hotkey configuration and chord/focus predicates.
//! Complete watchers and their commands live with their business features.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use self::windows::{
    chord_is_pressed, chord_is_pressed_d2_only, chord_keys_are_pressed,
};
#[cfg(target_os = "linux")]
pub(crate) use linux::{
    chord_is_pressed_d2_only_linux, chord_is_pressed_linux, chord_keys_are_pressed_linux,
};

/// Hotkey configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    /// Virtual key code (e.g., 0x4B for 'K')
    pub key_code: u32,
    /// Modifier flags (Ctrl, Shift, Alt, Win)
    pub modifiers: u32,
    /// Human-readable representation (e.g., "Ctrl+K")
    pub display: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            key_code: 0x4B,    // 'K' key
            modifiers: 0x0002, // MOD_CONTROL
            display: "Ctrl+K".to_string(),
        }
    }
}

pub(crate) fn is_mouse_hotkey_key(key_code: u32) -> bool {
    (0x04..=0x06).contains(&key_code)
}
