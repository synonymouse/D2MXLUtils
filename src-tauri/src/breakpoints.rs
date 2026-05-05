use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

use crate::offsets::{
    body_loc, d2common, data_tables, inventory, inventory_grid, item_types_txt, items_txt, unit,
};
use crate::process::D2Context;
use crate::injection::D2Injector;
use crate::logger::info as log_info;

#[derive(Debug, Clone, Serialize, Default)]
pub struct BreakpointData {
    pub class: u32,
    pub wclass: String,
    pub wsm: i32,
    pub file_index: u32,
    /// Chain of 4-char ItemTypes codes for the equipped weapon, most
    /// specific first (`["qaxe", "axe", "mele", "weap"]`), so the frontend
    /// can roll MXL sub-types up to a known base family.
    pub family_codes: Vec<String>,
    pub ias: i32,
    pub fcr: i32,
    pub fhr: i32,
    pub fbr: i32,
    pub skill_ias: i32,
    pub skill_fhr: i32,
    pub merc_type: Option<u32>,
}

/// MXL Σ monstats ids verified via `docs/ce-scripts/verify-merc-class.lua`.
/// Shapeshifter (MERCS[2]) is manual-only — we don't yet know its distinct id.
fn classify_merc(class: u32) -> Option<u32> {
    match class {
        271 => Some(0),
        338 => Some(1),
        359 => Some(3),
        561 => Some(4),
        _ => None,
    }
}

static LAST_UNKNOWN_MERC_CLASS: AtomicU32 = AtomicU32::new(u32::MAX);

const STAT_IAS: u32 = 93;
const STAT_FCR: u32 = 105;
const STAT_FHR: u32 = 99;
const STAT_FBR: u32 = 102;
const STAT_SKILL_IAS: u32 = 68;
const STAT_SKILL_FHR: u32 = 69;

pub fn read_unit_breakpoint_data(
    ctx: &D2Context,
    injector: &D2Injector,
    unit_ptr_offset: usize,
) -> Option<BreakpointData> {
    let unit_ptr_addr = ctx.d2_client + unit_ptr_offset;
    let p_unit = match ctx.process.read_memory::<u32>(unit_ptr_addr) {
        Ok(p) if p != 0 => p,
        _ => return None,
    };

    let class = ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::CLASS)
        .unwrap_or(0);
    let unit_type = ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::UNIT_TYPE)
        .unwrap_or(u32::MAX);

    let ias = read_stat(ctx, injector, p_unit, STAT_IAS);
    let fcr = read_stat(ctx, injector, p_unit, STAT_FCR);
    let fhr = read_stat(ctx, injector, p_unit, STAT_FHR);
    let fbr = read_stat(ctx, injector, p_unit, STAT_FBR);
    let skill_ias = read_stat(ctx, injector, p_unit, STAT_SKILL_IAS);
    let skill_fhr = read_stat(ctx, injector, p_unit, STAT_SKILL_FHR);

    let (wclass, wsm, family_codes, file_index) = read_equipped_weapon(ctx, p_unit);

    let merc_type = if unit_type == 1 {
        let detected = classify_merc(class);
        if detected.is_none()
            && LAST_UNKNOWN_MERC_CLASS.swap(class, Ordering::Relaxed) != class
        {
            log_info(&format!(
                "breakpoints: unknown merc class id {} — please report",
                class
            ));
        }
        detected
    } else {
        None
    };

    Some(BreakpointData {
        class,
        wclass,
        wsm,
        file_index,
        family_codes,
        ias,
        fcr,
        fhr,
        fbr,
        skill_ias,
        skill_fhr,
        merc_type,
    })
}

fn read_stat(ctx: &D2Context, injector: &D2Injector, p_unit: u32, stat_id: u32) -> i32 {
    match injector.get_unit_stat(&ctx.process, p_unit, stat_id) {
        Ok(v) => v as i32,
        Err(_) => 0,
    }
}

