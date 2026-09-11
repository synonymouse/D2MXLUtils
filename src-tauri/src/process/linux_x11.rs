/// Window title the game uses (confirmed via `xprop`/`wmctrl` against a
/// live session — the X11 `WM_CLASS` Wine assigns is not stable/unique
/// ("steam_proton" was observed even for a non-Steam Wine build), so we
/// match on the window title instead, same string used on Windows.
pub const WINDOW_TITLE: &str = "Diablo II";

/// Process-wide shared X11 connection. Every window-lookup/focus-check
/// helper here — plus the key queries in `hotkeys/linux.rs` and the
/// click-through toggle in `app/windows.rs` — used to open (and immediately
/// drop) a brand new connection on every single call. Under concurrent
/// load from ~5 hotkey-poll threads (each polling every ~30ms) plus
/// the 250ms overlay sync tick, that's 100+ connection handshakes per
/// second, which was enough to produce genuinely flaky reads (overlay
/// visibility flapping, click-through state going stale) rather than
/// just being wasteful. `RustConnection` is internally synchronized
/// (see its own module docs) and explicitly designed to be shared
/// across threads, so one connection for the whole process is both
/// correct and far cheaper.
pub fn x11_conn() -> Result<(&'static x11rb::rust_connection::RustConnection, usize), String> {
    static CONN: std::sync::OnceLock<
        Result<(x11rb::rust_connection::RustConnection, usize), String>,
    > = std::sync::OnceLock::new();
    match CONN
        .get_or_init(|| x11rb::connect(None).map_err(|e| format!("X11 connection failed: {}", e)))
    {
        Ok((conn, screen_num)) => Ok((conn, *screen_num)),
        Err(e) => Err(e.clone()),
    }
}

/// Find the real host PID of the window titled `title`, by walking
/// `_NET_CLIENT_LIST` and reading `_NET_WM_PID` off the matching window.
pub fn find_pid_by_window_title(title: &str) -> Result<u32, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_client_list = intern("_NET_CLIENT_LIST")?;
    let net_wm_pid = intern("_NET_WM_PID")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;

    let list_reply = conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
        .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

    let windows: Vec<u32> = list_reply
        .value32()
        .map(|it| it.collect())
        .unwrap_or_default();

    for win in windows {
        // Prefer the EWMH UTF-8 title; fall back to legacy WM_NAME.
        let name = conn
            .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            });

        if name.as_deref() != Some(title) {
            continue;
        }

        let pid_reply = conn
            .get_property(false, win, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .map_err(|e| format!("_NET_WM_PID request failed: {}", e))?
            .reply()
            .map_err(|e| format!("_NET_WM_PID reply failed: {}", e))?;

        let pid = pid_reply.value32().and_then(|mut pids| pids.next());
        if let Some(pid) = pid {
            return Ok(pid);
        }
    }

    Err(format!("Window titled '{}' not found", title))
}

/// Find the on-screen rectangle `(x, y, width, height)` of the window
/// titled `title`, in absolute root-window coordinates. Used to anchor
/// the notification overlay to the actual game window rather than a
/// screen/monitor corner — the game is often windowed, not filling its
/// monitor, so a monitor-corner anchor can end up a long way from the
/// game window itself on an unusual multi-monitor layout.
pub fn find_window_rect_by_title(title: &str) -> Result<(i32, i32, u32, u32), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_client_list = intern("_NET_CLIENT_LIST")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;

    let list_reply = conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
        .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

    let windows: Vec<u32> = list_reply
        .value32()
        .map(|it| it.collect())
        .unwrap_or_default();

    for win in windows {
        let name = conn
            .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            });

        if name.as_deref() != Some(title) {
            continue;
        }

        let geom = conn
            .get_geometry(win)
            .map_err(|e| format!("get_geometry failed: {}", e))?
            .reply()
            .map_err(|e| format!("get_geometry reply failed: {}", e))?;

        // `get_geometry` gives size + position relative to the window's
        // immediate parent (often a WM-added decoration frame, not the
        // root) — translate (0, 0) into root-relative coordinates to
        // get the window's true on-screen position.
        let translated = conn
            .translate_coordinates(win, root, 0, 0)
            .map_err(|e| format!("translate_coordinates failed: {}", e))?
            .reply()
            .map_err(|e| format!("translate_coordinates reply failed: {}", e))?;

        return Ok((
            translated.dst_x as i32,
            translated.dst_y as i32,
            geom.width as u32,
            geom.height as u32,
        ));
    }

    Err(format!("Window titled '{}' not found", title))
}

