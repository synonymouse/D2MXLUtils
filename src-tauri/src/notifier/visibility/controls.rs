//! Player item-visibility controls and retryable hook-mask operations.

use super::super::{no_pickup_flag_write, DropScanner};
use super::VisibilityMaskOp;
use crate::logger::error as log_error;
use crate::offsets::d2sigma;
use std::collections::HashSet;

fn visibility_mask_op_description(op: VisibilityMaskOp) -> &'static str {
    match op {
        VisibilityMaskOp::SetShow => "force-show",
        VisibilityMaskOp::SetHide => "hide",
        VisibilityMaskOp::ClearShow => "clear force-show bit for",
        VisibilityMaskOp::ClearHide => "clear hide bit for",
    }
}

impl DropScanner {
    pub fn set_force_show_all(&self, value: bool) -> Result<(), String> {
        if !self.loot_hook.is_injected() {
            return Ok(());
        }
        self.loot_hook.set_force_show_all(&self.state.ctx, value)
    }

    fn always_show_items_addr(&self) -> Result<Option<usize>, String> {
        if self.state.ctx.d2_sigma == 0 {
            return Ok(None);
        }
        let Some(rva) = self.state.ctx.always_show_items_ptr_rva else {
            return Ok(None);
        };
        let base = self.state.ctx.d2_sigma + rva;
        let struct_ptr = self.state.ctx.process.read_memory::<u32>(base)?;
        if struct_ptr == 0 {
            return Ok(None);
        }
        Ok(Some(struct_ptr as usize + d2sigma::ALWAYS_SHOW_ITEMS_FLAG))
    }

    /// Ok(false) = base ptr NULL (caller should retry next tick).
    pub fn set_always_show_items(&self, on: bool) -> Result<bool, String> {
        let Some(addr) = self.always_show_items_addr()? else {
            return Ok(false);
        };
        let value: u32 = if on { 1 } else { 0 };
        self.state
            .ctx
            .process
            .write_buffer(addr, &value.to_le_bytes())?;
        Ok(true)
    }

    /// Ok(None) = struct not allocated yet.
    pub fn read_always_show_items(&self) -> Result<Option<bool>, String> {
        let Some(addr) = self.always_show_items_addr()? else {
            return Ok(None);
        };
        let value = self.state.ctx.process.read_memory::<u32>(addr)?;
        Ok(Some(value != 0))
    }

    pub fn set_no_pickup(&self, on: bool) -> Result<(), String> {
        let (addr, bytes) = no_pickup_flag_write(self.state.ctx.d2_client, on);
        self.state.ctx.process.write_buffer(addr, &bytes)
    }

    pub(in crate::notifier) fn reset_departed_mask_collision(
        &mut self,
        unit_id: u32,
        current_item_ids: &HashSet<u32>,
    ) -> bool {
        let colliding_departed_ids = self
            .hook_bits
            .departed_mask_collisions(unit_id, current_item_ids);
        if colliding_departed_ids.is_empty() {
            return true;
        }

        match self
            .loot_hook
            .clear_unit_id_bits(&self.state.ctx, &[unit_id])
        {
            Ok(()) => {
                self.hook_bits.confirm_cleared(&colliding_departed_ids);
                true
            }
            Err(e) => {
                log_error(&format!(
                    "Failed to reset stale hook bits for fresh colliding item {}: {}",
                    unit_id, e
                ));
                false
            }
        }
    }

    pub(in crate::notifier) fn retry_pending_visibility_ops(&mut self, unit_id: u32) {
        if !self.loot_hook.is_injected() {
            return;
        }

        let ops = self.pending_visibility_ops.take(unit_id);
        if ops.is_empty() {
            return;
        }

        let failed_ops = self.apply_visibility_mask_ops(unit_id, &ops);
        self.pending_visibility_ops
            .record_failed(unit_id, failed_ops);
        self.hook_bits.mark_written(unit_id);
    }

    pub(in crate::notifier) fn apply_visibility_mask_ops(
        &self,
        unit_id: u32,
        ops: &[VisibilityMaskOp],
    ) -> Vec<VisibilityMaskOp> {
        let mut failed_ops = Vec::new();
        for &op in ops {
            if let Err(e) = self.apply_visibility_mask_op(unit_id, op) {
                log_error(&format!(
                    "Failed to {} item {}: {}",
                    visibility_mask_op_description(op),
                    unit_id,
                    e
                ));
                failed_ops.push(op);
            }
        }
        failed_ops
    }

    fn apply_visibility_mask_op(&self, unit_id: u32, op: VisibilityMaskOp) -> Result<(), String> {
        match op {
            VisibilityMaskOp::SetShow => self.loot_hook.add_shown_unit_id(&self.state.ctx, unit_id),
            VisibilityMaskOp::SetHide => {
                self.loot_hook.add_hidden_unit_id(&self.state.ctx, unit_id)
            }
            VisibilityMaskOp::ClearShow => {
                self.loot_hook.clear_shown_unit_id(&self.state.ctx, unit_id)
            }
            VisibilityMaskOp::ClearHide => self
                .loot_hook
                .clear_hidden_unit_id(&self.state.ctx, unit_id),
        }
    }
}
