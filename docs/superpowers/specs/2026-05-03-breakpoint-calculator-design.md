# Breakpoint Calculator — Design Spec

## Overview

Add a "Breakpoints" tab that displays attack speed, cast speed, hit recovery, and block breakpoint tables for the player and mercenary. Data is read live from the game process when connected; all parameters can also be set manually for theorycrafting without the game.

## Requirements

### Functional

1. **Auto-read from game**: class, IAS, FCR, FHR, FBR, skill velocities, equipped weapon type, WSM — for both player and mercenary.
2. **Breakpoint tables**: for each animation type (Attack 1, Attack 2, Cast, Block, Hit Recovery), show all achievable FPA thresholds with the required stat value. Highlight the current position and show distance to next breakpoint.
3. **Manual override**: user can override any auto-detected parameter (class, weapon type, morph, debuff, stat values). Overrides take precedence until cleared.
4. **Morph selection**: manual-only (no auto-detection). Dropdown with morph options relevant to the selected class.
5. **Debuff selector**: Decrepify (-20), Phoboss (-20), Uldyssian (-30), Chill (-50), or None.
6. **Offline mode**: tab fully functional without game. User picks class/weapon/morph manually, enters stat values, sees breakpoint tables.
7. **Live updates**: when tab is visible and game is connected, re-read stats every scanner tick (~30ms). Stop polling when tab is hidden.
8. **SpeedcalcData.txt**: fetch from `https://dev.median-xl.com/speedcalc/SpeedcalcData.txt` on first use, cache locally in `app_data_dir`. Refresh from site when possible.

### Non-functional

- Calculation runs on the frontend (pure math, no backend roundtrip for recalculation on manual changes).
- Backend only emits raw parameters; frontend owns the formula logic.
- Polling adds negligible overhead to existing scanner tick (6-8 `get_unit_stat` calls + a few memory reads per tick).

## Data Sources

### From Game Memory

| Data | Method | Location |
|------|--------|----------|
| Player class | `read_memory` | `UnitAny.CLASS` (offset 0x04) at `D2Client + 0x11BBFC` |
| Merc class | `read_memory` | `UnitAny.CLASS` (offset 0x04) at `D2Client + 0x10A80C` |
| IAS (items) | `inject_get_unit_stat` | stat ID 93 |
| FCR (items) | `inject_get_unit_stat` | stat ID 105 |
| FHR (items) | `inject_get_unit_stat` | stat ID 99 |
| FBR (items) | `inject_get_unit_stat` | stat ID 102 |
| Skill attack speed | `inject_get_unit_stat` | stat ID 68 |
| Skill FHR/FBR | `inject_get_unit_stat` | stat ID 69 |
| Equipped weapon index | `read_memory` chain | `unit → Inventory (0x60) → WEAPON_ID (0x1C)` → find item → `ItemData.FILE_INDEX` (0x2C) |
| Weapon type | `read_memory` | `items.txt[index].type` (offset TBD — Step 0) |
| WSM | `read_memory` | `items.txt[index].speed` (offset TBD — Step 0) |

### From Website (cached)

- `SpeedcalcData.txt` — TSV with columns: CofName, FramesPerDirection, AnimationSpeed
- CofName format: `{ClassToken}{AnimType}{WeaponType}` (e.g., `AMA11HS`)
- ~280 records covering all class/animation/weapon combinations

### Bundled Constants

- Weapon type code → animation token mapping (~15 entries: e.g., `sword → 1HS`, `bow → BOW`)
- WSM per weapon base for offline mode (~30 entries)
- Class list with tokens: Amazon (AM), Sorceress (SO), Necromancer (NE), Paladin (PA), Barbarian (BA), Druid (DZ), Assassin (AI)
- Morph list with tokens: Werewolf (40), Werebear (TG), Wereowl (OW), Superbeast (~Z), Deathlord (0N), Treewarden (TH)
- Mercenary list: Rogue (RG), Town Guard (GU), Iron Wolf (IW), Son of Harrogath (0A)
- Debuff list: None (0), Decrepify (-20), Phoboss (-20), Uldyssian (-30), Chill (-50)
- Throwing flag: applies -30 penalty to attack speed calculation

## Calculation Formulas

All formulas use integer arithmetic (floor/ceil).

### Effective Speed (diminishing returns)

```
eSpeed = floor(120 * speed / (120 + speed))
```

### Attack Speed

```
eIAS = floor(120 * IAS / (120 + IAS))
effective = min(eIAS - WSM + SkillSlow, 75)
FPA = ceil(256 * FramesPerDirection / floor(AnimationSpeed * (100 + effective) / 100)) - 1
```

