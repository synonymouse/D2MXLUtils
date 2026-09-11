//! Full character stat sheet — ports the "Basic"/"Page 1"/"Page 2" stat
//! panels from D2Stats.au3's `CreateGUI()` (`D2Stats.au3:2510-2636`).
//!
//! D2Stats reads these by manually summing the unit's raw `StatList`
//! (`UpdateStatValueMem` / `FixStats`), which requires several bug-workarounds
//! (zeroing velocity stats, halving life regen, etc — see `FixStats`). We
//! prefer a validated bulk read through `StatReadContext`, using the shared
//! reader's descriptor selection and ItemStatCost adjustments rather than
//! those manual-summation workarounds. If that read fails or level is missing
//! or nonpositive, we retain one legacy `D2Common::GetUnitStat` sweep via
//! `D2Injector::get_unit_stat`. Scaling and derived formulas remain unchanged;
//! direct/injected parity has not been verified against a live game.
//!
//! Stat ids are vanilla D2 `ItemStatCost.txt` row indices (MXL keeps the
//! vanilla core numbering stable). The frontend owns labels/grouping/tooltips
//! (mirroring D2Stats's `{NNN}` template strings) — this module only returns
//! flat `stat id -> value` maps plus the ids needed to compute derived rows.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use crate::AppState;

use crate::injection::D2Injector;
use crate::offsets::unit;
use crate::process::D2Context;
use crate::stat_telemetry::StatConsumer;
use crate::unit_stats_reader::fallback::StatReadContext;

mod base_stats;
mod inventory;

use base_stats::read_base_stats;
use inventory::count_charm_points;

#[cfg(all(test, target_os = "windows"))]
mod stat_acquisition_tests;

#[tauri::command]
pub(crate) fn set_stats_polling(enabled: bool, state: tauri::State<AppState>) {
    state.stats_polling.store(enabled, Ordering::SeqCst);
}

/// Every stat id shown anywhere in the Stats tab (character data, attributes,
/// speed, combat, resistances, misc, minions, life/mana-on-hit, absorb, and
/// the inputs to the derived Spell Focus cap below). Order doesn't matter.
const STAT_IDS: &[u32] = &[
    // Character data / misc (Basic tab)
    12, 13, 14, 15, 185, 356, 80, 79, 85, 479,
    // Base attributes (aggregate/full value — see `base_stats` for the
    // unmerged, no-item value used to compute the flat/percent breakdown)
    0, 2, 3, 1, // Current/Max Life & Mana (fixed-point — see `is_life_mana_stat`)
    6, 7, 8, 9, // Bonus attribute percent (item bonus str/dex/vit/ene %)
    359, 360, 362, 361, // Speed
    93, 68, 99, 69, 102, 96, 67, 105, // Combat
    76, 77, 25, 171, 119, 19, 34, 35, 184, 338, 339, 340, 136, 141, 344,
    // Resistances + damage/pierce + max-resist bonuses
    39, 43, 41, 45, 37, 36, 40, 44, 42, 46, 38, 329, 333, 331, 335, 330, 334, 332, 336, 431, 357,
    // Misc (Page 2). 485 = flat Spell Focus, 488 = "+#% Spell Focus (from
    // items/runes)" (`d2StatDescriptions.au3:493`) — boosts the flat SF
    // multiplicatively, same convention as the attribute %-bonus stats.
    485, 488, 409, 74, 27, 109, 110, 489, 121, 122, 150, 376, 363, 493, // Minions
    444, 470, 487, 500, // Life/Mana on hit
    60, 62, 86, 138, 208, 209, 210, 295, // Absorb
    142, 143, 148, 149, 144, 145, 146, 147, // Boolean flags
    108, 118, 153,
    // Innate Elemental Damage bonus % (MXL-custom `ItemStatCost.txt` stat
    // "IED" — multiplies a weapon base's built-in elemental-from-attribute
    // conversion %, e.g. elemental bows/claws). Verified against Median
    // XL's own data files (MedianXLOfflineTools), not vanilla D2.
    484,
];

/// Base-attribute stat ids read from the unit's own (unmerged, no-item)
/// StatList — used by the frontend to split each aggregate stat into its
/// flat/percent item-and-skill bonus (mirrors D2Stats.au3's `vector 0`
/// reads in `UpdateStatValueMem`, which only uses this "vector 0" trick for
/// the 4 base attributes, ids < 4 — Life/Mana are derived stats where this
/// unmerged value doesn't reliably mean "no items", so we don't fetch them
/// here; see the frontend's `lifeManaBreakdown` for why).
const BASE_STAT_IDS: &[u32] = &[0, 2, 3, 1];