/// Ask the window manager to activate (raise + focus) the window
/// titled `title`, via the standard EWMH `_NET_ACTIVE_WINDOW`
/// client-message request (the same mechanism `wmctrl -a` uses) rather
/// than an `XSetInputFocus` call directly — going through the WM keeps
/// its own stacking/focus bookkeeping consistent, which a raw focus
/// call can desync from. Used to hand keyboard focus explicitly back
/// to the D2 window when an overlay panel (edit mode / loot history /
/// item search) closes, instead of relying on the WM's implicit
/// behavior when the overlay unmaps — that implicit behavior isn't
/// guaranteed under every focus policy and was the root cause of focus
/// visibly "swapping" between the overlay and the game after closing
/// a panel.
pub fn activate_window_by_title(title: &str) -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt, EventMask,
    };

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_client_list = intern("_NET_CLIENT_LIST")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;
    let net_active_window = intern("_NET_ACTIVE_WINDOW")?;

    let list_reply = conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
        .map_err(|e| format!("_NET_CLIENT_LIST request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_CLIENT_LIST reply failed: {}", e))?;

    let windows: Vec<u32> = list_reply
        .value32()
        .map(|it| it.collect())
        .unwrap_or_default();

    for win in windows {
        let name = conn
            .get_property(false, win, net_wm_name, utf8_string, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                    .ok()
                    .and_then(|c| c.reply().ok())
                    .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            });

        if name.as_deref() != Some(title) {
            continue;
        }

        // source indication = 2 ("pager/other utility"), not 1
        // ("application activating its own window"). We're a separate
        // process handing focus to a *different* application's window
        // on the user's behalf — the same role a taskbar/pager plays,
        // not D2 reclaiming itself. This matters in practice, not just
        // semantically: KWin (confirmed live) applies real
        // focus-stealing-prevention scrutiny to source=1 requests —
        // weighed against the requesting app's own recent-user-input
        // timestamp — which this process has none of, since the user
        // never actually interacts with it directly. That scrutiny is
        // lenient with no competing focus history (works right after
        // launch) but starts silently ignoring the request once real
        // focus history exists (e.g. after alt-tabbing away and back),
        // which reproduced as an unrecoverable focus-flip loop: our
        // "give focus back to D2" request gets dropped, the overlay
        // (which the WM did auto-focus on map) reads as focused, we
        // hide it, focus reverts to D2, next tick shows the overlay
        // again, repeat — until a real user click legitimizes a
        // request. source=2 is the pager path, which WMs are expected
        // to honor without that scrutiny — timestamp = 0 (unknown) is
        // normal for it since pagers don't have a "last user event" of
        // their own; requestor's currently-active window = 0
        // (unknown/not tracked here).
        let event = ClientMessageEvent::new(
            32,
            win,
            net_active_window,
            ClientMessageData::from([2u32, 0, 0, 0, 0]),
        );
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|e| format!("send_event(_NET_ACTIVE_WINDOW) failed: {}", e))?;
        conn.flush()
            .map_err(|e| format!("X11 flush failed: {}", e))?;
        return Ok(());
    }

    Err(format!("Window titled '{}' not found", title))
}

