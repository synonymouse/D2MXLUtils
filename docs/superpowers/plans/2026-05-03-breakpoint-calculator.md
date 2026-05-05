# Breakpoint Calculator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a live breakpoint calculator tab showing attack/cast/block/recovery FPA thresholds for player and mercenary, with game data auto-read and manual override.

**Architecture:** Backend reads raw stats (IAS/FCR/FHR/FBR, weapon type, WSM) from D2 memory each scanner tick and emits them via Tauri event. Frontend owns all breakpoint math (pure integer arithmetic) and renders tables. SpeedcalcData.txt is fetched from the MXL site, cached on disk.

**Tech Stack:** Rust (ureq for HTTP, serde for serialization), Svelte 5 + TypeScript (calculation engine, reactive UI), Tauri v2 events/commands.

---

## Implementation Status (2026-05-05 — Tasks 9–11 complete, mercenary + weapon-edge-case bugs open for next session)

### Session log 2026-05-05

**Task 9 — equipped weapon detection: DONE.** Rewrote `read_equipped_weapon` in `breakpoints.rs` against the D2MOO grid path (`pInventory + 0x14 → pGrids[0] + 0x0C → ppItems[BODYLOC_RARM=4]`). Verified live with `docs/ce-scripts/verify-equipped-weapon.lua`. Removed stale `WEAPON_SWITCH`, `BODY_SLOTS_START`, `BODY_SLOT_STRIDE` from `offsets.rs::inventory`; added new `inventory::GRIDS`, `inventory_grid::*`, `body_loc::*` modules.

