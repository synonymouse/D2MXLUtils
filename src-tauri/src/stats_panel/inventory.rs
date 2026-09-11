use std::collections::HashSet;

use crate::breakpoints::resolve_item_type_chain;
use crate::offsets::{d2common, inventory, item_data, items_txt, unit};
use crate::process::D2Context;

/// Counts the unit's currently-carried charm-type items (2pt each, no
/// per-item exceptions modeled yet — see `super::STAT_CHARMS`) by walking its
/// actual inventory, instead of trusting the engine's own internal
/// `GetUnitStat(356)` value.
///
/// Each item's true `items.txt` row is `unit::CLASS` (read directly off the
/// item's own `UnitAny`) — *not* `item_data::FILE_INDEX`, which for
/// specially-named items (charms, quest items, uniques) turned out to hold
/// an unrelated base-appearance/graphic index instead of the real type.
/// That distinction was confirmed by cross-checking against the game's own
/// `D2Injector::get_item_name` resolution (the same call the shipped item
/// search/hover feature uses) for a known real charm — `unit::CLASS`
/// matched its true row; `item_data::FILE_INDEX` pointed at an unrelated
/// weapon base.
///
/// Returns `None` when the unit has no inventory (unit pointer stale, or a
/// merc that hasn't fully attached this tick) so the caller can fall back
/// to the last successfully-computed count instead of flashing to 0.
pub(super) fn count_charm_points(ctx: &D2Context, p_unit: u32) -> Option<u32> {
    let inv_ptr = match ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::INVENTORY)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return None,
    };
    let items_base = match ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::ITEMS_TXT)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return None,
    };
    let items_count = ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::ITEMS_TXT_COUNT)
        .unwrap_or(0);

    let mut p_item = ctx
        .process
        .read_memory::<u32>(inv_ptr + inventory::FIRST_ITEM)
        .unwrap_or(0);

    let mut visited = HashSet::new();
    let mut points = 0u32;
    // Same generous cap as the equivalent debug walk — real inventories
    // (inventory + cube + stash combined, per `item_data::GAME_LOCATION`)
    // stay well under this.
    for _ in 0..512 {
        if p_item == 0 || !visited.insert(p_item) {
            break;
        }
        let p_unit_data = match ctx
            .process
            .read_memory::<u32>(p_item as usize + unit::UNIT_DATA)
        {
            Ok(p) if p != 0 => p as usize,
            _ => break,
        };

        let file_index = ctx
            .process
            .read_memory::<u32>(p_item as usize + unit::CLASS)
            .unwrap_or(0);
        if file_index > 0 && file_index < items_count {
            let record = items_base + file_index as usize * items_txt::RECORD_SIZE;
            let type0 = ctx
                .process
                .read_memory::<u16>(record + items_txt::TYPE_0)
                .unwrap_or(0);
            if resolve_item_type_chain(ctx, type0)
                .iter()
                .any(|code| code == "char")
            {
                points += 2;
            }
        }

        p_item = ctx
            .process
            .read_memory::<u32>(p_unit_data + item_data::NEXT_ITEM)
            .unwrap_or(0);
    }

    Some(points)
}
