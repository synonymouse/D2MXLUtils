//! Shared process attachment and native I/O for scanners, readouts and hooks.

mod context;
pub use context::D2Context;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{open_process_by_window_class, ProcessHandle};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{open_process_by_window_class, ProcessHandle};
#[cfg(target_os = "linux")]
mod linux_x11;
#[cfg(target_os = "linux")]
pub use linux_x11::{
    activate_window_by_title as linux_activate_window_by_title,
    activate_window_by_title_confirmed as linux_activate_window_by_title_confirmed,
    find_window_rect_by_title as linux_find_window_rect_by_title,
    is_d2_or_own_window_focused as linux_is_d2_or_own_window_focused,
    is_own_window_focused as linux_is_own_window_focused,
    is_window_focused_by_title as linux_is_window_focused_by_title, x11_conn as linux_x11_conn,
    WINDOW_TITLE as LINUX_WINDOW_TITLE,
};

#[cfg(target_os = "linux")]
pub(crate) mod linux_ptrace;

#[cfg(all(test, target_os = "linux"))]
mod live_probe;
#[cfg(all(test, target_os = "windows"))]
pub(crate) mod marker_test_io;
