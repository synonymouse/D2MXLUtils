//! Overlay synchronization, focus, hit testing and native style policy.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[cfg(target_os = "linux")]
use super::is_diablo2_running;
use crate::logger::error as log_error;

#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HWND, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowLongW, GetWindowRect, IsIconic, MoveWindow,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOW, SW_SHOWNA, WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
    WS_THICKFRAME,
};

#[tauri::command]
pub(crate) fn set_overlay_interactive(
    app: AppHandle,
    active: bool,
    keyboard_active: bool,
) -> Result<(), String> {
    OVERLAY_CLICK_THROUGH.store(!active, Ordering::SeqCst);
    OVERLAY_KEYBOARD_INTERACTIVE.store(keyboard_active, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let _ = sync_overlay_with_game_impl(&app);
        if overlay_should_force_foreground(active, keyboard_active) {
            force_overlay_foreground(&app);
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Overlay is shown non-focusable/click-through by default (see
        // `sync_overlay_with_game_impl_linux`) to avoid a focus-flicker
        // loop against the game and to not block clicks into it. Any
        // interactive panel (item search, edit-mode drag, loot history)
        // needs mouse clicks to actually land on it, and unlike Windows'
        // WS_EX_NOACTIVATE (which guarantees click delivery to a
        // non-activatable window), there's no such guarantee under an
        // arbitrary X11 WM — so `active` (not just `keyboard_active`)
        // toggles focusable here, not only the item-search typing case.
        // `overlay_should_be_visible` (used by the sync call below) keeps
        // the overlay from being hidden out from under the user once it
        // holds focus itself. Call the sync directly (not just wait for
        // the next 250ms poll) so the click-through/focus change applies
        // immediately, same as the Windows branch above.
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.set_focusable(active);
        }
        let _ = sync_overlay_with_game_impl_linux(&app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = &app;
    }
    // Windows only force-focuses for the keyboard case (WS_EX_NOACTIVATE
    // already guarantees mouse-only interaction like edit-mode dragging
    // works without taking foreground — forcing it there would just
    // steal keyboard focus from the game for no reason). Linux has no
    // such guarantee: some WMs swallow a window's *first* click after a
    // focus change (using it only to activate/raise the window, not
    // deliver it to the app), which showed up as "the first drag click
    // after the game regains focus does nothing, second click works" —
    // so proactively focus the overlay the moment any panel goes
    // interactive, before the user's own click would have had to do it.
    #[cfg(target_os = "linux")]
    let should_focus = active;
    #[cfg(not(target_os = "linux"))]
    let should_focus = active && keyboard_active;
    if should_focus {
        if let Some(overlay) = app.get_webview_window("overlay") {
            if let Err(e) = overlay.set_focus() {
                log_error(&format!("Failed to focus overlay window: {}", e));
            }
        }
    }
    // Closing a panel (active -> false) leaves keyboard focus wherever the
    // overlay panel put it. On Windows, WS_EX_NOACTIVATE means mouse-only
    // panels never took focus in the first place and the keyboard case is
    // handled by `force_overlay_foreground`'s counterpart elsewhere; on
    // Linux there's no such guarantee, and the overlay hiding itself
    // (`sync_overlay_with_game_impl_linux`) only *implicitly* returns focus
    // to D2 via the WM's own unmap-focus behavior — not guaranteed under
    // every focus policy, and was the actual root cause of focus visibly
    // swapping between the overlay and the game after closing a panel. So
    // explicitly hand focus back to D2 here instead of hoping the WM does it.
    #[cfg(target_os = "linux")]
    if !should_focus && is_diablo2_running() {
        if let Err(e) = crate::process::linux_activate_window_by_title_confirmed(
            crate::process::LINUX_WINDOW_TITLE,
        ) {
            log_error(&format!("Failed to refocus D2 window: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn set_overlay_edit_mode(app: AppHandle, active: bool) -> Result<(), String> {
    OVERLAY_EDIT_ACTIVE.store(active, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    {
        let _ = sync_overlay_with_game_impl(&app);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = sync_overlay_with_game_impl_linux(&app);
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn sync_overlay_with_game(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        sync_overlay_with_game_impl(&app)
    }

    #[cfg(target_os = "linux")]
    {
        sync_overlay_with_game_impl_linux(&app)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
        Err("Overlay sync is not supported on this OS".to_string())
    }
}

/// Linux overlay, matching the Windows design: the overlay window is
/// resized/repositioned to exactly cover the game window every tick (not
/// just once on first show — the game can move/resize), and made
/// click-through via the X11 SHAPE extension's input region unless a
/// panel (item search, edit mode, loot history) is actually open. This
/// is the Linux analog of Windows' live `WS_EX_LAYERED`/`WS_EX_TRANSPARENT`
/// toggling (see `docs/overlay-reposition-hittest-bug.md` for the
/// Windows-side version of this problem). Percentage-based widget
/// positions (`ItemSearchOverlay.svelte` etc.) assume the overlay spans
/// the full game window, same as on Windows — a smaller fixed-size corner
/// toast (the original v1 approach here) clips them.
#[cfg(target_os = "linux")]
fn sync_overlay_with_game_impl_linux(app: &AppHandle) -> Result<(), String> {
    const MARGIN: i32 = 16;
    const FALLBACK_WIDTH: u32 = 1024;
    const FALLBACK_HEIGHT: u32 = 768;

    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "overlay window not found".to_string())?;

    // Only draw while D2 is both running *and* the focused window — no
    // point covering the screen with a game overlay while the user has
    // switched to something else. Exception: while the overlay itself is
    // in an interactive panel (item search, edit-mode drag, loot history),
    // it may hold real X11 focus (typing) or just get raised/activated by
    // the WM as a side effect of a plain click on it (edit-mode dragging
    // never explicitly requests focus, but some WMs activate a window on
    // click regardless) — either way that makes this tick look identical
    // to "the user alt-tabbed away", hiding the overlay out from under a
    // mid-drag/mid-search user. `panel_active` (not just the narrower
    // `keyboard_interactive`) is what actually protects against that;
    // `overlay_should_be_visible` (shared with the Windows path) applies
    // it via `foreground_matches_overlay`.
    let running = is_diablo2_running();
    let d2_focused = running
        && crate::process::linux_is_window_focused_by_title(crate::process::LINUX_WINDOW_TITLE)
            .unwrap_or(false);
    let own_focused = crate::process::linux_is_own_window_focused().unwrap_or(false);
    let panel_active = !OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
    let focused = overlay_should_be_visible(d2_focused, own_focused, !running, panel_active);
    let was_visible = OVERLAY_WAS_VISIBLE.swap(focused, Ordering::SeqCst);

    if !focused {
        if was_visible {
            overlay
                .hide()
                .map_err(|e| format!("Failed to hide overlay: {}", e))?;
            if let Ok(mut last) = OVERLAY_LAST_RECT_LINUX.lock() {
                *last = None;
            }
            // Force a fresh click-through/input-shape application on the
            // next show — the "last applied" cache surviving a hide/show
            // cycle caused a real bug: if edit mode was already captured
            // before a transient focus-loss hide, the desired state looks
            // unchanged after reshowing, so the code skipped reapplying
            // the X11 SHAPE input region even though it may not have
            // survived the hide/show cycle intact, leaving clicks falling
            // through to the game until the user released and re-pressed
            // the edit-mode chord (which forces reapplication via a
            // logical state change).
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(-1, Ordering::SeqCst);
        }
        return Ok(());
    }

    // Anchor/size to the actual game window, not just its monitor — the
    // game is often windowed and doesn't fill the monitor, so a
    // monitor-corner anchor can land well away from the game on an
    // unusual multi-monitor layout. Requires XWayland (forced before
    // desktop initialization): native Wayland gives clients no control
    // over top-level window position/size at all.
    let game_rect =
        crate::process::linux_find_window_rect_by_title(crate::process::LINUX_WINDOW_TITLE).ok();
    let (x, y, width, height) = game_rect.unwrap_or_else(|| {
        let fallback_pos = overlay
            .primary_monitor()
            .ok()
            .flatten()
            .or_else(|| overlay.current_monitor().ok().flatten())
            .or_else(|| {
                overlay.available_monitors().ok().and_then(|mut m| {
                    if m.is_empty() {
                        None
                    } else {
                        Some(m.remove(0))
                    }
                })
            })
            .map(|m| *m.position())
            .unwrap_or(tauri::PhysicalPosition { x: 0, y: 0 });
        (
            fallback_pos.x + MARGIN,
            fallback_pos.y + MARGIN,
            FALLBACK_WIDTH,
            FALLBACK_HEIGHT,
        )
    });

    if !was_visible {
        // Without this, showing the overlay hands it keyboard focus (most
        // WMs auto-focus newly-mapped windows), which our own focus check
        // above then reads as "D2 lost focus" on the very next 250ms tick
        // — hide, which returns focus to D2 — which we read as "D2 focused
        // again" — show — repeat, flickering forever. `focusable(false)` is
        // a runtime-only call (this Linux code path is the only caller),
        // so it doesn't touch the shared `tauri.conf.json` window config
        // Windows' own dynamic WS_EX_NOACTIVATE toggling still relies on.
        let _ = overlay.set_focusable(panel_active);
    }

    let needs_move = OVERLAY_LAST_RECT_LINUX
        .lock()
        .ok()
        .map(|guard| *guard != Some((x, y, width, height)))
        .unwrap_or(true);
    if needs_move {
        let _ = overlay.set_size(tauri::Size::Physical(tauri::PhysicalSize { width, height }));
        let _ = overlay.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        if let Ok(mut last) = OVERLAY_LAST_RECT_LINUX.lock() {
            *last = Some((x, y, width, height));
        }
    }

    if !was_visible {
        overlay
            .show()
            .map_err(|e| format!("Failed to show overlay: {}", e))?;
        if panel_active {
            // Reshowing mid-interactive-session (e.g. the game regained
            // focus while edit mode was still logically active) needs the
            // same proactive focus `set_overlay_interactive` does on the
            // initial transition into an interactive panel — otherwise
            // the WM treats the user's next click as "just activate the
            // window" and swallows it instead of delivering it as a drag
            // start.
            if let Err(e) = overlay.set_focus() {
                log_error(&format!("Failed to focus overlay window on reshow: {}", e));
            }
        } else if running {
            // `set_focusable(false)` above is meant to stop the WM from
            // handing the newly-mapped overlay focus in the first place,
            // but that's just an advisory ICCCM hint — confirmed live
            // (KWin) to not be honored reliably at map time, which
            // reproduces exactly the flicker loop described above: show
            // steals focus, next tick reads "D2 lost focus", hide,
            // "D2 focused again", show, repeat — happening right at
            // launch, before any panel is ever touched. Deterministically
            // reassert D2 as focused immediately after showing, the same
            // way `set_overlay_interactive` already does when a panel
            // closes, instead of trusting the hint alone.
            if let Err(e) = crate::process::linux_activate_window_by_title_confirmed(
                crate::process::LINUX_WINDOW_TITLE,
            ) {
                log_error(&format!(
                    "Failed to refocus D2 window after showing overlay: {}",
                    e
                ));
            }
        }
    }

    // Click-through unless a panel is actually open (edit mode / item
    // search / loot history) — `panel_active` (computed above from
    // `OVERLAY_CLICK_THROUGH`, updated by `set_overlay_interactive`) is
    // the same signal Windows' WS_EX_TRANSPARENT toggle uses.
    let desired_click_through = !panel_active;
    let desired_i8: i8 = if desired_click_through { 1 } else { 0 };
    if OVERLAY_LAST_CLICK_THROUGH_APPLIED.swap(desired_i8, Ordering::SeqCst) != desired_i8 {
        set_overlay_click_through_linux(&overlay, desired_click_through, width, height);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
static OVERLAY_LAST_RECT_LINUX: Mutex<Option<(i32, i32, u32, u32)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn overlay_xid(window: &tauri::WebviewWindow) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        _ => None,
    }
}

/// Toggle whether the overlay window intercepts mouse input, via the X11
/// SHAPE extension's input region — the Linux analog of Windows'
/// WS_EX_TRANSPARENT toggling. An empty input region makes the whole
/// window click-through (events fall through to whatever's behind it,
/// i.e. the game); a single rect covering the window makes it capture
/// input normally, needed while an interactive panel is open.
#[cfg(target_os = "linux")]
fn set_overlay_click_through_linux(
    window: &tauri::WebviewWindow,
    click_through: bool,
    width: u32,
    height: u32,
) {
    use x11rb::protocol::shape::{self, SK, SO};
    use x11rb::protocol::xproto::{ClipOrdering, Rectangle};

    let Some(xid) = overlay_xid(window) else {
        return;
    };
    let Ok((conn, _)) = crate::process::linux_x11_conn() else {
        return;
    };

    let rects: Vec<Rectangle> = if click_through {
        Vec::new()
    } else {
        vec![Rectangle {
            x: 0,
            y: 0,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
        }]
    };

    let cookie = match shape::rectangles(
        conn,
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        xid,
        0,
        0,
        &rects,
    ) {
        Ok(cookie) => cookie,
        Err(e) => {
            log_error(&format!(
                "overlay click-through: shape::rectangles request failed: {}",
                e
            ));
            return;
        }
    };
    if let Err(e) = cookie.check() {
        log_error(&format!(
            "overlay click-through: shape::rectangles failed: {}",
            e
        ));
    }
}

static OVERLAY_WAS_VISIBLE: AtomicBool = AtomicBool::new(false);
static OVERLAY_CLICK_THROUGH: AtomicBool = AtomicBool::new(true);
static OVERLAY_KEYBOARD_INTERACTIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY_STYLES_APPLIED: AtomicBool = AtomicBool::new(false);
static OVERLAY_EDIT_ACTIVE: AtomicBool = AtomicBool::new(false);

// -1 sentinel = never applied; forces first sync to push the style.
static OVERLAY_LAST_CLICK_THROUGH_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);
static OVERLAY_LAST_EDIT_MODE_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);
static OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED: std::sync::atomic::AtomicI8 =
    std::sync::atomic::AtomicI8::new(-1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayWindowKind {
    Visual,
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OverlayWindowSpec {
    label: &'static str,
    title: &'static str,
    layered: bool,
    click_through: bool,
}

fn overlay_window_spec(kind: OverlayWindowKind) -> OverlayWindowSpec {
    match kind {
        OverlayWindowKind::Visual => OverlayWindowSpec {
            label: "overlay",
            title: "D2MXLUtils Overlay",
            layered: true,
            click_through: true,
        },
        OverlayWindowKind::Edit => OverlayWindowSpec {
            label: "overlay",
            title: "D2MXLUtils Overlay",
            layered: false,
            click_through: false,
        },
    }
}

fn overlay_should_be_visible(
    foreground_matches_game: bool,
    foreground_matches_overlay: bool,
    game_minimized: bool,
    keyboard_interactive: bool,
) -> bool {
    !game_minimized
        && (foreground_matches_game || (keyboard_interactive && foreground_matches_overlay))
}

fn overlay_should_use_noactivate(keyboard_interactive: bool) -> bool {
    !keyboard_interactive
}

fn overlay_should_force_foreground(active: bool, keyboard_active: bool) -> bool {
    active && keyboard_active
}

fn overlay_window_kind_for_state(edit_active: bool, click_through: bool) -> OverlayWindowKind {
    if edit_active || !click_through {
        OverlayWindowKind::Edit
    } else {
        OverlayWindowKind::Visual
    }
}

fn overlay_style_needs_update(
    just_applied: bool,
    last_click_through: i8,
    desired_click_through: i8,
    last_edit_mode: i8,
    desired_edit_mode: i8,
    last_keyboard_interactive: i8,
    desired_keyboard_interactive: i8,
) -> bool {
    just_applied
        || last_click_through != desired_click_through
        || last_edit_mode != desired_edit_mode
        || last_keyboard_interactive != desired_keyboard_interactive
}

fn overlay_should_strip_chrome(_style_changed: bool, chrome_present: bool) -> bool {
    chrome_present
}

#[cfg(target_os = "windows")]
fn overlay_chrome_mask() -> i32 {
    (WS_CAPTION.0
        | WS_BORDER.0
        | WS_DLGFRAME.0
        | WS_THICKFRAME.0
        | WS_SYSMENU.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0) as i32
}

#[cfg(target_os = "windows")]
static OVERLAY_LAST_RECT: Mutex<Option<RECT>> = Mutex::new(None);

#[cfg(target_os = "windows")]
fn force_overlay_foreground(app: &AppHandle) {
    let spec = overlay_window_spec(OverlayWindowKind::Visual);
    let Some(overlay) = app.get_webview_window(spec.label) else {
        return;
    };

    let hwnd_overlay = match overlay.hwnd() {
        Ok(hwnd) => hwnd,
        Err(e) => {
            log_error(&format!(
                "Failed to get overlay HWND for foreground activation: {}",
                e
            ));
            return;
        }
    };
    let hwnd_overlay = HWND(hwnd_overlay.0 as _);

    if hwnd_overlay.0.is_null() {
        return;
    }

    unsafe {
        if !SetForegroundWindow(hwnd_overlay).as_bool() {
            log_error("Failed to activate overlay foreground for item search");
        }
    }
}

#[cfg(target_os = "windows")]
fn reset_overlay_runtime_state() {
    OVERLAY_WAS_VISIBLE.store(false, Ordering::SeqCst);
    OVERLAY_STYLES_APPLIED.store(false, Ordering::SeqCst);
    OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(-1, Ordering::SeqCst);
    OVERLAY_LAST_EDIT_MODE_APPLIED.store(-1, Ordering::SeqCst);
    OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED.store(-1, Ordering::SeqCst);
    if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
        *last = None;
    }
}

#[cfg(test)]
#[path = "overlay_window_tests.rs"]
mod overlay_window_tests;

#[cfg(target_os = "windows")]
fn sync_overlay_with_game_impl(app: &AppHandle) -> Result<(), String> {
    let visual_spec = overlay_window_spec(OverlayWindowKind::Visual);
    let class_wide: Vec<u16> = OsStr::new("Diablo II")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_game =
        unsafe { FindWindowW(PCWSTR(class_wide.as_ptr()), PCWSTR::null()) }.map_err(|_| {
            "Diablo II window not found (class 'Diablo II'). Is the game running?".to_string()
        })?;

    if hwnd_game.0.is_null() {
        return Err("Diablo II window handle is null".to_string());
    }

    let overlay_window = app.get_webview_window(visual_spec.label).ok_or(format!(
        "Overlay window with label '{}' not found",
        visual_spec.label
    ))?;

    let title_wide: Vec<u16> = OsStr::new(visual_spec.title)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd_overlay = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr())) }
        .map_err(|_| format!("Overlay OS window '{}' not found", visual_spec.title))?;

    if hwnd_overlay.0.is_null() {
        return Err("Overlay HWND is null".to_string());
    }

    unsafe {
        let fg = GetForegroundWindow();
        let game_minimized = IsIconic(hwnd_game).as_bool();
        let keyboard_interactive = OVERLAY_KEYBOARD_INTERACTIVE.load(Ordering::SeqCst);
        if !overlay_should_be_visible(
            fg.0 == hwnd_game.0,
            fg.0 == hwnd_overlay.0,
            game_minimized,
            keyboard_interactive,
        ) {
            let _ = ShowWindow(hwnd_overlay, SW_HIDE);
            let _ = overlay_window.hide();
            reset_overlay_runtime_state();
            return Ok(());
        }
    }

    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd_game, &mut rect).map_err(|e| format!("GetWindowRect failed: {}", e))?;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    let was_visible = OVERLAY_WAS_VISIBLE.swap(true, Ordering::SeqCst);

    unsafe {
        // WS_EX_NOACTIVATE prevents the overlay from ever stealing foreground
        // from the game — without it, alt-tabbing back triggers a focus war
        // that flickers the screen edges and steals mouse input.
        let just_applied = !OVERLAY_STYLES_APPLIED.swap(true, Ordering::SeqCst);
        let edit_active = OVERLAY_EDIT_ACTIVE.load(Ordering::SeqCst);
        let keyboard_interactive = OVERLAY_KEYBOARD_INTERACTIVE.load(Ordering::SeqCst);
        let click_through = OVERLAY_CLICK_THROUGH.load(Ordering::SeqCst);
        let mode_spec =
            overlay_window_spec(overlay_window_kind_for_state(edit_active, click_through));
        let desired_ct = mode_spec.click_through && click_through;
        let desired_ct_i8 = if desired_ct { 1 } else { 0 };
        let edit_active_i8 = if edit_active { 1 } else { 0 };
        let keyboard_interactive_i8 = if keyboard_interactive { 1 } else { 0 };
        let needs_style = overlay_style_needs_update(
            just_applied,
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.load(Ordering::SeqCst),
            desired_ct_i8,
            OVERLAY_LAST_EDIT_MODE_APPLIED.load(Ordering::SeqCst),
            edit_active_i8,
            OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED.load(Ordering::SeqCst),
            keyboard_interactive_i8,
        );
        if needs_style {
            let ex_style = GetWindowLongW(hwnd_overlay, GWL_EXSTYLE);
            let mut new_ex = ex_style | WS_EX_TOOLWINDOW.0 as i32;
            if overlay_should_use_noactivate(keyboard_interactive) {
                new_ex |= WS_EX_NOACTIVATE.0 as i32;
            } else {
                new_ex &= !(WS_EX_NOACTIVATE.0 as i32);
            }
            if mode_spec.layered {
                new_ex |= WS_EX_LAYERED.0 as i32;
            } else {
                new_ex &= !(WS_EX_LAYERED.0 as i32);
            }
            if desired_ct {
                new_ex |= WS_EX_TRANSPARENT.0 as i32;
            } else {
                new_ex &= !(WS_EX_TRANSPARENT.0 as i32);
            }
            SetWindowLongW(hwnd_overlay, GWL_EXSTYLE, new_ex);
            OVERLAY_LAST_CLICK_THROUGH_APPLIED.store(desired_ct_i8, Ordering::SeqCst);
            OVERLAY_LAST_EDIT_MODE_APPLIED.store(edit_active_i8, Ordering::SeqCst);
            OVERLAY_LAST_KEYBOARD_INTERACTIVE_APPLIED
                .store(keyboard_interactive_i8, Ordering::SeqCst);

            // Suppress the 1px Win11 DWM accent frame; ignored on Win10.
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let _ = DwmSetWindowAttribute(
                hwnd_overlay,
                DWMWA_BORDER_COLOR,
                &DWMWA_COLOR_NONE as *const u32 as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }

        // Tauri/Windows can reintroduce caption bits when transparency styles
        // change. Re-strip them every sync so a title bar cannot survive until
        // the next edit-mode transition.
        let style = GetWindowLongW(hwnd_overlay, GWL_STYLE);
        let chrome_mask = overlay_chrome_mask();
        if overlay_should_strip_chrome(needs_style, (style & chrome_mask) != 0) {
            let new_style = (style & !chrome_mask) | WS_POPUP.0 as i32;
            SetWindowLongW(hwnd_overlay, GWL_STYLE, new_style);
            let _ = SetWindowPos(
                hwnd_overlay,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }

        // WebView2 only commits transparency on a resize, so on the first show
        // we resize by 1px and back. SW_SHOWNA (not Tauri's show(), which uses
        // SW_SHOW) — SW_SHOW would activate and steal focus from the game.
        if !was_visible {
            let _ = MoveWindow(
                hwnd_overlay,
                rect.left,
                rect.top,
                width + 1,
                height + 1,
                BOOL(1),
            );
            let show_cmd = if keyboard_interactive {
                SW_SHOW
            } else {
                SW_SHOWNA
            };
            let _ = ShowWindow(hwnd_overlay, show_cmd);
            let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
            if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
                *last = Some(rect);
            }
        } else {
            let needs_move = OVERLAY_LAST_RECT
                .lock()
                .ok()
                .map(|guard| match *guard {
                    Some(prev) => {
                        prev.left != rect.left
                            || prev.top != rect.top
                            || prev.right != rect.right
                            || prev.bottom != rect.bottom
                    }
                    None => true,
                })
                .unwrap_or(true);
            if needs_move {
                let _ = MoveWindow(hwnd_overlay, rect.left, rect.top, width, height, BOOL(1));
                if let Ok(mut last) = OVERLAY_LAST_RECT.lock() {
                    *last = Some(rect);
                }
            }
        }

        // No SWP_SHOWWINDOW: that flag forces a frame repaint each tick, which
        // re-flashed the Win11 DWM border and was a major source of the
        // edge-flicker.
        let _ = SetWindowPos(
            hwnd_overlay,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }

    Ok(())
}