**Auto-detection family resolution: DONE.** Replaced numeric `BreakpointData.weapon_type: u16` with chained `family_codes: Vec<String>`. Backend reads `items.txt[fidx].wType[0]` (offset 0x11E) and walks `equiv1` up through ItemTypes.txt (`pSgptDataTables + 0xBF8`) — yields a chain like `["qaxe", "axe", "mele", "weap"]` for a Sacred War Axe. Frontend (`findWeaponTypeByWclass`) walks the chain and picks the first code that matches a token in `WEAPON_TYPES`, so MXL sub-types automatically roll up to base families. Killed the hardcoded vanilla `WEAPON_TYPE_INDEX` map (was useless against MXL's extended ItemTypes count = 346).

**Found and fixed two ancillary bugs along the way:**
- `u32_to_packed_code` (formerly `u32_to_wclass`) was returning `"BOW "` with trailing space — MXL pads 3-char codes with ASCII space, not NUL. Now stops on either `\0` or `b' '`.
- D2InventoryStrc is `0x40` bytes; the previous code was reading "body slot entries" from `inventory + 0x40` which is OUT OF BOUNDS — apparent matches were heap noise. The new grid path is the authoritative D2MOO/MXL layout.

**Task 10 — Weapon Base catalog backend: DONE.** New `weapon_families.rs` walks all of items.txt once on scanner-attach, keeps weapons (`wclass != 0`), reads name via `D2Lang.GetStringById(NAME_ID @ 0xF4)`, captures family chain + WSM + file_index. Caches to `weapon-bases.json` (schema-versioned; survives offline launches). Wired into `main.rs`: `AppState::weapon_base_catalog`, Tauri command `get_weapon_base_catalog`, event `weapon-base-catalog-updated`. Added `BreakpointData::file_index` so the frontend can pre-select the base on live updates.

**Task 11 — Weapon Base catalog frontend: DONE.** `BreakpointsTab.svelte` now:
- loads catalog on mount + listens for live updates;
- builds `basesByFamily: Map<token, WeaponBase[]>` (each base is grouped by the first family-code that matches `WEAPON_TYPE_MAP`);
- **strips MXL tier suffixes** (`(1)`, `(2)`, `(3)`, `(4)`, `(Sacred)`, `(Angelic)`, `(Mastercrafted)`) from base names so `Spear (1) … Spear (Sacred)` collapse into one **"Spear"** entry. WSM is identical across tiers anyway;
- adds a **Weapon Base** dropdown next to Weapon Type (only when catalog is loaded); shows clean names without WSM number;
- per-entity overrides: `overrideBaseFileIndex` (player) / `overrideMercBaseFileIndex` (merc); switching Weapon Type clears the base override via `setWeaponTypeOverride`;
- effective WSM source priority: selected base → live → 0;
- live match handles tiered variants: looks up `live.file_index` in the full catalog, strips its tier suffix, then matches by name in `availableBases`.

**A1/A2 column merge.** Removed Attack 2 entirely from the UI: D2 has two attack anim slots but they're identical for ~all class/weapon combinations (only edge cases like Barb Double Swing differ), and the MXL site shows only one. `ANIM_TYPES` is now `["A1", "SC", "GH", "BL"]`; `ANIM_TYPE_LABELS.A1 = "Attack"`; `breakpoint-calc.ts` lost its A2 case.

**WSM input field already removed before this session;** Throwing checkbox already removed; `WEAPON_TYPES` already 38 site-style families. (See "Tasks 0–8 evolved beyond plan" section below.)

### Known issues — TODO next session

1. **Mercenary side has bugs** — user reported during smoke-test but didn't dig in yet. Need to:
   - Verify merc weapon dropdown filtering (`MERC_WEAPON_TOKENS` in `breakpoint-constants.ts`) still surfaces the right families now that we have `family_codes` resolution.
   - Check `findWeaponTypeByWclass` path for merc tokens (RG/GU/IW/0A) — fallback `CLASS_PREFERRED_ANIM` doesn't have entries for mercs.
   - Verify catalog grouping puts merc-relevant bases under merc-allowed families (e.g. Iron Wolf → Sceptres / Crystal Swords specifically, not generic Maces).
   - Confirm live `breakpoints-update` for merc when game has no merc hired (likely returns `class=0` which collides with Rogue id 0).

2. **Some weapon combinations still wrong** — need user to enumerate which weapons + classes are still misdetected. Likely either:
   - Family code that exists in MXL but not in our `WEAPON_TYPES` (need to add it).
   - Equiv chain that walks past 6 hops or doesn't pass through a known family code (need to extend or add fallbacks).

3. **Task 12 (E2E testing)** — never executed. Defer until 1+2 are fixed.

### Tasks 0–8: completed and evolved beyond original plan

All listed steps (`speedcalc_data.rs`, `breakpoints.rs`, frontend constants/calc/tab, MainWindow wiring) are implemented. The Breakpoints tab works for both Player and Mercenary. Stats (IAS/FCR/FHR/FBR/skill velocities) read correctly via `D2Injector.get_unit_stat`. SpeedcalcData.txt fetching/caching works. Frontend formulas match the site's calculator.

The frontend has evolved well beyond what Tasks 5–7 originally described. Key divergences from the original plan (current code is the source of truth):

- **`WEAPON_CLASSES` (10 COF tokens) → `WEAPON_TYPES` (38 site-style families).** Each entry is `{ token, name, primaryAnim, blockAnim }` matching the MXL site dropdown. `token` is the items.txt code (e.g. `swor`, `axe`, `abow`), not the COF anim class. `primaryAnim` and `blockAnim` are separate because most one-handed weapons block as `1HS` regardless of attack anim.
- **Per-class and per-mercenary weapon filtering** (`getWeaponTypesForCharacter`, `MERC_WEAPON_TOKENS`). Amazon gets generic + `abow`/`aspe`/`ajav`; Iron Wolf merc only gets `swor`; etc.
- **`CHAR_OVERRIDES`** replaces the original `morphToken` plumbing. Werewolf/Werebear/Wereowl/Superbeast/Deathlord/Treewarden + Rogue merc all force `HTH` weapon anim; Deathlord/Treewarden additionally remap cast prefix to `A1`.
- **WSM input field — removed from UI.** Users don't know WSM as a number. In live mode it's read from items.txt automatically; in manual mode it currently defaults to 0 (until Task 11 lands).
- **Throwing checkbox — removed from UI**, `isThrowing` removed from `CalcParams`, `-30` throw penalty removed from the formula. Will be reintroduced via the Weapon Base catalog (Task 10) flagging throwing families.
- **Auto-detection via `weapon_type` ItemTypes.txt index** (`0x11E`) added in `findWeaponTypeByWclass`. Works for the tested classes; fallback uses class-preferred-anim mapping.
- **Separate Player vs Mercenary override state** in `BreakpointsTab.svelte` (so switching the entity toggle doesn't lose either side's manual edits).

### BLOCKER: equipped weapon detection — RESOLVED & IMPLEMENTED (Task 9)

The earlier blocker (reading "body slot entries at `inventory + 0x40`") was reading **out of bounds** of `D2InventoryStrc`. Per D2MOO source (`ThePhrozenKeep/D2MOO/source/D2Common/src/D2Inventory.cpp`):

```c
D2UnitStrc* INVENTORY_GetItemFromBodyLoc(D2InventoryStrc* pInventory, int nBodyLoc) {
    D2InventoryGridStrc* pGrid = INVENTORY_GetGrid(pInventory, INVGRID_BODYLOC, ...);
    return pGrid->ppItems[nBodyLoc];
}
```

`D2InventoryStrc` size is `0x40` (last field `nCorpseCount @ 0x3C`). The real path:

```
pPlayer + 0x60         → pInventory          (UnitAny.pInventory)
pInventory + 0x14      → pGrids              (D2InventoryGridStrc[])
pGrids + 0 * 0x10      → BodyLoc grid        (INVGRID_BODYLOC = 0)
+ 0x0C                 → ppItems             (D2UnitStrc** of length 13)
ppItems + bodyLoc * 4  → equipped item unit  (BODYLOC_RARM = 4)
```

**Verified live against MXL** with `docs/ce-scripts/verify-equipped-weapon.lua`:
- Layout matches D2MOO 1-to-1 (BodyLoc grid `width=13, height=1`, MXL has `nGridCount=3`).
- The engine **physically swaps the weapon into `BODYLOC_RARM`** on weapon switch (W key). Before swap: `ppItems[4]` = axe `{wclass=1hs, wsm=0}`, `ppItems[11]` = bow `{wclass=bow, wsm=8}`. After swap: `ppItems[4]` = bow, `ppItems[11]` = axe. Therefore **we never need to read `BODYLOC_SWRARM`** and we don't need a swap flag.

**Stale offsets to remove from `offsets.rs::inventory`:**
- `WEAPON_SWITCH = 0x24` — this is actually `dwOwnerGuid` (always `0x01` for player). The plan's earlier claim that it toggles 0/1 on swap was a misobservation; live verification showed it's constant at `0x01` before and after pressing W.
- `BODY_SLOTS_START = 0x40` and `BODY_SLOT_STRIDE = 0x40` — these fields don't exist; reads were landing in heap memory beyond the inventory struct.

**New offsets to add:**
- `inventory::GRIDS = 0x14` (`D2InventoryGridStrc*`)
- new `mod inventory_grid` with `PP_ITEMS = 0x0C`, `SIZE = 0x10`
- new `mod body_loc` with at minimum `RARM = 4`

Implementation = Task 9 below.

---

### Weapon Base catalog (Variant 2) — IMPLEMENTED (Tasks 10 + 11)

The MXL site `https://dev.median-xl.com/speedcalc/` uses **two** weapon dropdowns:
1. **Weapon Type** (family) — what we already have as `WEAPON_TYPES` (One-Handed Axes, Bows, etc.).
2. **Weapon Base** — a specific weapon within the family (Hand Axe / Axe / Double Axe / War Axe), each with its own WSM.

We currently lack the second dropdown. In live mode WSM is read from items.txt; in manual / offline mode WSM is effectively 0 (input field has been removed because users don't know WSM as a number).

**Goal:** Add `Weapon Base` dropdown. Auto-prefill from the equipped weapon in live mode. Allow manual override (user picks a different base → its WSM enters the formula).

**Data source:** read from live items.txt at runtime (we already access items.txt via `notifier.rs::class_cache` and `items_cache.rs`). For each weapon record: name (via `D2Lang.GetStringById(NAME_ID @ 0xF4)`), `wclass @ 0xC0`, `wsm @ 0xD8`, `weapon_type @ 0x11E`. Group by ItemTypes.txt category (= site's "Weapon Type"). Cache to `weapon-families.json` next to `speedcalc-data.json` so the catalog survives offline launches.

Implementation = Tasks 10 + 11 below.

---

## File Structure

### Backend (create)
- `src-tauri/src/speedcalc_data.rs` — fetch, parse, cache SpeedcalcData.txt
- `src-tauri/src/breakpoints.rs` — read player/merc breakpoint params from game memory

### Backend (modify)
- `src-tauri/Cargo.toml` — add `ureq` dependency
- `src-tauri/src/main.rs` — add module declarations, AppState field, commands, scanner loop hook
- ~~`src-tauri/src/offsets.rs`~~ — DONE: added `SPEED` (0xD8), `WCLASS` (0xC0), `WCLASS_2H` (0xC4)

### Frontend (create)
- `src/lib/breakpoint-calc.ts` — pure calculation functions (formulas + table generation)
- `src/lib/breakpoint-constants.ts` — bundled class/weapon/morph/debuff data
- `src/views/BreakpointsTab.svelte` — tab UI component

### Frontend (modify)
- `src/views/MainWindow.svelte` — add Breakpoints tab to tabs array and routing
- `src/views/index.ts` — export BreakpointsTab

---

## Task 0: Verify items.txt `speed` (WSM) Offset — DONE

**Goal:** Find the byte offset of the `speed` field (Weapon Speed Modifier) within items.txt records in game memory.

**Files:**
- Modify: `src-tauri/src/offsets.rs`

**Results (verified via CE Lua scripts on 500 records):**

- [x] **SPEED offset: `0xD8` (i32, not i8)**. D2MOO field `dwSpeed`. Verified: Crystal Sword = −10, Great Maul = +10, War Staff = +20. Hundreds of MXL weapons confirmed with values in −25 to +20 range.

- [x] **WEAPON_TYPE offset `0x11E` confirmed** — contains ItemTypes.txt indices (26, 28, 30, 32, 34, 38, 43, etc.), NOT simple enum codes 1-10 as originally assumed.

- [x] **NEW: WCLASS offset `0xC0` (u32, 4-char string code)** — D2MOO field `dwWeapClass`. Stores weapon animation class directly as a packed string: `"1hs"`, `"1ht"`, `"2hs"`, `"2ht"`, `"stf"`, `"bow"`, `"xbw"`, `"ht1"`. Also found `WCLASS_2H` at `0xC4` (two-hand override).

- [x] **All three offsets added to `offsets.rs`.**

**Architecture change:** Use `WCLASS` (0xC0, string) instead of `WEAPON_TYPE` (0x11E, ItemTypes.txt index) for the breakpoint calculator. This eliminates the need for a numeric-to-token mapping table — the string code goes directly into SpeedcalcData.txt COF key lookup. Tasks 3, 5, and 7 are updated below to reflect this.

CE verification scripts: `docs/ce-scripts/verify-wsm-offset.lua`

---

## Task 1: Add `ureq` Dependency — DONE

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [x] **Step 1: Add ureq to Cargo.toml**

Add after the `windows` dependency:

```toml
ureq = "3"
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`

Expected: compiles without errors.

- [ ] **Step 3: Commit**

```
feat: add ureq HTTP client for speedcalc data fetching
```

---

## Task 2: SpeedcalcData Module — DONE

**Files:**
- Create: `src-tauri/src/speedcalc_data.rs`

- [ ] **Step 1: Create the module with types and parser**

```rust
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::logger::{error as log_error, info as log_info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimEntry {
    pub frames: u16,
    pub anim_speed: u16,
}

pub type SpeedcalcTable = HashMap<String, AnimEntry>;

const SPEEDCALC_URL: &str = "https://dev.median-xl.com/speedcalc/SpeedcalcData.txt";
const CACHE_FILE: &str = "speedcalc-data.json";

pub fn parse_tsv(raw: &str) -> SpeedcalcTable {
    let mut table = HashMap::new();
    for line in raw.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let cof_name = parts[0].to_string();
        let frames = match parts[1].parse::<u16>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let anim_speed = match parts[2].parse::<u16>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        table.insert(cof_name, AnimEntry { frames, anim_speed });
    }
    table
}

pub fn fetch_from_site() -> Result<String, String> {
    let response = ureq::get(SPEEDCALC_URL)
        .call()
        .map_err(|e| format!("Failed to fetch SpeedcalcData.txt: {}", e))?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    Ok(body)
}

pub fn load_from_cache(app_data_dir: &PathBuf) -> Option<SpeedcalcTable> {
    let path = app_data_dir.join(CACHE_FILE);
    if !path.exists() {
        return None;
    }
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            log_error(&format!("speedcalc cache: read failed: {}", e));
            return None;
        }
    };
    match serde_json::from_str::<SpeedcalcTable>(&content) {
        Ok(table) => {
            log_info(&format!(
                "speedcalc cache: loaded {} entries from {}",
                table.len(),
                path.display()
            ));
            Some(table)
        }
        Err(e) => {
            log_error(&format!("speedcalc cache: parse failed: {}", e));
            None
        }
    }
}

pub fn save_to_cache(app_data_dir: &PathBuf, table: &SpeedcalcTable) -> Result<(), String> {
    if !app_data_dir.exists() {
        fs::create_dir_all(app_data_dir)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    let path = app_data_dir.join(CACHE_FILE);
    let json = serde_json::to_string(table)
        .map_err(|e| format!("Failed to serialize speedcalc data: {}", e))?;
    fs::write(&path, json)
        .map_err(|e| format!("Failed to write speedcalc cache: {}", e))?;
    log_info(&format!(
        "speedcalc cache: wrote {} entries to {}",
        table.len(),
        path.display()
    ));
    Ok(())
}

pub fn fetch_and_cache(app_data_dir: &PathBuf) -> Result<SpeedcalcTable, String> {
    let raw = fetch_from_site()?;
    let table = parse_tsv(&raw);
    if table.is_empty() {
        return Err("Parsed SpeedcalcData.txt but got 0 entries".to_string());
    }
    if let Err(e) = save_to_cache(app_data_dir, &table) {
        log_error(&format!("speedcalc: cache save failed (non-fatal): {}", e));
    }
    Ok(table)
}
```

- [ ] **Step 2: Add module declaration to main.rs**

Add to the module declarations at the top of `src-tauri/src/main.rs`:

```rust
mod speedcalc_data;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`

- [ ] **Step 4: Commit**

```
feat: add speedcalc_data module for fetching/caching breakpoint animation data
```

---

## Task 3: Breakpoints Reader Module — DONE (weapon detection BLOCKED)

**Files:**
- Create: `src-tauri/src/breakpoints.rs`

- [ ] **Step 1: Create the module with BreakpointData struct and reader**

```rust
use serde::Serialize;

use crate::logger::error as log_error;
use crate::offsets::{d2common, inventory, item_data, items_txt, unit};
use crate::process::D2Context;
use crate::injection::D2Injector;

#[derive(Debug, Clone, Serialize, Default)]
pub struct BreakpointData {
    pub class: u32,
    pub wclass: String,
    pub wsm: i32,
    pub ias: i32,
    pub fcr: i32,
    pub fhr: i32,
    pub fbr: i32,
    pub skill_ias: i32,
    pub skill_fhr: i32,
}

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

    let ias = read_stat(ctx, injector, p_unit, STAT_IAS);
    let fcr = read_stat(ctx, injector, p_unit, STAT_FCR);
    let fhr = read_stat(ctx, injector, p_unit, STAT_FHR);
    let fbr = read_stat(ctx, injector, p_unit, STAT_FBR);
    let skill_ias = read_stat(ctx, injector, p_unit, STAT_SKILL_IAS);
    let skill_fhr = read_stat(ctx, injector, p_unit, STAT_SKILL_FHR);

    let (wclass, wsm) = read_weapon_info(ctx, p_unit);

    Some(BreakpointData {
        class,
        wclass,
        wsm,
        ias,
        fcr,
        fhr,
        fbr,
        skill_ias,
        skill_fhr,
    })
}

fn read_stat(ctx: &D2Context, injector: &D2Injector, p_unit: u32, stat_id: u32) -> i32 {
    match injector.get_unit_stat(&ctx.process, p_unit, stat_id) {
        Ok(v) => v as i32,
        Err(_) => 0,
    }
}

fn read_weapon_info(ctx: &D2Context, p_unit: u32) -> (String, i32) {
    let p_inventory = match ctx
        .process
        .read_memory::<u32>(p_unit as usize + unit::INVENTORY)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return (String::new(), 0),
    };

    let weapon_id = match ctx
        .process
        .read_memory::<u32>(p_inventory + inventory::WEAPON_ID)
    {
        Ok(id) if id != 0 => id,
        _ => return (String::new(), 0),
    };

    let p_weapon = match find_item_by_id(ctx, p_inventory, weapon_id) {
        Some(p) => p,
        None => return (String::new(), 0),
    };

    let p_item_data = match ctx
        .process
        .read_memory::<u32>(p_weapon as usize + unit::UNIT_DATA)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return (String::new(), 0),
    };

    let file_index = match ctx
        .process
        .read_memory::<u32>(p_item_data + item_data::FILE_INDEX)
    {
        Ok(idx) => idx as usize,
        Err(_) => return (String::new(), 0),
    };

    read_weapon_fields_from_items_txt(ctx, file_index)
}

fn find_item_by_id(ctx: &D2Context, p_inventory: usize, target_id: u32) -> Option<u32> {
    let mut p_item = match ctx
        .process
        .read_memory::<u32>(p_inventory + inventory::FIRST_ITEM)
    {
        Ok(p) if p != 0 => p,
        _ => return None,
    };

    for _ in 0..256 {
        if p_item == 0 {
            break;
        }
        let uid = ctx
            .process
            .read_memory::<u32>(p_item as usize + unit::UNIT_ID)
            .unwrap_or(0);
        if uid == target_id {
            return Some(p_item);
        }
        let p_unit_data = match ctx
            .process
            .read_memory::<u32>(p_item as usize + unit::UNIT_DATA)
        {
            Ok(p) if p != 0 => p as usize,
            _ => break,
        };
        p_item = ctx
            .process
            .read_memory::<u32>(p_unit_data + item_data::NEXT_ITEM)
            .unwrap_or(0);
    }
    None
}

fn read_weapon_fields_from_items_txt(ctx: &D2Context, file_index: usize) -> (String, i32) {
    let base_ptr = match ctx
        .process
        .read_memory::<u32>(ctx.d2_common + d2common::ITEMS_TXT)
    {
        Ok(p) if p != 0 => p as usize,
        _ => return (String::new(), 0),
    };

    let record = base_ptr + file_index * items_txt::RECORD_SIZE;

    // WCLASS is a 4-byte packed string (e.g. "1hs\0" as little-endian u32)
    let wclass_raw = ctx
        .process
        .read_memory::<u32>(record + items_txt::WCLASS)
        .unwrap_or(0);
    let wclass = u32_to_wclass(wclass_raw);

    let wsm = ctx
        .process
        .read_memory::<i32>(record + items_txt::SPEED)
        .unwrap_or(0);

    (wclass, wsm)
}

fn u32_to_wclass(raw: u32) -> String {
    let bytes = raw.to_le_bytes();
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(4);
    String::from_utf8_lossy(&bytes[..len]).to_uppercase()
}
```

- [ ] **Step 2: Add module declaration to main.rs**

Add to the module declarations in `src-tauri/src/main.rs`:

```rust
mod breakpoints;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`

Task 0 has already added SPEED (0xD8), WCLASS (0xC0), and WCLASS_2H (0xC4) to offsets.rs.

- [ ] **Step 4: Commit**

```
feat: add breakpoints module for reading character/merc speed stats
```

---

## Task 4: Backend Integration (main.rs) — DONE

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Add breakpoints_polling to AppState**

In the `AppState` struct, add:

```rust
breakpoints_polling: Arc<AtomicBool>,
```

And in the `AppState` initialization within `.setup()` (where the struct is constructed), add:

```rust
breakpoints_polling: Arc::new(AtomicBool::new(false)),
```

- [ ] **Step 2: Add speedcalc_data state to AppState**

Add to AppState:

```rust
speedcalc_table: Arc<RwLock<Option<speedcalc_data::SpeedcalcTable>>>,
```

And in initialization:

```rust
speedcalc_table: Arc::new(RwLock::new(None)),
```

- [ ] **Step 3: Add Tauri commands**

```rust
#[tauri::command]
fn set_breakpoints_polling(enabled: bool, state: tauri::State<AppState>) {
    state.breakpoints_polling.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
fn get_speedcalc_data(state: tauri::State<AppState>) -> Option<speedcalc_data::SpeedcalcTable> {
    state
        .speedcalc_table
        .read()
        .ok()
        .and_then(|guard| guard.clone())
}

#[tauri::command]
fn refresh_speedcalc_data(
    state: tauri::State<AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let table = speedcalc_data::fetch_and_cache(&app_data_dir)?;
    if let Ok(mut guard) = state.speedcalc_table.write() {
        *guard = Some(table);
    }
    Ok(())
}
```

- [ ] **Step 4: Register commands in invoke_handler**

Add to the `tauri::generate_handler![]` list:

```rust
set_breakpoints_polling,
get_speedcalc_data,
refresh_speedcalc_data,
```

- [ ] **Step 5: Load speedcalc cache on startup**

In the `.setup()` closure, after items_cache loading, add:

```rust
let speedcalc_table_clone = app_state.speedcalc_table.clone();
let app_data_for_speedcalc = app_handle.path().app_data_dir().ok();
if let Some(ref dir) = app_data_for_speedcalc {
    if let Some(table) = speedcalc_data::load_from_cache(dir) {
        if let Ok(mut guard) = speedcalc_table_clone.write() {
            *guard = Some(table);
        }
    }
}
```

- [ ] **Step 6: Add breakpoint polling to the scanner loop**

Inside the scanner loop's `if ingame { ... }` block (after the loot history section, before `thread::sleep`), add:

```rust
if breakpoints_polling.load(Ordering::Relaxed) {
    let injector = shared_state.injector.lock().unwrap();
    let player_data = breakpoints::read_unit_breakpoint_data(
        &shared_state.ctx,
        &injector,
        d2client::PLAYER_UNIT,
    );
    let merc_data = breakpoints::read_unit_breakpoint_data(
        &shared_state.ctx,
        &injector,
        d2client::MERCENARY_UNIT,
    );
    drop(injector);

    #[derive(serde::Serialize)]
    struct BreakpointsPayload {
        player: Option<breakpoints::BreakpointData>,
        merc: Option<breakpoints::BreakpointData>,
    }
    let payload = BreakpointsPayload {
        player: player_data,
        merc: merc_data,
    };
    if let Err(e) = app_handle.emit("breakpoints-update", &payload) {
        log_error(&format!("Failed to emit breakpoints-update: {}", e));
    }
}
```

Also pass `breakpoints_polling` into the scanner thread closure (clone the Arc alongside the existing ones like `filter_enabled`, `is_scanning`, etc.):

```rust
let breakpoints_polling = state.breakpoints_polling.clone();
```

- [ ] **Step 7: Verify it compiles**

Run: `cd src-tauri && cargo check`

- [ ] **Step 8: Commit**

```
feat: integrate breakpoints polling into scanner loop with Tauri commands
```

---

## Task 5: Frontend — Breakpoint Constants — DONE

**Files:**
- Create: `src/lib/breakpoint-constants.ts`

- [ ] **Step 1: Create the constants file**

```typescript
export interface ClassInfo {
    id: number;
    name: string;
    token: string;
}

export interface MorphInfo {
    name: string;
    token: string;
    baseClass: string; // token of the class this morph belongs to
}

export interface MercInfo {
    id: number;
    name: string;
    token: string;
}

export interface DebuffInfo {
    name: string;
    value: number;
}

export interface WeaponClassInfo {
    token: string;
    name: string;
}

export const CLASSES: ClassInfo[] = [
    { id: 0, name: "Amazon", token: "AM" },
    { id: 1, name: "Sorceress", token: "SO" },
    { id: 2, name: "Necromancer", token: "NE" },
    { id: 3, name: "Paladin", token: "PA" },
    { id: 4, name: "Barbarian", token: "BA" },
    { id: 5, name: "Druid", token: "DZ" },
    { id: 6, name: "Assassin", token: "AI" },
];

export const MORPHS: MorphInfo[] = [
    { name: "Werewolf", token: "40", baseClass: "DZ" },
    { name: "Werebear", token: "TG", baseClass: "DZ" },
    { name: "Wereowl", token: "OW", baseClass: "DZ" },
    { name: "Superbeast", token: "~Z", baseClass: "PA" },
    { name: "Deathlord", token: "0N", baseClass: "NE" },
    { name: "Treewarden", token: "TH", baseClass: "BA" },
];

export const MERCS: MercInfo[] = [
    { id: 0, name: "Rogue (Act 1)", token: "RG" },
    { id: 1, name: "Town Guard (Act 2)", token: "GU" },
    { id: 2, name: "Iron Wolf (Act 3)", token: "IW" },
    { id: 3, name: "Barbarian (Act 5)", token: "0A" },
];

export const DEBUFFS: DebuffInfo[] = [
    { name: "None", value: 0 },
    { name: "Decrepify", value: -20 },
    { name: "Phobos", value: -20 },
    { name: "Uldyssian", value: -30 },
    { name: "Chill", value: -50 },
];

export const ANIM_TYPES = ["A1", "A2", "SC", "GH", "BL"] as const;
export type AnimType = (typeof ANIM_TYPES)[number];

export const ANIM_TYPE_LABELS: Record<AnimType, string> = {
    A1: "Attack 1",
    A2: "Attack 2",
    SC: "Cast",
    GH: "Hit Recovery",
    BL: "Block",
};

// Weapon class tokens — read directly from items.txt WCLASS field (0xC0).
// No numeric mapping needed; the game stores the string code as a packed u32.
export const WEAPON_CLASSES: WeaponClassInfo[] = [
    { token: "HTH", name: "Hand-to-Hand" },
    { token: "1HS", name: "One-Hand Swing" },
    { token: "1HT", name: "One-Hand Thrust" },
    { token: "2HS", name: "Two-Hand Swing" },
    { token: "2HT", name: "Two-Hand Thrust (Spear)" },
    { token: "STF", name: "Staff" },
    { token: "BOW", name: "Bow" },
    { token: "XBW", name: "Crossbow" },
    { token: "HT1", name: "Claw (Single)" },
    { token: "HT2", name: "Claw (Dual)" },
];

// Classes that have StartingFrame = 2 for melee/staff attack animations.
export const STARTING_FRAME_CLASSES = new Set(["AM", "SO"]);
// Weapon tokens that trigger the StartingFrame adjustment.
export const STARTING_FRAME_WEAPONS = new Set(["1HS", "1HT", "2HS", "2HT", "STF"]);
```

- [ ] **Step 2: Commit**

```
feat: add breakpoint calculator constants (classes, morphs, weapon types)
```

---

## Task 6: Frontend — Calculation Library — DONE

**Files:**
- Create: `src/lib/breakpoint-calc.ts`

- [ ] **Step 1: Create the calculation module**

```typescript
import {
    STARTING_FRAME_CLASSES,
    STARTING_FRAME_WEAPONS,
    type AnimType,
} from "./breakpoint-constants";

export interface AnimData {
    frames: number;
    animSpeed: number;
}

export interface BreakpointEntry {
    fpa: number;
    requiredStat: number;
}

export interface BreakpointTable {
    animType: AnimType;
    entries: BreakpointEntry[];
    currentFpa: number;
    currentStat: number;
    nextBreakpoint: BreakpointEntry | null;
    delta: number | null; // stat needed to reach next BP
}

export type SpeedcalcTable = Record<string, { frames: number; anim_speed: number }>;

function lookupAnim(
    table: SpeedcalcTable,
    classToken: string,
    animType: string,
    weaponToken: string,
): AnimData | null {
    const key = `${classToken}${animType}${weaponToken}`;
    const entry = table[key];
    if (!entry) return null;
    return { frames: entry.frames, animSpeed: entry.anim_speed };
}

function calcAttackFpa(
    frames: number,
    animSpeed: number,
    ias: number,
    wsm: number,
    skillSlow: number,
    isThrowing: boolean,
    hasStartingFrame: boolean,
): number {
    const effectiveFrames = hasStartingFrame ? frames - 2 : frames;
    const eIAS = Math.floor((120 * ias) / (120 + ias));
    const throwPenalty = isThrowing ? 30 : 0;
    const effective = Math.min(eIAS - wsm + skillSlow - throwPenalty, 75);
    const divisor = Math.floor((animSpeed * (100 + effective)) / 100);
    if (divisor <= 0) return effectiveFrames;
    return Math.ceil((256 * effectiveFrames) / divisor) - 1;
}

function calcCastFpa(
    frames: number,
    animSpeed: number,
    fcr: number,
    skillSlow: number,
): number {
    const eFCR = Math.min(Math.floor((120 * fcr) / (120 + fcr)) + skillSlow, 75);
    const divisor = Math.floor((animSpeed * (100 + eFCR)) / 100);
    if (divisor <= 0) return frames;
    return Math.ceil((256 * frames) / divisor) - 1;
}

function calcDefensiveFpa(
    frames: number,
    animSpeed: number,
    stat: number,
    skillSlow: number,
): number {
    const eStat = Math.floor((120 * stat) / (120 + stat));
    const divisor = Math.floor((animSpeed * (50 + eStat + skillSlow)) / 100);
    if (divisor <= 0) return frames;
    return Math.ceil((256 * frames) / divisor) - 1;
}

function calcWereformAttackFpa(
    table: SpeedcalcTable,
    morphToken: string,
    ias: number,
    wsm: number,
    skillSlow: number,
    isThrowing: boolean,
): number | null {
    const nuEntry = lookupAnim(table, morphToken, "NU", "HTH");
    const a1Entry = lookupAnim(table, morphToken, "A1", "HTH");
    if (!nuEntry || !a1Entry) return null;

    const eIAS = Math.floor((120 * ias) / (120 + ias));
    const inner = Math.floor(((100 + eIAS - wsm) * a1Entry.animSpeed) / 100);
    if (inner <= 0) return null;
    const wAnimSpeed = Math.floor(
        (256 * nuEntry.frames) / Math.floor((256 * a1Entry.frames) / inner),
    );

    const throwPenalty = isThrowing ? 30 : 0;
    const effective = Math.min(eIAS - wsm + skillSlow - throwPenalty, 75);
    const divisor = Math.floor((wAnimSpeed * (100 + effective)) / 100);
    if (divisor <= 0) return null;
    return Math.ceil((256 * a1Entry.frames) / divisor) - 1;
}

function findRequiredStat(
    targetFpa: number,
    calcFn: (stat: number) => number,
): number {
    // Binary search for minimum stat that achieves targetFpa
    let lo = 0;
    let hi = 500;
    const baseFpa = calcFn(0);
    if (baseFpa <= targetFpa) return 0;
    if (calcFn(hi) > targetFpa) return -1; // unreachable

    while (lo < hi) {
        const mid = Math.floor((lo + hi) / 2);
        if (calcFn(mid) <= targetFpa) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    return lo;
}

export interface CalcParams {
    classToken: string;
    morphToken: string | null;
    weaponToken: string;
    wsm: number;
    ias: number;
    fcr: number;
    fhr: number;
    fbr: number;
    skillIas: number;
    skillFhr: number;
    debuff: number;
    isThrowing: boolean;
}

export function computeBreakpointTable(
    table: SpeedcalcTable,
    params: CalcParams,
    animType: AnimType,
): BreakpointTable | null {
    const token = params.morphToken || params.classToken;
    const anim = lookupAnim(table, token, animType, params.weaponToken);
    if (!anim) return null;

    const skillSlow = params.debuff;
    const hasStartingFrame =
        animType === "A1" &&
        !params.morphToken &&
        STARTING_FRAME_CLASSES.has(params.classToken) &&
        STARTING_FRAME_WEAPONS.has(params.weaponToken);

    let currentStat: number;
    let calcFn: (stat: number) => number;

    switch (animType) {
        case "A1":
        case "A2":
            currentStat = params.ias;
            if (params.morphToken) {
                const morphToken = params.morphToken;
                calcFn = (s) =>
                    calcWereformAttackFpa(
                        table,
                        morphToken,
                        s,
                        params.wsm,
                        skillSlow,
                        params.isThrowing,
                    ) ?? anim.frames;
            } else {
                calcFn = (s) =>
                    calcAttackFpa(
                        anim.frames,
                        anim.animSpeed,
                        s,
                        params.wsm,
                        skillSlow,
                        params.isThrowing,
                        hasStartingFrame,
                    );
            }
            break;
        case "SC":
            currentStat = params.fcr;
            calcFn = (s) => calcCastFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
        case "GH":
            currentStat = params.fhr;
            calcFn = (s) => calcDefensiveFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
        case "BL":
            currentStat = params.fbr;
            calcFn = (s) => calcDefensiveFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
    }

    // Generate all breakpoint entries
    const entries: BreakpointEntry[] = [];
    const maxFpa = calcFn(0);
    const minFpa = calcFn(500);
    let prevFpa = -1;

    for (let fpa = maxFpa; fpa >= minFpa; fpa--) {
        const required = findRequiredStat(fpa, calcFn);
        if (required < 0) continue;
        if (fpa === prevFpa) continue;
        // Verify this is actually a distinct breakpoint
        const actualFpa = calcFn(required);
        if (actualFpa !== fpa) continue;
        if (entries.length > 0 && entries[entries.length - 1].requiredStat === required) continue;
        entries.push({ fpa, requiredStat: required });
        prevFpa = fpa;
    }

    const currentFpa = calcFn(currentStat);
    const nextBreakpoint =
        entries.find((e) => e.fpa < currentFpa && e.requiredStat > currentStat) ?? null;
    const delta = nextBreakpoint ? nextBreakpoint.requiredStat - currentStat : null;

    return {
        animType,
        entries,
        currentFpa,
        currentStat,
        nextBreakpoint,
        delta,
    };
}
```

- [ ] **Step 2: Commit**

```
feat: add breakpoint calculation library (formulas + table generation)
```

---

## Task 7: Frontend — BreakpointsTab Component — DONE

**Files:**
- Create: `src/views/BreakpointsTab.svelte`

- [ ] **Step 1: Create the tab component**

```svelte
<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import { onMount } from "svelte";
    import {
        CLASSES,
        MORPHS,
        MERCS,
        DEBUFFS,
        ANIM_TYPES,
        ANIM_TYPE_LABELS,
        WEAPON_CLASSES,
        type AnimType,
    } from "../lib/breakpoint-constants";
    import {
        computeBreakpointTable,
        type BreakpointTable,
        type CalcParams,
        type SpeedcalcTable,
    } from "../lib/breakpoint-calc";

    interface BreakpointData {
        class: number;
        wclass: string;
        wsm: number;
        ias: number;
        fcr: number;
        fhr: number;
        fbr: number;
        skill_ias: number;
        skill_fhr: number;
    }

    interface BreakpointsPayload {
        player: BreakpointData | null;
        merc: BreakpointData | null;
    }

    // State
    let speedcalcTable = $state<SpeedcalcTable | null>(null);
    let livePlayer = $state<BreakpointData | null>(null);
    let liveMerc = $state<BreakpointData | null>(null);
    let loadError = $state<string | null>(null);
    let activeEntity = $state<"player" | "merc">("player");

    // Manual overrides (null = use live data)
    let overrideClass = $state<number | null>(null);
    let overrideMorph = $state<string | null>(null);
    let overrideWeapon = $state<string | null>(null);
    let overrideWsm = $state<number | null>(null);
    let overrideIas = $state<string>("");
    let overrideFcr = $state<string>("");
    let overrideFhr = $state<string>("");
    let overrideFbr = $state<string>("");
    let debuffIndex = $state(0);
    let isThrowing = $state(false);

    // Derived: effective params for calculation
    let calcParams = $derived.by((): CalcParams | null => {
        if (!speedcalcTable) return null;

        const live = activeEntity === "player" ? livePlayer : liveMerc;
        const classId = overrideClass ?? live?.class ?? 0;

        let classToken: string;
        if (activeEntity === "merc") {
            classToken = MERCS[classId]?.token ?? "RG";
        } else {
            classToken = CLASSES[classId]?.token ?? "AM";
        }

        const weaponToken =
            overrideWeapon ?? live?.wclass ?? "HTH";

        const wsm = overrideWsm ?? live?.wsm ?? 0;
        const ias = overrideIas !== "" ? parseInt(overrideIas) || 0 : live?.ias ?? 0;
        const fcr = overrideFcr !== "" ? parseInt(overrideFcr) || 0 : live?.fcr ?? 0;
        const fhr = overrideFhr !== "" ? parseInt(overrideFhr) || 0 : live?.fhr ?? 0;
        const fbr = overrideFbr !== "" ? parseInt(overrideFbr) || 0 : live?.fbr ?? 0;

        return {
            classToken,
            morphToken: overrideMorph,
            weaponToken,
            wsm,
            ias,
            fcr,
            fhr,
            fbr,
            skillIas: live?.skill_ias ?? 0,
            skillFhr: live?.skill_fhr ?? 0,
            debuff: DEBUFFS[debuffIndex]?.value ?? 0,
            isThrowing,
        };
    });

    // Derived: breakpoint tables for all animation types
    let tables = $derived.by((): BreakpointTable[] => {
        if (!speedcalcTable || !calcParams) return [];
        const result: BreakpointTable[] = [];
        for (const animType of ANIM_TYPES) {
            const table = computeBreakpointTable(
                speedcalcTable,
                calcParams,
                animType as AnimType,
            );
            if (table && table.entries.length > 0) {
                result.push(table);
            }
        }
        return result;
    });

    // Derived: display values for the stats section
    let displayClass = $derived.by(() => {
        const live = activeEntity === "player" ? livePlayer : liveMerc;
        const classId = overrideClass ?? live?.class ?? 0;
        if (activeEntity === "merc") return MERCS[classId]?.name ?? "Unknown";
        return CLASSES[classId]?.name ?? "Unknown";
    });

    function clearOverrides() {
        overrideClass = null;
        overrideMorph = null;
        overrideWeapon = null;
        overrideWsm = null;
        overrideIas = "";
        overrideFcr = "";
        overrideFhr = "";
        overrideFbr = "";
        debuffIndex = 0;
        isThrowing = false;
    }

    onMount(() => {
        const unlisteners: UnlistenFn[] = [];

        // Start polling
        invoke("set_breakpoints_polling", { enabled: true });

        // Load speedcalc data
        invoke<SpeedcalcTable | null>("get_speedcalc_data").then((data) => {
            if (data && Object.keys(data).length > 0) {
                speedcalcTable = data;
            } else {
                // Try fetching from site
                invoke("refresh_speedcalc_data")
                    .then(() => invoke<SpeedcalcTable | null>("get_speedcalc_data"))
                    .then((freshData) => {
                        if (freshData && Object.keys(freshData).length > 0) {
                            speedcalcTable = freshData;
                        } else {
                            loadError = "Failed to load breakpoint data";
                        }
                    })
                    .catch((e) => {
                        loadError = `Failed to fetch breakpoint data: ${e}`;
                    });
            }
        });

        // Listen for live updates
        listen<BreakpointsPayload>("breakpoints-update", (event) => {
            livePlayer = event.payload.player;
            liveMerc = event.payload.merc;
        }).then((u) => unlisteners.push(u));

        return () => {
            invoke("set_breakpoints_polling", { enabled: false });
            unlisteners.forEach((u) => u());
        };
    });
</script>

<div class="breakpoints-tab">
    {#if loadError}
        <div class="error-banner">{loadError}</div>
    {/if}

    <!-- Entity toggle -->
    <div class="entity-toggle">
        <button
            class="entity-btn"
            class:active={activeEntity === "player"}
            onclick={() => { activeEntity = "player"; clearOverrides(); }}
        >
            Player
        </button>
        <button
            class="entity-btn"
            class:active={activeEntity === "merc"}
            onclick={() => { activeEntity = "merc"; clearOverrides(); }}
        >
            Mercenary
        </button>
    </div>

    <!-- Controls -->
    <div class="controls">
        <div class="control-row">
            <label>
                <span class="label">Class</span>
                <select
                    value={overrideClass ?? (activeEntity === "player" ? livePlayer?.class : liveMerc?.class) ?? 0}
                    onchange={(e) => { overrideClass = parseInt(e.currentTarget.value); }}
                >
                    {#each activeEntity === "merc" ? MERCS : CLASSES as cls, i}
                        <option value={activeEntity === "merc" ? cls.id : i}>{cls.name}</option>
                    {/each}
                </select>
            </label>

            <label>
                <span class="label">Morph</span>
                <select
                    value={overrideMorph ?? ""}
                    onchange={(e) => { overrideMorph = e.currentTarget.value || null; }}
                >
                    <option value="">None</option>
                    {#each MORPHS as morph}
                        <option value={morph.token}>{morph.name}</option>
                    {/each}
                </select>
            </label>

            <label>
                <span class="label">Weapon</span>
                <select
                    value={overrideWeapon ?? ""}
                    onchange={(e) => { overrideWeapon = e.currentTarget.value || null; }}
                >
                    <option value="">Auto</option>
                    {#each WEAPON_CLASSES as wc}
                        <option value={wc.token}>{wc.name}</option>
                    {/each}
                </select>
            </label>

            <label>
                <span class="label">Debuff</span>
                <select
                    value={debuffIndex}
                    onchange={(e) => { debuffIndex = parseInt(e.currentTarget.value); }}
                >
                    {#each DEBUFFS as debuff, i}
                        <option value={i}>{debuff.name} ({debuff.value})</option>
                    {/each}
                </select>
            </label>

            <label class="checkbox-label">
                <input type="checkbox" bind:checked={isThrowing} />
                <span>Throwing</span>
            </label>
        </div>

        <div class="control-row">
            <label>
                <span class="label">IAS</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.ias : liveMerc?.ias)?.toString() ?? "0"}
                    bind:value={overrideIas}
                />
            </label>
            <label>
                <span class="label">FCR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fcr : liveMerc?.fcr)?.toString() ?? "0"}
                    bind:value={overrideFcr}
                />
            </label>
            <label>
                <span class="label">FHR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fhr : liveMerc?.fhr)?.toString() ?? "0"}
                    bind:value={overrideFhr}
                />
            </label>
            <label>
                <span class="label">FBR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fbr : liveMerc?.fbr)?.toString() ?? "0"}
                    bind:value={overrideFbr}
                />
            </label>
            <label>
                <span class="label">WSM</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.wsm : liveMerc?.wsm)?.toString() ?? "0"}
                    onchange={(e) => { overrideWsm = e.currentTarget.value ? parseInt(e.currentTarget.value) : null; }}
                    value={overrideWsm ?? ""}
                />
            </label>
        </div>
    </div>

    <!-- Current status -->
    <div class="current-status">
        <span class="status-class">{displayClass}</span>
        {#if calcParams}
            <span class="status-weapon">Weapon: {calcParams.weaponToken}</span>
            <span class="status-wsm">WSM: {calcParams.wsm}</span>
        {/if}
    </div>

    <!-- Breakpoint tables -->
    <div class="tables-container">
        {#each tables as bpTable (bpTable.animType)}
            <div class="bp-table">
                <h3 class="bp-title">
                    {ANIM_TYPE_LABELS[bpTable.animType]}
                    <span class="bp-current">Current: {bpTable.currentFpa} FPA</span>
                    {#if bpTable.delta !== null}
                        <span class="bp-delta">+{bpTable.delta} to next</span>
                    {/if}
                </h3>
                <table>
                    <thead>
                        <tr>
                            <th>FPA</th>
                            <th>Required</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each bpTable.entries as entry (entry.fpa)}
                            <tr class:current={entry.fpa === bpTable.currentFpa} class:next={entry === bpTable.nextBreakpoint}>
                                <td>{entry.fpa}</td>
                                <td>{entry.requiredStat}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {:else}
            {#if speedcalcTable}
                <p class="no-data">No breakpoint data available for this combination.</p>
            {:else if !loadError}
                <p class="no-data">Loading breakpoint data...</p>
            {/if}
        {/each}
    </div>
</div>

<style>
    .breakpoints-tab {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        height: 100%;
        overflow-y: auto;
    }

    .error-banner {
        padding: var(--space-2) var(--space-3);
        background: var(--status-error-bg);
        color: var(--status-error-text);
        border-radius: var(--radius-sm);
        font-size: var(--text-sm);
    }

    .entity-toggle {
        display: flex;
        gap: var(--space-1);
    }

    .entity-btn {
        padding: var(--space-1) var(--space-3);
        border: 1px solid var(--border-primary);
        background: var(--bg-secondary);
        color: var(--text-secondary);
        border-radius: var(--radius-sm);
        cursor: pointer;
        font-size: var(--text-sm);
    }

    .entity-btn.active {
        background: var(--accent-primary);
        color: var(--accent-text);
        border-color: var(--accent-primary);
    }

    .controls {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
        padding: var(--space-2) var(--space-3);
        background: var(--bg-secondary);
        border-radius: var(--radius-md);
        border: 1px solid var(--border-primary);
    }

    .control-row {
        display: flex;
        gap: var(--space-3);
        flex-wrap: wrap;
        align-items: end;
    }

    .control-row label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        font-size: var(--text-sm);
    }

    .control-row .label {
        font-size: var(--text-xs);
        color: var(--text-muted);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    .control-row select,
    .control-row input[type="number"] {
        padding: var(--space-1) var(--space-2);
        background: var(--bg-primary);
        border: 1px solid var(--border-primary);
        border-radius: var(--radius-sm);
        color: var(--text-primary);
        font-size: var(--text-sm);
        font-family: var(--font-mono);
        min-width: 80px;
    }

    .control-row input[type="number"] {
        width: 70px;
    }

    .checkbox-label {
        flex-direction: row !important;
        align-items: center !important;
        gap: var(--space-1) !important;
    }

    .current-status {
        display: flex;
        gap: var(--space-3);
        align-items: center;
        font-size: var(--text-sm);
        color: var(--text-muted);
    }

    .status-class {
        font-weight: 600;
        color: var(--text-primary);
    }

    .tables-container {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
        gap: var(--space-3);
        flex: 1;
        min-height: 0;
        overflow-y: auto;
    }

    .bp-table {
        background: var(--bg-secondary);
        border: 1px solid var(--border-primary);
        border-radius: var(--radius-md);
        padding: var(--space-2);
    }

    .bp-title {
        font-size: var(--text-sm);
        font-weight: 600;
        color: var(--text-primary);
        margin: 0 0 var(--space-2) 0;
        display: flex;
        align-items: center;
        gap: var(--space-2);
        flex-wrap: wrap;
    }

    .bp-current {
        font-size: var(--text-xs);
        color: var(--accent-primary);
        font-weight: 500;
    }

    .bp-delta {
        font-size: var(--text-xs);
        color: var(--status-warning-text);
        font-weight: 500;
    }

    table {
        width: 100%;
        border-collapse: collapse;
        font-size: var(--text-xs);
        font-family: var(--font-mono);
    }

    th {
        text-align: left;
        padding: var(--space-1);
        color: var(--text-muted);
        border-bottom: 1px solid var(--border-primary);
        font-weight: 500;
    }

    td {
        padding: var(--space-1);
        color: var(--text-secondary);
    }

    tr.current {
        background: var(--accent-primary-subtle, rgba(99, 102, 241, 0.1));
    }

    tr.current td {
        color: var(--accent-primary);
        font-weight: 600;
    }

    tr.next td {
        color: var(--status-warning-text);
    }

    .no-data {
        color: var(--text-muted);
        font-size: var(--text-sm);
        text-align: center;
        padding: var(--space-4);
    }
</style>
```

- [ ] **Step 2: Commit**

```
feat: add BreakpointsTab component with live updates and manual controls
```

---

## Task 8: Wire Tab Into MainWindow — DONE

**Files:**
- Modify: `src/views/MainWindow.svelte`
- Modify: `src/views/index.ts`

- [ ] **Step 1: Export BreakpointsTab from views/index.ts**

Add to `src/views/index.ts`:

```typescript
export { default as BreakpointsTab } from './BreakpointsTab.svelte';
```

- [ ] **Step 2: Add tab to MainWindow**

In `src/views/MainWindow.svelte`, update the import:

```typescript
import { GeneralTab, LootFilterTab, NotificationsTab, BreakpointsTab } from "./index";
```

Add to the `tabs` array:

```typescript
const tabs = [
    { id: "general", label: "General" },
    { id: "lootfilter", label: "Loot Filter" },
    { id: "notifications", label: "Notifications" },
    { id: "breakpoints", label: "Breakpoints" },
];
```

Add the route in the `{#snippet children(tab)}` block:

```svelte
{:else if tab === "breakpoints"}
    <BreakpointsTab />
```

- [ ] **Step 3: Verify it compiles**

Run: `pnpm tauri dev`

Expected: app launches, new "Breakpoints" tab appears, shows either loading state or data (depending on whether D2 is running and SpeedcalcData.txt is cached).

- [ ] **Step 4: Commit**

```
feat: wire BreakpointsTab into MainWindow as fourth tab
```

---

## Task 9: Fix equipped weapon detection (D2MOO grid path) — DONE 2026-05-04

**Files:**
- Modify: `src-tauri/src/offsets.rs`
- Modify: `src-tauri/src/breakpoints.rs`

- [ ] **Step 1: Update `offsets.rs::inventory`**

Remove stale: `WEAPON_SWITCH`, `BODY_SLOTS_START`, `BODY_SLOT_STRIDE`. Add `GRIDS = 0x14`. Add new sibling modules `inventory_grid` (`PP_ITEMS = 0x0C`, `SIZE = 0x10`) and `body_loc` (`RARM = 4`, plus the rest of the enum for documentation). Reference D2MOO `D2InventoryStrc` / `D2InventoryGridStrc` / `D2C_PlayerBodyLocs` in comments.

- [ ] **Step 2: Rewrite `read_equipped_weapon` in `breakpoints.rs`**

Replace the body-slot iteration loop with the 3-deref chain:

```rust
// pInventory->pGrids[INVGRID_BODYLOC=0]->ppItems[BODYLOC_RARM=4].
// The engine physically moves the active weapon into BODYLOC_RARM on
// weapon switch, so we never need BODYLOC_SWRARM.
let p_grids = read::<u32>(p_inventory + inventory::GRIDS)?;
let pp_items = read::<u32>(p_grids + inventory_grid::PP_ITEMS)?;
let p_weapon = read::<u32>(pp_items + body_loc::RARM * 4)?;
let file_index = read::<u32>(p_weapon + unit::CLASS)?;
read_weapon_fields_from_items_txt(ctx, file_index as usize)
```

Remove `TICK_COUNTER`, throttled `weapon_found` / `weapon_not_found` logging, `BODY_LOC_RIGHT_ARM` / `BODY_LOC_RIGHT_ARM_SWAP` locals, `find_item_by_id` helper.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo check`

- [ ] **Step 4: Manual smoke test**

Launch app + game. Open Breakpoints tab. Confirm Weapon Type dropdown reflects the actually equipped weapon. Press W to swap weapons → confirm dropdown updates.

---

## Task 10: Weapon Base catalog backend — DONE 2026-05-05

Implementation diverged slightly from this task's original step list — the actual code path is what's in `weapon_families.rs` + `main.rs` scanner-attach hook. Notable choices:
- Backend ships **family chains** (`Vec<String>` from `equiv1` walk), not pre-grouped families. Frontend handles grouping using `WEAPON_TYPES`. Single source of truth.
- ItemTypes name strings (display names) are NOT read — frontend uses its own family display names from `WEAPON_TYPES`. Item base names ARE read via `D2Lang.GetStringById`.
- New offsets: `data_tables::ITEM_TYPES_TXT_PTR = 0xBF8`, `ITEM_TYPES_TXT_COUNT = 0xBFC`, `items_txt::TYPE_0 = 0x11E`, `TYPE_1 = 0x120`, `item_types_txt::CODE = 0x00`. (Old `WEAPON_TYPE` const removed/renamed.)
- Helpers `resolve_item_type_chain` and `u32_to_packed_code` live in `breakpoints.rs` as `pub(crate)` and are reused by `weapon_families.rs`.

Original steps (kept for reference):

**Files:**
- Create: `src-tauri/src/weapon_families.rs`
- Modify: `src-tauri/src/main.rs` — module declaration, AppState field, Tauri commands, scanner-attach hook
- Modify: `src-tauri/src/offsets.rs` — possibly add `items_txt::TYPE` and `item_types_txt::CODE` / `NAME_STR_ID` if not yet present

- [ ] **Step 1: Discover items.txt `type` field offset and ItemTypes.txt `name`/`code` offsets**

We already know `WCLASS @ 0xC0`, `SPEED @ 0xD8`, `WEAPON_TYPE @ 0x11E`, `NAME_ID @ 0xF4`. We still need:
- items.txt offset for the `type` column (4-char ItemTypes code, e.g. `"swor"`) — required to group records into families.
- ItemTypes.txt offset for the family display name (string ID resolvable via `D2Lang.GetStringById`) and/or its 4-char `code`.

Verify via a CE Lua script (similar to `verify-wsm-offset.lua`) — pick a known weapon (e.g. file index 627 = a 1HS sword), dump 0x40 bytes around suspected offsets, compare to expected ItemTypes code. Save script under `docs/ce-scripts/`.

- [ ] **Step 2: Implement `weapon_families.rs`**

```rust
struct WeaponBase { name: String, wsm: i32, file_index: u32 }
struct WeaponFamily { token: String, name: String, wclass: String, weapon_type_index: u16, is_throwing: bool, bases: Vec<WeaponBase> }
type WeaponFamilyTable = Vec<WeaponFamily>;
```

Walk items.txt records: skip non-weapons (`wclass == 0`), resolve item base name via `D2Lang.GetStringById`, group records by ItemTypes type code, build the table. Cache to `app_data_dir/weapon-families.json` (mirroring `speedcalc_data.rs` patterns).

- [ ] **Step 3: Wire into scanner attach**

When the scanner attaches to D2 (build of `class_cache` / items dictionary), build the weapon-family catalog once. Store in `AppState::weapon_families: Arc<RwLock<Option<WeaponFamilyTable>>>`. Save to disk for offline launches.

- [ ] **Step 4: Tauri commands**

```rust
#[tauri::command] fn get_weapon_families(...) -> Option<WeaponFamilyTable>
#[tauri::command] fn refresh_weapon_families(...) -> Result<(), String>
```

Load from cache on startup (mirroring `speedcalc_data` pattern from Task 4).

- [ ] **Step 5: Extend `BreakpointData`**

Include the equipped weapon's items.txt file index (or its base name) so the frontend can match it against the catalog and pre-select the correct base.

- [ ] **Step 6: Verify it compiles**

`cd src-tauri && cargo check`

---

## Task 11: Weapon Base catalog frontend — DONE 2026-05-05

Implemented in `BreakpointsTab.svelte` + `breakpoint-constants.ts`. Notable choices vs. original step list:
- **Tier suffixes are stripped** from base names (`(1)`, `(2)`, `(3)`, `(4)`, `(Sacred)`, `(Angelic)`, `(Mastercrafted)`). All tiers of the same base collapse into one dropdown entry — WSM is identical across tiers anyway. Regex: `\s*\((?:[1-4]|Sacred|Angelic|Mastercrafted)\)\s*$/i`.
- **WSM is NOT shown** in the dropdown text or status line. Internally bases are sorted fastest-first by WSM (gets the snappier weapons up top), but the number is noise to the user.
- **Throwing penalty NOT reintroduced** in this pass. Step 3 is deferred — would need the catalog to flag throwing families. Currently no UI distinction; the `-30` penalty is still missing from the formula. Tag for next session.
- Live-match handles tiered variants: looks up `live.file_index` in the FULL catalog, strips tier, then matches stripped name against the dedup'd `availableBases`.

Original steps (kept for reference):

**Files:**
- Modify: `src/lib/breakpoint-constants.ts` — replace static `WEAPON_TYPES` with dynamic data shape; add `WeaponBase`/`WeaponFamily` types
- Modify: `src/views/BreakpointsTab.svelte` — add Weapon Base select; remove last vestiges of WSM from manual flow
- Modify: `src/lib/breakpoint-calc.ts` if `isThrowing` is reintroduced (a `throwPenalty` field on `CalcParams`)

- [ ] **Step 1: Load weapon families from backend**

In `BreakpointsTab.svelte` `onMount`, `invoke<WeaponFamilyTable>("get_weapon_families")`. Persist into a `$state` variable. If empty, fall back to refresh + retry, mirroring how `speedcalc_data` is loaded.

- [ ] **Step 2: Two-level select**

`Weapon Type` select drives a derived `Weapon Base` select. Selecting a base sets `wsm` and (re-)flags throwing. Live data prefills both selects: equipped item's family from its items.txt category, base from its file index.

- [ ] **Step 3: Reintroduce throw penalty (only if needed)**

If the catalog tags throwing families (Javelins / Throwing Knives / Throwing Axes / Amazon Javelins), restore the `-30` penalty inside `calcAttackFpa`. Drive it from the selected family flag, not a UI checkbox.

- [ ] **Step 4: Manual smoke test**

Launch app + game with several weapons. Confirm that:
- Equipped weapon → correct Type + Base autoselected.
- Manually switching Base updates breakpoint tables (WSM changes).
- Switching weapons in-game refreshes the prefill (unless user has manually overridden).

---

## Task 12: End-to-End Testing

- [ ] **Step 1: Test offline mode**

1. Launch app without D2 running
2. Navigate to Breakpoints tab
3. Select a class (e.g., Amazon), weapon type (e.g., Bow), enter IAS manually
4. Verify breakpoint tables appear with correct values
5. Cross-check one value against the online calculator at https://dev.median-xl.com/speedcalc/

- [ ] **Step 2: Test live mode**

1. Launch D2 with a character
2. Launch app, wait for scanner to attach
3. Navigate to Breakpoints tab
4. Verify class and weapon are auto-detected
5. Verify IAS/FCR/FHR/FBR show current values from character
6. Equip a different weapon → verify values update
7. Switch to Mercenary → verify merc data shows

- [ ] **Step 3: Test polling lifecycle**

1. Navigate away from Breakpoints tab
2. Verify (via logs or process monitor) that `breakpoints-update` events stop
3. Navigate back → events resume

- [ ] **Step 4: Test overrides**

1. With live data showing, manually change class dropdown
2. Verify tables recalculate immediately with new class
3. Clear override → verify returns to live data
4. Select a debuff → verify values shift

- [ ] **Step 5: Test cache**

1. Delete `speedcalc-data.json` from app data dir
2. Launch app → navigate to Breakpoints → should fetch from site
3. Close app, disconnect internet
4. Launch app → navigate to Breakpoints → should load from cache

- [ ] **Step 6: Commit all fixes from testing**

```
fix: breakpoint calculator end-to-end fixes
```