/// Derived stat id (not a real `ItemStatCost.txt` row) holding effective
/// Spell Focus (flat SF boosted by the item/rune % bonus) — *not yet*
/// divided by 10. The frontend computes `min(effectiveSF / 10, 100)` with
/// one decimal place, per https://docs.median-xl.com/doc/concepts/spellfocus
/// ("for every 10 Spell Focus, +1% spell damage, capped at 100%"). Kept as
/// the raw integer here so the frontend doesn't lose the fractional %
/// (e.g. 105 SF is a real 10.5%, not 10%).
const STAT_SF_CAP: u32 = 904;
/// Derived stat id for the separate Energy-derived spell-damage bonus %
/// (`130*(energy+20)/500 + energy`), the other half of the docs' combined
/// formula: `130*(energy+20)/500 + energy + min(spell_focus/10, 100)`.
const STAT_SF_ENERGY_BONUS: u32 = 907;
/// Derived stat ids for the experience-to-next-level breakdown: cumulative
/// exp required to reach the character's current level, and to reach the
/// next level (or `-1` when the level is beyond the known table).
const STAT_EXP_LEVEL_START: u32 = 905;
const STAT_EXP_LEVEL_NEXT: u32 = 906;
/// "Charms" — see `count_charm_points` for why this is computed locally
/// instead of trusted from the engine's own `GetUnitStat`.
const STAT_CHARMS: u32 = 356;

#[derive(Clone)]
pub struct CharacterStats {
    /// Unit class id (0=Amazon..6=Assassin) — lets the frontend look up
    /// class-specific constants (e.g. life-per-vitality).
    pub class: u32,
    /// Aggregate (fully merged, includes item/skill bonuses) stat values,
    /// acquired direct-first with legacy `GetUnitStat` fallback, then scaled
    /// and supplemented by the local overrides and derived rows below.
    pub stats: BTreeMap<u32, i32>,
    /// Unmerged (no item/skill bonuses) values for `BASE_STAT_IDS`, read
    /// directly from the unit's own StatList.
    pub base_stats: BTreeMap<u32, i32>,
}

/// Reads the full character-stats sheet for one unit.
///
/// Returns `None` when the unit pointer is null or cannot be read. A validated
/// direct bulk result supplies the raw values, with missing optional stats
/// treated as zero. A reader error or missing/nonpositive level triggers
/// exactly one legacy per-ID sweep, without retrying direct reads per ID.
/// During that fallback sweep, a failed `GetUnitStat` uses the already-scaled
/// per-ID value from `previous` (tracked in `app/scanner_runtime/worker.rs`), or zero if unavailable.
/// Historically, aborting the entire injected sweep on any failure made the
/// sheet stall under flaky injection: a fully successful ~100-call sweep
/// could be rare. Retaining per-ID last-good values preserves that recovery
/// behavior when direct acquisition is unavailable.
pub fn read_unit_character_stats(
    ctx: &D2Context,
    injector: &D2Injector,
    unit_ptr_offset: usize,
    previous: Option<&CharacterStats>,
) -> Option<CharacterStats> {
    let unit_ptr_addr = ctx.d2_client + unit_ptr_offset;
    let p_unit = match ctx.process.read_memory::<u32>(unit_ptr_addr) {
        Ok(p) if p != 0 => p,
        _ => return None,
    };

    let class = ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::CLASS)
        .unwrap_or(0);

    let acquisition = StatReadContext::new(ctx, injector, StatConsumer::Stats);
    let direct = acquisition.read_bulk(p_unit, STAT_IDS);
    let mut stats = BTreeMap::new();
    for &id in STAT_IDS {
        let raw = match &direct {
            Ok(values) => Ok(values.get(&id).copied().unwrap_or(0)),
            Err(()) => acquisition.legacy_stat(p_unit, id),
        };
        let value = match raw {
            Ok(raw) => scale_life_mana(id, raw),
            Err(_) => previous
                .and_then(|p| p.stats.get(&id))
                .copied()
                .unwrap_or(0),
        };
        stats.insert(id, value);
    }

    // Override the engine's own GetUnitStat(356) "Charms" value with a count
    // we compute ourselves by walking the unit's actual inventory — verified
    // via raw memory inspection to badly under-report (a character with 48
    // confirmed real charm-type items showed 29 via GetUnitStat, when the
    // documented (Charms+Relics)*2 formula implies ~90+). Root cause
    // unconfirmed (stale engine-side cache? outdated formula docs?), but the
    // live inventory count is independently verifiable and trustworthy.
    // Doesn't yet model the documented per-item exceptions (Ennead=1pt,
    // Sunstone of the Twin Seas=1-4pt by scrolls used, Riftwalker=2-6pt by
    // upgrades, Sleep=0pt unless fully Awakened, Tome of Possession=+2pt
    // despite not being a charm) — every matched item counts flat 2pt.
    if let Some(points) = count_charm_points(ctx, p_unit) {
        stats.insert(STAT_CHARMS, points as i32);
    } else if let Some(prev_points) = previous.and_then(|p| p.stats.get(&STAT_CHARMS)) {
        stats.insert(STAT_CHARMS, *prev_points);
    }

    let energy_full = *stats.get(&1).unwrap_or(&0) as f64;
    let flat_sf = *stats.get(&485).unwrap_or(&0) as f64;
    let pct_sf = *stats.get(&488).unwrap_or(&0) as f64;
    // Effective SF = flat Spell Focus boosted by the item/rune % bonus,
    // same multiplicative convention as the attribute %-bonus stats (not
    // independently confirmed by the docs formula below, but consistent
    // with how every other flat/% stat pair in the game stacks).
    let effective_sf = (flat_sf * (1.0 + pct_sf / 100.0)).floor() as i32;
    // Current formula (docs.median-xl.com/doc/concepts/spellfocus): total
    // spell-damage bonus = 130*(energy+20)/500 + energy + min(SF/10, 100).
    // SF is *not* weighted by Strength/Energy — that was an older, since-
    // reworked mechanic. `effective_sf` is stored un-divided (see
    // `STAT_SF_CAP` doc) so the frontend can show the fractional %.
    stats.insert(STAT_SF_CAP, effective_sf);
    let energy_bonus = (130.0 * (energy_full + 20.0) / 500.0 + energy_full).floor() as i32;
    stats.insert(STAT_SF_ENERGY_BONUS, energy_bonus);

    let level = *stats.get(&12).unwrap_or(&0) as u32;
    let exp_start = exp_for_level(level).unwrap_or(0);
    let exp_next = exp_for_level(level + 1);
    stats.insert(STAT_EXP_LEVEL_START, exp_start as i32);
    stats.insert(
        STAT_EXP_LEVEL_NEXT,
        exp_next.map(|v| v as i32).unwrap_or(-1),
    );

    let mut base_stats = read_base_stats(ctx, p_unit, BASE_STAT_IDS);
    for (&id, value) in base_stats.iter_mut() {
        *value = scale_life_mana(id, *value);
    }

    Some(CharacterStats {
        class,
        stats,
        base_stats,
    })
}

