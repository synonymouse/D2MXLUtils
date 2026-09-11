//! Linux chord and focus predicates; raw X11 key queries stay private.

use super::{is_mouse_hotkey_key, HotkeyConfig};

/// X11 key-state polling, replacing `GetAsyncKeyState`. All 6 hotkey
/// features already poll on a ~30ms timer rather than using a true event
/// loop (`RegisterHotKey`'s `WM_HOTKEY` can't deliver release events or
/// accept modifier-only chords, so even the one Windows mechanism that
/// *could* use a message loop shares the polling design) — so this keeps
/// the exact same architecture on Linux, just swapping the leaf
/// "is this key down right now" primitive from `GetAsyncKeyState` to
/// `XQueryKeymap` (system-wide, like `GetAsyncKeyState` — not scoped to
/// whichever window has focus, works the same via XWayland). One
/// difference worth knowing: `RegisterHotKey` also *consumes* the key
/// combo so the game/focused app never sees it; polling only observes,
/// so on Linux the key still reaches whatever's focused too. Unlikely to
/// matter for the combos this app defaults to (Ctrl+K, Alt+F, etc.).
mod x11_keys {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    // Windows VK_* code -> X11 keysym(s). Multiple entries (e.g. Ctrl) are
    // "either physical key counts" — VK_CONTROL doesn't distinguish L/R.
    pub const VK_SHIFT: u32 = 0x10;
    pub const VK_CONTROL: u32 = 0x11;
    pub const VK_MENU: u32 = 0x12; // Alt
    pub const VK_LWIN: u32 = 0x5B;
    pub const VK_RWIN: u32 = 0x5C;

    const XK_SHIFT_L: u32 = 0xffe1;
    const XK_SHIFT_R: u32 = 0xffe2;
    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_CONTROL_R: u32 = 0xffe4;
    const XK_ALT_L: u32 = 0xffe9;
    const XK_ALT_R: u32 = 0xffea;
    const XK_SUPER_L: u32 = 0xffeb;
    const XK_SUPER_R: u32 = 0xffec;

    /// Maps a single-value VK code (a specific key, not a modifier pair)
    /// to its X11 keysym. Covers letters, digits, F-keys, and the common
    /// named keys this app's hotkey picker is likely to produce.
    fn vk_to_keysym(vk: u32) -> Option<u32> {
        match vk {
            0x30..=0x39 => Some(vk),        // '0'-'9' — same codepoint as ASCII/keysym
            0x41..=0x5A => Some(vk + 0x20), // 'A'-'Z' -> lowercase keysym (physical key, shift-independent)
            0x70..=0x7B => Some(0xffbe + (vk - 0x70)), // F1..F12 -> XK_F1..XK_F12
            0x08 => Some(0xff08),           // Backspace
            0x09 => Some(0xff09),           // Tab
            0x0D => Some(0xff0d),           // Return
            0x1B => Some(0xff1b),           // Escape
            0x20 => Some(0x0020),           // Space
            0x25 => Some(0xff51),           // Left
            0x26 => Some(0xff52),           // Up
            0x27 => Some(0xff53),           // Right
            0x28 => Some(0xff54),           // Down
            0x21 => Some(0xff55),           // PageUp
            0x22 => Some(0xff56),           // PageDown
            0x23 => Some(0xff57),           // End
            0x24 => Some(0xff50),           // Home
            0x2D => Some(0xff63),           // Insert
            0x2E => Some(0xffff),           // Delete
            _ => None,
        }
    }

    struct KeyboardMapping {
        min_kc: u8,
        max_kc: u8,
        per_kc: usize,
        keysyms: Vec<u32>,
    }

