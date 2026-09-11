//! Application controls, window policy, platform preparation, commands and scanner runtime.

mod commands;
mod controls;
mod platform;
mod scanner_runtime;
mod windows;

#[cfg(target_os = "linux")]
use scanner_runtime::is_diablo2_running;
pub(crate) use scanner_runtime::spawn_auto_scanner;

pub(crate) use windows::{
    __cmd__set_overlay_edit_mode, __cmd__set_overlay_interactive, __cmd__sync_overlay_with_game,
    set_overlay_edit_mode, set_overlay_interactive, sync_overlay_with_game,
};

pub(crate) use platform::{
    enable_debug_privilege, prepare_environment, setup_webview2_for_elevation,
};

pub(crate) use commands::{
    __cmd__get_changelog, __cmd__get_game_status, __cmd__get_scanner_status,
    __cmd__open_app_folder, __cmd__open_devtools, __cmd__open_external_url,
    __cmd__refresh_game_data_caches, get_changelog, get_game_status, get_scanner_status,
    open_app_folder, open_devtools, open_external_url, refresh_game_data_caches,
};

pub(crate) use controls::{
    update_edit_mode_hotkey, update_hotkey, EditModeState, HotkeyState,
    __cmd__update_edit_mode_hotkey, __cmd__update_hotkey,
};