/// Reads the currently equipped right-hand weapon via the D2MOO inventory
/// grid path: `pInventory->pGrids[INVGRID_BODYLOC=0]->ppItems[BODYLOC_RARM=4]`.
/// The engine physically moves the active weapon into BODYLOC_RARM on weapon
/// switch, so we never need to read BODYLOC_SWRARM. Returns `(wclass, wsm,
/// family_codes, file_index)` or empty/zero values when nothing is equipped.
fn read_equipped_weapon(ctx: &D2Context, p_unit: u32) -> (String, i32, Vec<String>, u32) {
    let empty = (String::new(), 0, Vec::new(), 0u32);

    let p_inventory = match ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::INVENTORY)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return empty,
    };

    let p_grids = match ctx.process.read_memory::<u32>(p_inventory + inventory::GRIDS) {
        Ok(p) if p != 0 => p as usize,
        _ => return empty,
    };

    let pp_items = match ctx
        .process
        .read_memory::<u32>(p_grids + inventory_grid::PP_ITEMS)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return empty,
    };

    let p_weapon = match ctx
        .process
        .read_memory::<u32>(pp_items + body_loc::RARM * 4)
    {
        Ok(p) if p != 0 => p,
        _ => return empty,
    };

    let file_index = match ctx
        .process
        .read_memory::<u32>(p_weapon as usize + unit::CLASS)
    {
        Ok(idx) => idx,
        Err(_) => return empty,
    };

    let (wclass, wsm, family_codes) = read_weapon_fields_from_items_txt(ctx, file_index as usize);
    (wclass, wsm, family_codes, file_index)
}

fn read_weapon_fields_from_items_txt(ctx: &D2Context, file_index: usize) -> (String, i32, Vec<String>) {
    let base_ptr = match ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::ITEMS_TXT)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return (String::new(), 0, Vec::new()),
    };

    let record = base_ptr + file_index * items_txt::RECORD_SIZE;

    let wclass_raw = ctx
        .process
        .read_memory::<u32>(record + items_txt::WCLASS)
        .unwrap_or(0);
    let wclass = u32_to_packed_code(wclass_raw);

    let wsm = ctx
        .process
        .read_memory::<i32>(record + items_txt::SPEED)
        .unwrap_or(0);

    let type0 = ctx
        .process
        .read_memory::<u16>(record + items_txt::TYPE_0)
        .unwrap_or(0);

    let family_codes = resolve_item_type_chain(ctx, type0);

    (wclass, wsm, family_codes)
}

/// Walks an `items.txt::TYPE_0` index up through ItemTypes.txt's `equiv1`
/// chain and returns each ancestor's 4-char `szCode`, most specific first
/// (e.g. `["qaxe", "axe", "mele", "weap"]` for a Sacred War Axe). The
/// frontend matches this list against its known weapon-family tokens so
/// MXL sub-types automatically roll up to their base family. Walks at most
/// 6 hops; cycle-safe; returns empty on any read failure.
pub(crate) fn resolve_item_type_chain(ctx: &D2Context, type_idx: u16) -> Vec<String> {
    let mut chain = Vec::new();

    if type_idx == 0 {
        return chain;
    }

    let p_data_tables = match ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::SGPT_DATA_TABLES)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return chain,
    };

    let p_item_types = match ctx
        .process
        .read_memory::<u32>(p_data_tables + data_tables::ITEM_TYPES_TXT_PTR)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return chain,
    };

    let count = ctx
        .process
        .read_memory::<i32>(p_data_tables + data_tables::ITEM_TYPES_TXT_COUNT)
        .unwrap_or(0);
    if count <= 0 {
        return chain;
    }

    let mut cur: i32 = type_idx as i32;
    let mut visited = std::collections::HashSet::new();

    for _ in 0..6 {
        if cur <= 0 || cur >= count || !visited.insert(cur) {
            break;
        }
        let record = p_item_types + (cur as usize) * item_types_txt::RECORD_SIZE;
        let code_raw = ctx
            .process
            .read_memory::<u32>(record + item_types_txt::CODE)
            .unwrap_or(0);
        let code = u32_to_packed_code(code_raw).to_lowercase();
        if code.is_empty() {
            break;
        }
        chain.push(code);
        let next = ctx
            .process
            .read_memory::<i16>(record + item_types_txt::EQUIV1)
            .unwrap_or(0);
        cur = next as i32;
    }

    chain
}

/// Decodes a 4-byte packed ASCII code (`u32` little-endian) into a String.
/// MXL pads unused bytes with either NUL or ASCII space, so we treat both
/// as the terminator. Returned uppercase by default; callers that need
/// lowercase (e.g. ItemTypes codes) should `.to_lowercase()`.
pub(crate) fn u32_to_packed_code(raw: u32) -> String {
    let bytes = raw.to_le_bytes();
    let len = bytes
        .iter()
        .position(|&b| b == 0 || b == b' ')
        .unwrap_or(4);
    String::from_utf8_lossy(&bytes[..len]).to_uppercase()
}