Notes:
- Amazon and Sorceress with melee/staff weapons use `FramesPerDirection - 2` (StartingFrame adjustment).
- Throwing weapons apply a -30 penalty: `effective = min(eIAS - WSM + SkillSlow - 30, 75)`.

### Wereform Attack (morphs)

When a morph is selected, attack speed uses a two-stage formula:

```
// Stage 1: compute effective AnimationSpeed from neutral stance
wAnimSpeed = floor(256 * NeutralFrames / floor(256 * BaseFrames / floor((100 + wIAS - WSM) * BaseAnimSpeed / 100)))

// Stage 2: standard attack formula with modified AnimationSpeed
FPA = ceil(256 * FramesPerDirection / floor(wAnimSpeed * (100 + effective) / 100)) - 1
```

Where `NeutralFrames` / `BaseAnimSpeed` come from the morph's `NU` + `HTH` entry in SpeedcalcData.txt, and `BaseFrames` / animation from the morph's `A1` + `HTH` entry.

### Cast Speed

```
eFCR = min(floor(120 * FCR / (120 + FCR)) + SkillSlow, 75)
FPA = ceil(256 * FramesPerDirection / floor(AnimationSpeed * (100 + eFCR) / 100)) - 1
```

### Hit Recovery

```
eFHR = floor(120 * FHR / (120 + FHR))
FPA = ceil(256 * FramesPerDirection / floor(AnimationSpeed * (50 + eFHR + SkillSlow) / 100)) - 1
```

### Block

```
eFBR = floor(120 * FBR / (120 + FBR))
FPA = ceil(256 * FramesPerDirection / floor(AnimationSpeed * (50 + eFBR + SkillSlow) / 100)) - 1
```

## Data Flow

```
[Scanner Tick] → reads stats via injection/memory
       ↓
[breakpoints-update event] → { player: BreakpointData, merc: BreakpointData }
       ↓
[Frontend receives] → applies manual overrides → runs formulas → renders tables
```

### Polling Lifecycle

1. User opens Breakpoints tab → frontend calls `set_breakpoints_polling(true)`
2. Scanner loop includes breakpoint data reads each tick → emits `breakpoints-update`
3. User leaves tab → frontend calls `set_breakpoints_polling(false)` → scanner skips reads

### Offline Mode

- No events emitted (game not connected)
- User fills all parameters manually
- Frontend calculates from manual inputs + cached SpeedcalcData.txt

## Step 0: Offset Verification

Before implementation, verify/find these items.txt field offsets:

1. **`speed` (WSM)** — the Weapon Speed Modifier value. Known to exist in items.txt records (size 0x1A8). Must find exact byte offset within the record.
2. **`type` / `type2`** — weapon type code that determines animation class. D2Stats.au3 references `weaponType` at offset 0x11E — needs verification against current MXL version.

Methods: Cheat Engine memory comparison of known weapons (e.g., Short Sword WSM=-20, Great Maul WSM=+23), or cross-reference with D2MOO/d2mods documentation.

## UI Structure

### BreakpointsTab layout

- **Controls section**: class selector, morph selector, weapon type selector, debuff selector
- **Stats display**: current IAS/FCR/FHR/FBR values (auto-read or manual input)
- **Tables section**: one table per animation type showing all FPA breakpoints
- **Player / Mercenary toggle or side-by-side**: show data for both entities

### Breakpoint Table Format

Each table (e.g., "Attack 1") shows:
- Column: required stat value to reach each FPA threshold
- Current position highlighted
- Delta to next breakpoint shown prominently

## Tauri Commands

- `set_breakpoints_polling(enabled: bool)` — toggle stat reading in scanner loop
- `get_speedcalc_data() → SpeedcalcTable` — return cached/fetched animation data
- `refresh_speedcalc_data()` — force re-fetch from website

## Events

- `breakpoints-update` → `{ player: Option<BreakpointData>, merc: Option<BreakpointData> }`

## BreakpointData Structure

```
BreakpointData {
    class: u32,           // 0-6 for player classes, merc type for mercs
    weapon_type: String,  // animation token: "1HS", "BOW", "STF", etc.
    wsm: i32,            // weapon speed modifier (can be negative)
    ias: u32,            // IAS from items (stat 93)
    fcr: u32,            // FCR from items (stat 105)
    fhr: u32,            // FHR from items (stat 99)
    fbr: u32,            // FBR from items (stat 102)
    skill_ias: u32,      // skill attack speed (stat 68)
    skill_fhr: u32,      // skill FHR/FBR velocity (stat 69)
}
```
