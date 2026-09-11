//! Damage capture, pure accumulation and reset control.

use crate::AppState;
use std::sync::atomic::Ordering;

#[tauri::command]
pub(crate) fn reset_dps_session(state: tauri::State<AppState>) {
    state.dps_reset_pending.store(true, Ordering::SeqCst);
}

mod accumulator;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod hook;
mod hotkey;

pub(crate) use accumulator::DpsMeter;
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) use hook::DpsHook;
pub(crate) use hotkey::{
    update_dps_meter_reset_hotkey, DpsMeterResetHotkeyState, __cmd__update_dps_meter_reset_hotkey,
};
