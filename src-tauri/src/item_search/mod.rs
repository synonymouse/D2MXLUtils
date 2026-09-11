//! Item search: API queries, optional hovered-item capture and search control.

mod api;
mod capture;
mod hotkey;

pub(crate) use api::{search_mxl_items, MxlItemApiState, __cmd__search_mxl_items};
#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) use capture::HoveredItemHook;
pub(crate) use hotkey::{
    update_item_search_hotkey, ItemSearchHotkeyState, __cmd__update_item_search_hotkey,
};
