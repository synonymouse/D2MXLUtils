//! Local inventory acquisition for pending loot-history pickup resolution.

use super::DropScanner;
use crate::offsets::{d2client, inventory, item_data, unit};
use std::collections::HashSet;

impl DropScanner {
    /// Walk the local player's inventory and return every item `unit_id`
    /// linked off `pFirstItem`. Robust against stale `p_unit_data` caches:
    /// the walk uses live pointers from the player struct outward.
    ///
    /// Chain: `PLAYER_UNIT` → `UnitAny + 0x60 (Inventory*)` →
    ///        `Inventory + 0x0C (pFirstItem)` → walk via item
    ///        `pUnitData + 0x64 (NEXT_ITEM)`.
    ///
    /// Capped at 256 iterations to defend against pointer cycles.
    pub(super) fn read_player_inventory_ids(&self) -> HashSet<u32> {
        let mut ids = HashSet::new();

        let player_unit_ptr_addr = self.state.ctx.d2_client + d2client::PLAYER_UNIT;
        let player_ptr = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(player_unit_ptr_addr)
        {
            Ok(p) if p != 0 => p as usize,
            _ => return ids,
        };

        let inv_ptr = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(player_ptr + unit::INVENTORY)
        {
            Ok(p) if p != 0 => p as usize,
            _ => return ids,
        };

        let mut p_item = match self
            .state
            .ctx
            .process
            .read_memory::<u32>(inv_ptr + inventory::FIRST_ITEM)
        {
            Ok(p) => p,
            Err(_) => return ids,
        };

        for _ in 0..256 {
            if p_item == 0 {
                break;
            }
            // UnitAny.unit_id at +0x0C
            if let Ok(uid) = self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_item as usize + unit::UNIT_ID)
            {
                ids.insert(uid);
            }
            // UnitAny.pUnitData at +0x14 → ItemData; ItemData + 0x64 = next.
            let p_unit_data = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_item as usize + unit::UNIT_DATA)
            {
                Ok(p) if p != 0 => p as usize,
                _ => break,
            };
            p_item = match self
                .state
                .ctx
                .process
                .read_memory::<u32>(p_unit_data + item_data::NEXT_ITEM)
            {
                Ok(p) => p,
                Err(_) => break,
            };
        }

        ids
    }
}
