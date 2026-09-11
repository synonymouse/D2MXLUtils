use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::AppState;

use crate::injection::D2Injector;
use crate::logger::info as log_info;
use crate::offsets::unit;
use crate::process::D2Context;
use crate::stat_telemetry::StatConsumer;
use crate::unit_stats_reader::fallback::StatReadContext;

mod speedcalc_data;
mod weapon;
mod weapon_families;

pub(crate) use speedcalc_data::{
    fetch_and_cache as fetch_and_cache_speedcalc_data, load_from_cache as load_speedcalc_cache,
    SpeedcalcTable,
};
pub(crate) use weapon::{read_equipped_weapon, resolve_item_type_chain, u32_to_packed_code};
pub(crate) use weapon_families::{
    build_catalog as build_weapon_base_catalog, load_from_cache as load_weapon_base_cache,
    save_to_cache as save_weapon_base_cache, WeaponBaseCatalog,
};

#[cfg(all(test, target_os = "windows"))]
mod stat_acquisition_tests;

#[tauri::command]
pub(crate) fn set_breakpoints_polling(enabled: bool, state: tauri::State<AppState>) {
    state.breakpoints_polling.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
pub(crate) fn get_speedcalc_data(state: tauri::State<AppState>) -> Option<SpeedcalcTable> {
    state
        .speedcalc_table
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

#[tauri::command]
pub(crate) fn refresh_speedcalc_data(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let table = fetch_and_cache_speedcalc_data(&app_data_dir)?;
    if let Ok(mut guard) = state.speedcalc_table.write() {
        *guard = Some(table);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn get_weapon_base_catalog(state: tauri::State<AppState>) -> Option<WeaponBaseCatalog> {
    state
        .weapon_base_catalog
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

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

/// `Ok(None)`: no unit at this offset (not in game / no mercenary hired) —
/// a legitimate absence (an unreadable unit pointer also returns `Ok(None)`).
/// Stats use validated direct reads first; missing optional stats become zero.
/// `Err(())` means a direct read and its one legacy fallback both failed.
/// On Linux, only that fallback uses `get_unit_stat`'s ptrace-hijacked remote
/// call (`call_remote`), which can fail to find a thread within its retry
/// budget under concurrent load. Historically, treating these injected-read
/// failures as zero made the displayed stats flash. Callers retain the last
/// `Ok(Some(_))` across an `Err(())` tick to preserve that last-good behavior.
pub fn read_unit_breakpoint_data(
    ctx: &D2Context,
    injector: &D2Injector,
    unit_ptr_offset: usize,
) -> Result<Option<BreakpointData>, ()> {
    let unit_ptr_addr = ctx.d2_client + unit_ptr_offset;
    let p_unit = match ctx.process.read_memory::<u32>(unit_ptr_addr) {
        Ok(p) if p != 0 => p,
        _ => return Ok(None),
    };

    let class = ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::CLASS)
        .unwrap_or(0);
    let unit_type = ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::UNIT_TYPE)
        .unwrap_or(u32::MAX);

    let ias = read_stat(ctx, injector, p_unit, STAT_IAS)?;
    let fcr = read_stat(ctx, injector, p_unit, STAT_FCR)?;
    let fhr = read_stat(ctx, injector, p_unit, STAT_FHR)?;
    let fbr = read_stat(ctx, injector, p_unit, STAT_FBR)?;
    let skill_ias = read_stat(ctx, injector, p_unit, STAT_SKILL_IAS)?;
    let skill_fhr = read_stat(ctx, injector, p_unit, STAT_SKILL_FHR)?;

    let (wclass, wsm, family_codes, file_index) = read_equipped_weapon(ctx, p_unit);

    let merc_type = if unit_type == 1 {
        let detected = classify_merc(class);
        if detected.is_none() && LAST_UNKNOWN_MERC_CLASS.swap(class, Ordering::Relaxed) != class {
            log_info(&format!(
                "breakpoints: unknown merc class id {} — please report",
                class
            ));
        }
        detected
    } else {
        None
    };

    Ok(Some(BreakpointData {
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
    }))
}

fn read_stat(ctx: &D2Context, injector: &D2Injector, p_unit: u32, stat_id: u32) -> Result<i32, ()> {
    StatReadContext::new(ctx, injector, StatConsumer::Breakpoints).read_stat(p_unit, stat_id)
}
