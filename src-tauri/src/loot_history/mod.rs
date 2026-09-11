//! Session-only loot history: items that fired a `notify` rule, with
//! per-entry pickup state resolved against the local player's inventory.
//!
//! `history.rs` keeps LootHistory's pure-data state and transitions. The
//! scanner (`notifier/`) drives them by calling `push`, then `resolve_pending`
//! once per tick with the current inventory ids. `hotkey.rs` owns the toggle
//! watcher, using shared platform input predicates and emitting Tauri events.

mod history;
mod hotkey;

use crate::AppState;
use tauri::{AppHandle, Emitter};

pub(crate) use hotkey::{
    update_loot_history_hotkey, LootHistoryHotkeyState, __cmd__update_loot_history_hotkey,
};

pub use history::LootHistory;

#[tauri::command]
pub(crate) fn get_loot_history(state: tauri::State<AppState>) -> Vec<LootEntry> {
    state
        .loot_history
        .read()
        .map(|h| h.snapshot())
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn clear_loot_history(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if let Ok(mut h) = state.loot_history.write() {
        h.clear();
    }
    app_handle
        .emit("loot-history-cleared", ())
        .map_err(|e| format!("Failed to emit loot-history-cleared: {}", e))
}

/// Maximum entries kept per session. Older entries are evicted FIFO.
pub const MAX_ENTRIES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickupState {
    /// On the ground, or in flight between ground and an inventory, or
    /// out of view (different area, town). Stays Pending until we have
    /// positive evidence of pickup — map changes do NOT auto-transition.
    Pending,
    /// In our local hero's inventory (terminal).
    PickedUp,
    /// Session ended while still Pending (player left the game). Terminal:
    /// no longer reachable from this session.
    Lost,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LootEntry {
    pub unit_id: u32,
    /// Milliseconds since UNIX epoch (set at push time).
    pub timestamp_ms: u64,
    /// Final display name as it appears in the notification.
    pub name: String,
    /// Item quality string (`"Unique"`, `"Set"`, `"Magic"`, …) — used by
    /// the frontend as a color fallback when the winning rule didn't set
    /// an explicit `color` flag, mirroring the in-game notification.
    #[serde(default)]
    pub quality: String,
    /// Lowercase color keyword from the winning rule's `color` flag (e.g.
    /// `"lime"`, `"gold"`). `None` = default color (frontend falls back to
    /// quality color or a neutral foreground).
    pub color: Option<String>,
    pub pickup: PickupState,
    /// `dwSeed` (item random seed) at offset `0x14` of `ItemData`. Stable
    /// per-item across area unload/reload in MP. Used as the dedup key:
    /// when the engine assigns a new `unit_id` to the same physical item
    /// after a teleport-away/return cycle, we re-key the existing entry
    /// instead of creating a duplicate row in the panel. Also serves as
    /// the stable identity for the frontend (the indexable key — `unit_id`
    /// can change underneath us via merge).
    ///
    /// `0` means "unknown" (read failed at push time) → falls back to
    /// `unit_id` for dedup, and the frontend keys by `unit_id` for that row.
    #[serde(default)]
    pub seed: u32,
}

/// Result of [`LootHistory::push`]. `Inserted` and `Merged` both leave
/// a row in the panel; `Duplicate` is a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// New row created in the panel.
    Inserted,
    /// Existing row found by `seed`; its `unit_id` was updated to the new
    /// sighting. No new row produced.
    Merged,
    /// Same `unit_id` already present, or `seed` matches a terminal entry — no-op.
    Duplicate,
}

/// Milliseconds since UNIX epoch. Wall-clock — frontend renders as HH:MM:SS.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
