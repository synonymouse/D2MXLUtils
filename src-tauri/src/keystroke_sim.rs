//! Cross-platform synthetic keystroke injection, used by the "autofill
//! game create" hotkey (`hotkeys.rs`) to type preset Name/Password/
//! Description text into whichever text field currently has focus in the
//! D2 window. Deliberately OS-level input synthesis rather than reading or
//! writing D2's own memory — the create-game UI's text-field buffers
//! proved to be either transient heap allocations with no discoverable
//! stable pointer, or otherwise not worth the reverse-engineering risk
//! (see the investigation that led here). Simulating real keystrokes needs
//! no game-memory access at all and works the same way a human typing
//! would.

/// Non-printable key used to move between fields.
pub enum SpecialKey {
    Tab,
}

/// Extra settle time after Tab specifically, on top of the per-keystroke
/// pacing every event already gets (see `linux_impl`/`windows_impl`).
/// Moving focus between D2's own hand-rolled UI controls isn't
/// instantaneous the way a native OS text field's focus is — sending the
/// next field's keystrokes immediately after Tab (zero delay, unlike real
/// typing) landed them in the *old* field before D2 finished processing
/// the focus change, which is what "description becomes the name" was:
/// Tab never had time to take effect before the next clear+type ran.
const TAB_SETTLE: std::time::Duration = std::time::Duration::from_millis(80);

/// Types Name, tabs to the next field, types Password, and — if
/// non-empty — tabs again and types Description. Assumes the Game Name
/// field already has focus (the user clicks into it before pressing the
/// hotkey) and that Tab moves through the fields in that order, matching
/// D2's own create-game dialog. Doesn't clear fields first — the create-
/// game dialog is freshly opened/empty each time this actually gets used.
pub fn autofill_create_game(name: &str, password: &str, description: &str) -> Result<(), String> {
    type_text(name)?;
    send_special(SpecialKey::Tab)?;
    std::thread::sleep(TAB_SETTLE);
    type_text(password)?;
    if !description.is_empty() {
        send_special(SpecialKey::Tab)?;
        std::thread::sleep(TAB_SETTLE);
        type_text(description)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::SpecialKey;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT};
    use x11rb::protocol::xtest::fake_input;

    const XK_TAB: u32 = 0xff09;

    /// Look up the keycode (and whether Shift is needed) currently bound
    /// to `keysym` in the live keyboard mapping. Rebuilt fresh per call —
    /// this only runs on a rare, user-triggered hotkey, not a hot path.
    fn keycode_for_keysym(
        conn: &x11rb::rust_connection::RustConnection,
        keysym: u32,
    ) -> Option<(u8, bool)> {
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let count = max.saturating_sub(min).saturating_add(1);
        let reply = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
        let per = reply.keysyms_per_keycode as usize;
        if per == 0 {
            return None;
        }
        for (i, chunk) in reply.keysyms.chunks(per).enumerate() {
            if chunk.first() == Some(&keysym) {
                return Some((min + i as u8, false));
            }
            if per > 1 && chunk.get(1) == Some(&keysym) {
                return Some((min + i as u8, true));
            }
        }
        None
    }

    /// Small gap between individual synthesized key events. D2's own UI
    /// (not a native OS text field) processes each event on its own game
    /// tick — a zero-delay burst of `fake_input` calls arrives faster than
    /// real typing ever would and got events dropped/coalesced in
    /// practice. Matches roughly what a fast human typist produces.
    const KEY_PACING: std::time::Duration = std::time::Duration::from_millis(12);

    fn send_keycode(
        conn: &x11rb::rust_connection::RustConnection,
        root: u32,
        keycode: u8,
        shift: bool,
    ) -> Result<(), String> {
        let shift_keycode = shift.then(|| keycode_for_keysym(conn, 0xffe1)).flatten(); // XK_Shift_L

        if shift {
            if let Some((sk, _)) = shift_keycode {
                fake_input(conn, KEY_PRESS_EVENT, sk, 0, root, 0, 0, 0)
                    .map_err(|e| format!("fake_input shift down failed: {}", e))?;
                conn.flush()
                    .map_err(|e| format!("X11 flush failed: {}", e))?;
                std::thread::sleep(KEY_PACING);
            }
        }
        fake_input(conn, KEY_PRESS_EVENT, keycode, 0, root, 0, 0, 0)
            .map_err(|e| format!("fake_input key down failed: {}", e))?;
        conn.flush()
            .map_err(|e| format!("X11 flush failed: {}", e))?;
        std::thread::sleep(KEY_PACING);
        fake_input(conn, KEY_RELEASE_EVENT, keycode, 0, root, 0, 0, 0)
            .map_err(|e| format!("fake_input key up failed: {}", e))?;
        conn.flush()
            .map_err(|e| format!("X11 flush failed: {}", e))?;
        std::thread::sleep(KEY_PACING);
        if shift {
            if let Some((sk, _)) = shift_keycode {
                fake_input(conn, KEY_RELEASE_EVENT, sk, 0, root, 0, 0, 0)
                    .map_err(|e| format!("fake_input shift up failed: {}", e))?;
                conn.flush()
                    .map_err(|e| format!("X11 flush failed: {}", e))?;
                std::thread::sleep(KEY_PACING);
            }
        }
        Ok(())
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        let (conn, screen_num) = crate::process::linux_x11_conn()?;
        let root = conn.setup().roots[screen_num].root;

        for c in text.chars() {
            // Only ASCII printable — Latin-1 keysyms equal their codepoint
            // for this range, and D2's Name/Password/Description fields
            // are ASCII-restricted anyway.
            if !(0x20..=0x7e).contains(&(c as u32)) {
                continue;
            }
            let keysym = c as u32;
            let Some((keycode, shift)) = keycode_for_keysym(conn, keysym) else {
                continue;
            };
            send_keycode(conn, root, keycode, shift)?;
        }
        Ok(())
    }

    pub fn send_special(key: SpecialKey) -> Result<(), String> {
        let (conn, screen_num) = crate::process::linux_x11_conn()?;
        let root = conn.setup().roots[screen_num].root;
        let keysym = match key {
            SpecialKey::Tab => XK_TAB,
        };
        let Some((keycode, shift)) = keycode_for_keysym(conn, keysym) else {
            return Err(format!("no keycode bound for keysym {:#x}", keysym));
        };
        send_keycode(conn, root, keycode, shift)
    }
}

