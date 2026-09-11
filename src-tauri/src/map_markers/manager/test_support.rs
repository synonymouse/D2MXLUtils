//! Shared marker builders; own-process support remains Windows-only.

use super::{sub_to_cell, MarkerItem};

#[cfg(target_os = "windows")]
#[path = "native_test_support.rs"]
pub(crate) mod native;

pub(super) fn mk(uid: u32, sx: i32, sy: i32) -> MarkerItem {
    let (cx, cy) = sub_to_cell(sx, sy);
    MarkerItem {
        unit_id: uid,
        cell_x: cx,
        cell_y: cy,
        sub_x: sx,
        sub_y: sy,
    }
}
