//! Live weapon/elemental damage readout — the "damage display" feature
//! ported from D2Stats.au3's `CalculateWeaponDamage()` / Weapon Damage
//! panel (`D2Stats.au3:684-732` and `:2708-2715`).
//!
//! Stat ids below are vanilla D2 `ItemStatCost.txt` row indices. MXL keeps
//! the vanilla core stat numbering stable (save/network compatibility), and
//! D2Stats.au3 — a tool built specifically for MXL — hardcodes these same
//! ids, so we reuse them directly instead of re-deriving from a stat-name
//! table. Confirmed by `offsets::d2common::ITEMS_TXT = 0x9FB98`, which is
//! byte-identical to D2Stats.au3's `$g_hD2Common + 0x9FB98`.
//!
//! Unlike D2Stats (which manually sums the unit's raw `StatList`), we call
//! the game's own `D2Common::GetUnitStat` via the existing code-injection
//! path (`D2Injector::get_unit_stat`, already used by `breakpoints.rs`).
//! That returns the same aggregated-across-layers value D2Stats computes
//! by hand, so the downstream formula is unchanged.

use serde::Serialize;

use crate::breakpoints::read_equipped_weapon;
use crate::injection::D2Injector;
use crate::offsets::{d2common, items_txt};
use crate::process::D2Context;

#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DamageStats {
    /// One-hand physical damage range (0/0 when the equipped weapon isn't 1H).
    pub phys_min_1h: i32,
    pub phys_max_1h: i32,
    /// Two-hand/ranged physical damage range (0/0 when not applicable).
    /// For a 1H-only weapon this instead holds the thrown-weapon damage
    /// (stat 159/160), matching D2Stats's "Two-hand/Ranged" column.
    pub phys_min_2h: i32,
    pub phys_max_2h: i32,
    pub fire_min: i32,
    pub fire_max: i32,
    pub cold_min: i32,
    pub cold_max: i32,
    pub lightning_min: i32,
    pub lightning_max: i32,
    pub magic_min: i32,
    pub magic_max: i32,
    /// Poison damage per second (D2Stats.au3:642-643 scales the raw stat by
    /// 25/256 to convert the engine's internal accumulation into dmg/s).
    pub poison_min_per_sec: i32,
    pub poison_max_per_sec: i32,
    /// Individual Strength/Dexterity contribution to physical weapon damage
    /// %, shown as separate informational rows. Computed the same way as
    /// the combined `stat_bonus` term below but without the shared `-1`
    /// adjustment, so the two may not sum exactly to the multiplier actually
    /// applied to `phys_*` — display-only, not used in the damage formula.
    pub str_damage_bonus_pct: i32,
    pub dex_damage_bonus_pct: i32,
}

// --- ItemStatCost.txt stat ids (vanilla D2 numbering) ---
const STAT_STRENGTH: u32 = 0;
const STAT_DEXTERITY: u32 = 2;
const STAT_MIN_DAMAGE_1H: u32 = 21;
const STAT_MAX_DAMAGE_1H: u32 = 22;
const STAT_MIN_DAMAGE_2H: u32 = 23;
const STAT_MAX_DAMAGE_2H: u32 = 24;
/// `damagepercent` — Enhanced Weapon Damage from items (`{025}%` in D2Stats).
const STAT_DAMAGE_PERCENT: u32 = 25;
/// Itemtype-specific EWD (Elfin Weapons, Shadow Dancer, ...).
const STAT_ITEMTYPE_EWD: u32 = 343;
const STAT_MIN_DAMAGE_THROW: u32 = 159;
const STAT_MAX_DAMAGE_THROW: u32 = 160;
const STAT_FIRE_MIN: u32 = 48;
const STAT_FIRE_MAX: u32 = 49;
const STAT_LIGHTNING_MIN: u32 = 50;
const STAT_LIGHTNING_MAX: u32 = 51;
const STAT_MAGIC_MIN: u32 = 52;
const STAT_MAGIC_MAX: u32 = 53;
const STAT_COLD_MIN: u32 = 54;
const STAT_COLD_MAX: u32 = 55;
const STAT_POISON_MIN: u32 = 57;
const STAT_POISON_MAX: u32 = 58;