    /// The keycode->keysym mapping only changes when the user changes
    /// keyboard layout at the OS level, which doesn't happen mid-session
    /// — fetch it once instead of on every ~30ms poll. Combined with
    /// `x11_conn()` (shared connection, see its doc comment), a chord
    /// check is now a single `query_keymap` round-trip instead of two
    /// requests over a throwaway connection.
    fn keyboard_mapping() -> Option<&'static KeyboardMapping> {
        static MAPPING: std::sync::OnceLock<Option<KeyboardMapping>> = std::sync::OnceLock::new();
        MAPPING
            .get_or_init(|| {
                let (conn, _) = crate::process::linux_x11_conn().ok()?;
                let setup = conn.setup();
                let min_kc = setup.min_keycode;
                let max_kc = setup.max_keycode;
                if max_kc < min_kc {
                    return None;
                }
                let count = max_kc - min_kc + 1;
                let reply = conn
                    .get_keyboard_mapping(min_kc, count)
                    .ok()?
                    .reply()
                    .ok()?;
                let per_kc = reply.keysyms_per_keycode as usize;
                if per_kc == 0 {
                    return None;
                }
                Some(KeyboardMapping {
                    min_kc,
                    max_kc,
                    per_kc,
                    keysyms: reply.keysyms,
                })
            })
            .as_ref()
    }

    /// One `query_keymap` round-trip over the shared connection, checked
    /// against every keysym the chord needs.
    fn keys_down(keysyms: &[u32]) -> Vec<bool> {
        let mut result = vec![false; keysyms.len()];
        let Some(mapping) = keyboard_mapping() else {
            return result;
        };
        let Ok((conn, _)) = crate::process::linux_x11_conn() else {
            return result;
        };
        let Ok(km_cookie) = conn.query_keymap() else {
            return result;
        };
        let Ok(km) = km_cookie.reply() else {
            return result;
        };

        let is_kc_down = |kc: u8| {
            let byte = kc as usize / 8;
            let bit = kc as usize % 8;
            km.keys
                .get(byte)
                .map(|b| b & (1 << bit) != 0)
                .unwrap_or(false)
        };

        for (chunk_i, chunk) in mapping.keysyms.chunks(mapping.per_kc).enumerate() {
            let kc = mapping.min_kc as usize + chunk_i;
            if kc > mapping.max_kc as usize {
                break;
            }
            if !chunk.iter().any(|ks| keysyms.contains(ks)) {
                continue;
            }
            if is_kc_down(kc as u8) {
                for (i, &target) in keysyms.iter().enumerate() {
                    if chunk.contains(&target) {
                        result[i] = true;
                    }
                }
            }
        }
        result
    }

    pub fn is_key_down(vk: u32) -> bool {
        match vk_to_keysym(vk) {
            Some(ks) => keys_down(&[ks]).first().copied().unwrap_or(false),
            None => false,
        }
    }

    pub fn is_modifier_down(vk: u32) -> bool {
        let pair = match vk {
            VK_CONTROL => [XK_CONTROL_L, XK_CONTROL_R],
            VK_SHIFT => [XK_SHIFT_L, XK_SHIFT_R],
            VK_MENU => [XK_ALT_L, XK_ALT_R],
            VK_LWIN => [XK_SUPER_L, XK_SUPER_L],
            VK_RWIN => [XK_SUPER_R, XK_SUPER_R],
            _ => return false,
        };
        let down = keys_down(&pair);
        down.iter().any(|&b| b)
    }
}

fn is_d2_or_app_foreground_linux() -> bool {
    crate::process::linux_is_d2_or_own_window_focused(crate::process::LINUX_WINDOW_TITLE)
        .unwrap_or(false)
}

fn is_d2_foreground_linux() -> bool {
    crate::process::linux_is_window_focused_by_title(crate::process::LINUX_WINDOW_TITLE)
        .unwrap_or(false)
}

pub(crate) fn chord_keys_are_pressed_linux(hk: &HotkeyConfig) -> bool {
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;

    if hk.key_code == 0 && hk.modifiers == 0 {
        return false;
    }
    if is_mouse_hotkey_key(hk.key_code) {
        // No portable "is this mouse button held" primitive without a
        // global pointer-grab; unsupported on Linux for v1.
        return false;
    }

    if hk.modifiers & MOD_CONTROL != 0 && !x11_keys::is_modifier_down(x11_keys::VK_CONTROL) {
        return false;
    }
    if hk.modifiers & MOD_SHIFT != 0 && !x11_keys::is_modifier_down(x11_keys::VK_SHIFT) {
        return false;
    }
    if hk.modifiers & MOD_ALT != 0 && !x11_keys::is_modifier_down(x11_keys::VK_MENU) {
        return false;
    }
    if hk.modifiers & MOD_WIN != 0
        && !(x11_keys::is_modifier_down(x11_keys::VK_LWIN)
            || x11_keys::is_modifier_down(x11_keys::VK_RWIN))
    {
        return false;
    }

    if hk.key_code != 0 && !x11_keys::is_key_down(hk.key_code) {
        return false;
    }

    true
}

pub(crate) fn chord_is_pressed_linux(hk: &HotkeyConfig) -> bool {
    is_d2_or_app_foreground_linux() && chord_keys_are_pressed_linux(hk)
}

pub(crate) fn chord_is_pressed_d2_only_linux(hk: &HotkeyConfig) -> bool {
    is_d2_foreground_linux() && chord_keys_are_pressed_linux(hk)
}