/// Same as `activate_window_by_title`, but re-issues the request a few
/// times (with a short sleep) until `is_window_focused_by_title`
/// confirms it actually landed, instead of firing once and hoping.
///
/// The single-shot version was found to still lose the focus-flicker
/// race in practice: `overlay.show()` returning doesn't mean KWin has
/// finished mapping/auto-focusing the overlay yet, so an activate call
/// placed immediately after can land *before* the WM's own map-time
/// focus grab — D2 flashes active, then the overlay steals it right
/// back a moment later, and the next poll tick sees D2 unfocused again.
/// Retrying for a short window absorbs that ordering race without
/// switching the whole sync loop to be event-driven.
pub fn activate_window_by_title_confirmed(title: &str) -> Result<(), String> {
    const MAX_ATTEMPTS: u32 = 6;
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(15);

    let mut last_err = None;
    for attempt in 0..MAX_ATTEMPTS {
        match activate_window_by_title(title) {
            Ok(()) => {
                std::thread::sleep(RETRY_DELAY);
                if is_window_focused_by_title(title).unwrap_or(false) {
                    return Ok(());
                }
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(RETRY_DELAY);
            }
        }
        let _ = attempt;
    }
    Err(last_err.unwrap_or_else(|| {
        format!(
            "activate_window_by_title_confirmed: '{}' never confirmed focused after {} attempts",
            title, MAX_ATTEMPTS
        )
    }))
}

/// Whether the window titled `title` is currently the active/focused
/// window, via `_NET_ACTIVE_WINDOW` on the root window. Used to hide
/// the overlay when the user has switched away from the game, rather
/// than leaving a stale notification toast drawn over whatever else
/// they're looking at.
pub fn is_window_focused_by_title(title: &str) -> Result<bool, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;

    let active_reply = conn
        .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

    let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
        return Ok(false);
    };
    if active == 0 {
        return Ok(false);
    }

    let name = conn
        .get_property(false, active, net_wm_name, utf8_string, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            conn.get_property(false, active, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        });

    Ok(name.as_deref() == Some(title))
}

/// Whether the currently active/focused window is either the window
/// titled `title` (the game) or one of our own app's windows (matched
/// by `_NET_WM_PID` against our own PID — this process draws several
/// top-level webviews, e.g. the overlay and any open item-search/loot-
/// history popovers, and hotkeys like edit-mode/loot-history should
/// still fire while one of those has focus, not just the game window).
pub fn is_d2_or_own_window_focused(title: &str) -> Result<bool, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
    let net_wm_pid = intern("_NET_WM_PID")?;
    let net_wm_name = intern("_NET_WM_NAME")?;
    let utf8_string = intern("UTF8_STRING")?;

    let active_reply = conn
        .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

    let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
        return Ok(false);
    };
    if active == 0 {
        return Ok(false);
    }

    let pid = conn
        .get_property(false, active, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().and_then(|mut p| p.next()));
    if pid == Some(std::process::id()) {
        return Ok(true);
    }

    let name = conn
        .get_property(false, active, net_wm_name, utf8_string, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            conn.get_property(false, active, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
        });

    Ok(name.as_deref() == Some(title))
}

/// Whether the currently active/focused window belongs to our own
/// process (matched by `_NET_WM_PID`). Used to keep the overlay window
/// alive while it holds keyboard focus itself — e.g. while the user is
/// typing into the item-search box — rather than the game-focus poll
/// hiding it out from under them the moment focus leaves D2.
pub fn is_own_window_focused() -> Result<bool, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

    let (conn, screen_num) = x11_conn()?;
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &str| -> Result<u32, String> {
        Ok(conn
            .intern_atom(false, name.as_bytes())
            .map_err(|e| format!("intern_atom({name}) failed: {e}"))?
            .reply()
            .map_err(|e| format!("intern_atom({name}) reply failed: {e}"))?
            .atom)
    };

    let net_active_window = intern("_NET_ACTIVE_WINDOW")?;
    let net_wm_pid = intern("_NET_WM_PID")?;

    let active_reply = conn
        .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| format!("_NET_ACTIVE_WINDOW request failed: {}", e))?
        .reply()
        .map_err(|e| format!("_NET_ACTIVE_WINDOW reply failed: {}", e))?;

    let Some(active) = active_reply.value32().and_then(|mut w| w.next()) else {
        return Ok(false);
    };
    if active == 0 {
        return Ok(false);
    }

    let pid = conn
        .get_property(false, active, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().and_then(|mut p| p.next()));

    Ok(pid == Some(std::process::id()))
}