#[cfg(target_os = "linux")]
pub use linux_impl::{send_special, type_text};

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::SpecialKey;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_TAB,
    };

    /// See `linux_impl::KEY_PACING` — same reasoning: D2's own hand-rolled
    /// UI processes events per game tick, not per Windows message, so a
    /// zero-delay burst of `SendInput` calls can arrive faster than the
    /// game consumes them.
    const KEY_PACING: std::time::Duration = std::time::Duration::from_millis(12);

    fn send_vk(vk: VIRTUAL_KEY) -> Result<(), String> {
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut up = down;
        up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        let inputs = [down, up];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            return Err("SendInput did not send all events".to_string());
        }
        std::thread::sleep(KEY_PACING);
        Ok(())
    }

    fn send_unicode_char(c: char) -> Result<(), String> {
        let mut buf = [0u16; 2];
        for &unit in c.encode_utf16(&mut buf).iter() {
            let down = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let mut up = down;
            up.Anonymous.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
            let inputs = [down, up];
            let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
            if sent != inputs.len() as u32 {
                return Err("SendInput did not send all events".to_string());
            }
            std::thread::sleep(KEY_PACING);
        }
        Ok(())
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        for c in text.chars() {
            send_unicode_char(c)?;
        }
        Ok(())
    }

    pub fn send_special(key: SpecialKey) -> Result<(), String> {
        let vk = match key {
            SpecialKey::Tab => VK_TAB,
        };
        send_vk(vk)
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::{send_special, type_text};

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn type_text(_text: &str) -> Result<(), String> {
    Err("keystroke synthesis is only supported on Windows and Linux".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn send_special(_key: SpecialKey) -> Result<(), String> {
    Err("keystroke synthesis is only supported on Windows and Linux".to_string())
}