/// Life/Mana/Stamina current+max stats (6-11) are stored as 8.8 fixed-point
/// (`D2Stats.au3:639-640` and `FixStats`'s `case 6 to 11` — this is a
/// genuine engine data-format detail, not an artifact of D2Stats's own
/// manual StatList summation, so it applies to engine-read values too).
fn scale_life_mana(id: u32, raw: i32) -> i32 {
    if (6..=11).contains(&id) {
        (raw as f64 / 256.0).floor() as i32
    } else {
        raw
    }
}

/// Cumulative experience required to reach level `N` (index `N - 2`, since
/// level 1 always starts at 0 exp). Sourced from Median XL's
/// `experience.txt` (identical across all 7 classes) via the community
/// MedianXLOfflineTools data files — re-verify against a current game patch
/// if displayed progress looks off.
const EXP_THRESHOLDS: &[u32] = &[
    500, 1500, 3500, 6500, 11500, 16000, 22000, 29000, 37000, 46000, 56000, 67000, 79000, 92000,
    106000, 121000, 137000, 154000, 172000, 191000, 211000, 232000, 254000, 277000, 301000, 326000,
    352000, 379000, 407000, 436000, 466000, 497000, 529000, 562000, 596000, 631000, 667000, 704000,
    742000, 781000, 821000, 862000, 904000, 947000, 991000, 1036000, 1082000, 1129000, 1177000,
    1226000, 1276000, 1327000, 1379000, 1432000, 1486000, 1541000, 1597000, 1654000, 1712000,
    1771000, 1831000, 1892000, 1954000, 2017000, 2081000, 2146000, 2212000, 2279000, 2347000,
    2416000, 2486000, 2557000, 2629000, 2702000, 2776000, 2851000, 2927000, 3004000, 3082000,
    3161000, 3241000, 3322000, 3404000, 3487000, 3571000, 3656000, 3742000, 3829000, 3917000,
    4006000, 4096000, 4187000, 4279000, 4372000, 4466000, 4561000, 4657000, 4754000, 4852000,
    4951001, 5051003, 5152007, 5254013, 5357021, 5461032, 5566047, 5672067, 5779093, 5887126,
    5996168, 6106221, 6217289, 6329375, 6442483, 6556619, 6671790, 6788004, 6905273, 7023609,
    7143029, 7263555, 7385213, 7508034, 7632061, 7757344, 7883947, 8011948, 8141447, 8272568,
    8405465, 8540331, 8677406, 8816991, 8959461, 9105282, 9255041, 9409466, 9569467, 9736181,
    9911026, 10095774, 10292636, 10504370, 10734423, 10987094, 11267753, 11583101, 11941504,
    12353407, 12831845, 13393094, 14057469, 14850333, 15803343, 16956021, 18357703, 20069975,
    22169717, 24752903, 27939338, 31878578, 36757320, 42808623, 50323420, 59664894, 71286417,
    85753945, 103773980, 126228470, 154218377, 189118067, 232643200, 286935490, 354668503,
    439179731, 544635471, 676236636, 840475685, 1045457354,
];

/// Cumulative exp required to reach `level`, or `None` when beyond the
/// known table (treat as max level in the UI).
fn exp_for_level(level: u32) -> Option<u32> {
    if level <= 1 {
        return Some(0);
    }
    EXP_THRESHOLDS.get((level - 2) as usize).copied()
}