/// Mirrors `D2Stats.au3::UpdateStatValues` + `CalculateWeaponDamage` for a
/// single unit (player or mercenary). Returns `None` when the unit pointer
/// is null or has no weapon in the right-hand slot (bare-handed) — same
/// early-out as `CalculateWeaponDamage`'s `if (not $pWeapon) then return`.
pub fn read_unit_damage_stats(
    ctx: &D2Context,
    injector: &D2Injector,
    unit_ptr_offset: usize,
) -> Option<DamageStats> {
    let unit_ptr_addr = ctx.d2_client + unit_ptr_offset;
    let p_unit = match ctx.process.read_memory::<u32>(unit_ptr_addr) {
        Ok(p) if p != 0 => p,
        _ => return None,
    };

    let (_, _, _, file_index) = read_equipped_weapon(ctx, p_unit);
    if file_index == 0 {
        return None;
    }

    let (str_bonus, dex_bonus, is_1h, is_2h) = read_weapon_damage_fields(ctx, file_index as usize);

    let stat = |id: u32| -> i32 {
        injector
            .get_unit_stat(&ctx.process, p_unit, id)
            .unwrap_or(0) as i32
    };

    let str_total = stat(STAT_STRENGTH);
    let dex_total = stat(STAT_DEXTERITY);

    let min_dmg_1h = if is_1h { stat(STAT_MIN_DAMAGE_1H) } else { 0 };
    let mut max_dmg_1h = if is_1h { stat(STAT_MAX_DAMAGE_1H) } else { 0 };
    let (min_dmg_2h, mut max_dmg_2h) = if is_2h {
        (stat(STAT_MIN_DAMAGE_2H), stat(STAT_MAX_DAMAGE_2H))
    } else if is_1h {
        // Thrown weapon: 1H but not 2H uses the throw-damage stat pair.
        (stat(STAT_MIN_DAMAGE_THROW), stat(STAT_MAX_DAMAGE_THROW))
    } else {
        (0, 0)
    };

    if max_dmg_1h < min_dmg_1h {
        max_dmg_1h = min_dmg_1h + 1;
    }
    if max_dmg_2h < min_dmg_2h {
        max_dmg_2h = min_dmg_2h + 1;
    }

    let ewd = stat(STAT_DAMAGE_PERCENT) + stat(STAT_ITEMTYPE_EWD);
    let str_pct = (str_total as i64 * str_bonus as i64) as f64 / 100.0;
    let dex_pct = (dex_total as i64 * dex_bonus as i64) as f64 / 100.0;
    let stat_bonus = (str_pct + dex_pct).floor() as i64 - 1;
    let total_mult = 1.0 + (ewd as f64) / 100.0 + (stat_bonus as f64) / 100.0;

    let scale = |v: i32| -> i32 { ((v as f64) * total_mult).floor() as i32 };

    Some(DamageStats {
        phys_min_1h: scale(min_dmg_1h),
        phys_max_1h: scale(max_dmg_1h),
        phys_min_2h: scale(min_dmg_2h),
        phys_max_2h: scale(max_dmg_2h),
        fire_min: stat(STAT_FIRE_MIN),
        fire_max: stat(STAT_FIRE_MAX),
        cold_min: stat(STAT_COLD_MIN),
        cold_max: stat(STAT_COLD_MAX),
        lightning_min: stat(STAT_LIGHTNING_MIN),
        lightning_max: stat(STAT_LIGHTNING_MAX),
        magic_min: stat(STAT_MAGIC_MIN),
        magic_max: stat(STAT_MAGIC_MAX),
        poison_min_per_sec: scale_poison(stat(STAT_POISON_MIN)),
        poison_max_per_sec: scale_poison(stat(STAT_POISON_MAX)),
        str_damage_bonus_pct: str_pct.floor() as i32,
        dex_damage_bonus_pct: dex_pct.floor() as i32,
    })
}

/// D2Stats.au3:642-643 — `$g_aiStatsCache[1][57] *= (25/256)`.
fn scale_poison(raw: i32) -> i32 {
    ((raw as f64) * (25.0 / 256.0)).floor() as i32
}

/// Reads `str_bonus`/`dex_bonus`/`is_1h`/`is_2h` for the weapon at
/// `Items.txt[file_index]`. Sibling of
/// `breakpoints::read_weapon_fields_from_items_txt`, pulling the fields
/// `CalculateWeaponDamage` needs instead of `wclass`/`wsm`/family chain.
fn read_weapon_damage_fields(ctx: &D2Context, file_index: usize) -> (i32, i32, bool, bool) {
    let base_ptr = match ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::ITEMS_TXT)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return (0, 0, false, false),
    };

    let record = base_ptr + file_index * items_txt::RECORD_SIZE;

    let str_bonus = ctx
        .process
        .read_memory::<u16>(record + items_txt::STR_BONUS)
        .unwrap_or(0) as i32;
    let dex_bonus = ctx
        .process
        .read_memory::<u16>(record + items_txt::DEX_BONUS)
        .unwrap_or(0) as i32;
    let is_2h = ctx
        .process
        .read_memory::<u8>(record + items_txt::IS_2H)
        .unwrap_or(0)
        != 0;
    // D2Stats.au3:699 — `$bIs1H = $bIs2H ? read(+0x13D) : 1`: a weapon that
    // isn't 2H is always usable 1H, so only consult the flag when 2H.
    let is_1h = if is_2h {
        ctx.process
            .read_memory::<u8>(record + items_txt::IS_1H)
            .unwrap_or(0)
            != 0
    } else {
        true
    };

    (str_bonus, dex_bonus, is_1h, is_2h)
}
