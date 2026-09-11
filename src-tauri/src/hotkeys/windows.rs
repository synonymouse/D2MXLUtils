//! Windows chord and focus predicates; raw key/window queries stay private.

use super::HotkeyConfig;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

fn is_d2_or_app_foreground() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }

        let our_pid = GetCurrentProcessId();
        let mut fg_pid: u32 = 0;
        GetWindowThreadProcessId(fg, Some(&mut fg_pid as *mut u32));
        if fg_pid == our_pid {
            return true;
        }

        let class: Vec<u16> = OsStr::new("Diablo II")
            .encode_wide()
            .chain(Some(0))
            .collect();
        if let Ok(d2) = FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            return !d2.0.is_null() && fg.0 == d2.0;
        }

        false
    }
}

fn is_d2_foreground() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetForegroundWindow};

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let class: Vec<u16> = OsStr::new("Diablo II")
            .encode_wide()
            .chain(Some(0))
            .collect();
        match FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            Ok(d2) => !d2.0.is_null() && fg.0 == d2.0,
            Err(_) => false,
        }
    }
}

fn is_key_down(vk: u16) -> bool {
    // High bit of GetAsyncKeyState is set while the key is held.
    unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 }
}

pub(crate) fn chord_is_pressed(hk: &HotkeyConfig) -> bool {
    if !is_d2_or_app_foreground() {
        return false;
    }

    chord_keys_are_pressed(hk)
}

pub(crate) fn chord_is_pressed_d2_only(hk: &HotkeyConfig) -> bool {
    is_d2_foreground() && chord_keys_are_pressed(hk)
}

pub(crate) fn chord_keys_are_pressed(hk: &HotkeyConfig) -> bool {
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;

    if hk.key_code == 0 && hk.modifiers == 0 {
        return false;
    }

    if hk.modifiers & MOD_CONTROL != 0 && !is_key_down(VK_CONTROL.0) {
        return false;
    }
    if hk.modifiers & MOD_SHIFT != 0 && !is_key_down(VK_SHIFT.0) {
        return false;
    }
    if hk.modifiers & MOD_ALT != 0 && !is_key_down(VK_MENU.0) {
        return false;
    }
    if hk.modifiers & MOD_WIN != 0 && !(is_key_down(VK_LWIN.0) || is_key_down(VK_RWIN.0)) {
        return false;
    }

    if hk.key_code != 0 && !is_key_down(hk.key_code as u16) {
        return false;
    }

    true
}
