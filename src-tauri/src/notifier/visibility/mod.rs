//! Item-visibility controls, reveal input, hook and bit accounting for DropScanner.

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod controls;
mod hook;
mod hotkey;
mod tracker;

pub(super) use hook::{visibility_mask_ops, LootFilterHook, VisibilityMaskOp};
pub(super) use tracker::{HookBitTracker, HookCleanupFailureLogThrottle, PendingVisibilityMaskOps};

pub(crate) use hotkey::{
    update_reveal_hidden_hotkey, RevealHiddenState, __cmd__update_reveal_hidden_hotkey,
};
