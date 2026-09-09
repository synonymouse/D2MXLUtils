//! D2 memory offsets relative to DLL base addresses
//! Based on D2Stats.au3 from MedianXL

/// D2Client.dll offsets
pub mod d2client {
    /// Player unit pointer (also used for IsIngame check: != 0 means in game)
    pub const PLAYER_UNIT: usize = 0x11BBFC;

    /// Mercenary unit pointer
    pub const MERCENARY_UNIT: usize = 0x10A80C;

    /// No-pickup flag (byte)
    pub const NO_PICKUP_FLAG: usize = 0x11C2F0;

    /// Base address for code injection area
    pub const INJECT_BASE: usize = 0xCDE00;

    /// Pointer to the current `AutomapLayer` (dword → AutomapLayer*). NULL
    /// outside of gameplay (loading screens, main menu). See
    /// `docs/map-marker-reverse-engineering.md`.
    pub const AUTOMAP_LAYER: usize = 0x11C1C4;

    /// Current game difficulty as `u32`: `0=Normal, 1=Nightmare, 2=Hell`.
    /// Used by the DPS-meter trampoline to index `MonStats.wMaxHP[diff]`.
    pub const DIFFICULTY: usize = 0x11C390;

    /// Injection function offsets (relative to INJECT_BASE)
    pub mod inject {
        pub const PRINT: usize = 0x01;
        pub const GET_STRING: usize = 0x11;
        pub const GET_ITEM_NAME: usize = 0x21;
        pub const GET_ITEM_STAT: usize = 0x3E;
        /// 6-byte stub: `call NewAutomapCell; ret`. EAX on return = AutomapCell*.
        /// Placed well past INJECT_GET_UNIT_STAT (`0x54` + ~17 bytes) with pad.
        pub const NEW_AUTOMAP_CELL: usize = 0x70;

        /// Linux only: staging address for a small hand-written `mmap2`
        /// syscall stub (~40 bytes), used once per `D2Injector::new` to
        /// allocate real `string_buffer`/`params_buffer` scratch pages —
        /// the Linux analog of `VirtualAllocEx`. Empirically confirmed free
        /// (all-zero for the surrounding 512 bytes on a live process) —
        /// see `process.rs`'s `live_probe::find_free_padding_runs` test.
        /// Placed comfortably past `NEW_AUTOMAP_CELL`'s 6-byte stub (ends
        /// `+0x76`).
        pub const LINUX_MMAP_STUB: usize = 0x90;
    }

    /// Internal D2Client functions
    pub mod func {
        /// PrintStringToChat function
        pub const PRINT_STRING: usize = 0x7D850;
        /// GetItemName internal
        pub const GET_ITEM_NAME: usize = 0x914F0;
        /// GetItemStats internal
        pub const GET_ITEM_STAT: usize = 0x560B0;
        /// `AutomapCell* __fastcall NewAutomapCell(void)` — pool alloc, cell
        /// returned uninitialized. Do NOT call AddAutomapCell (0x61320): it
        /// crashes when invoked from a remote thread in our setup.
        pub const NEW_AUTOMAP_CELL: usize = 0x5F6B0;
    }
}

/// D2Common.dll offsets
pub mod d2common {
    /// Number of records in Items.txt (dword, immediately before ITEMS_TXT pointer)
    pub const ITEMS_TXT_COUNT: usize = 0x9FB94;

    /// Pointer to Items.txt data
    pub const ITEMS_TXT: usize = 0x9FB98;

    /// Pointer to `D2DataTablesStrc` (D2MOO naming: `sgptDataTables`).
    /// Dereference once to get the base of the struct that holds pointers
    /// to all .txt tables. Same as `$g_pD2sgpt` in D2Stats.au3:259.
    pub const SGPT_DATA_TABLES: usize = 0x99E1C;

    /// GetUnitStat function
    pub const GET_UNIT_STAT: usize = 0x38B70;

    /// D2Common_GetUnitStat injection offset (relative to D2Client inject base)
    pub const INJECT_GET_UNIT_STAT: usize = 0x54;

    /// `STATLIST_SetUnitStat` (D2Common ordinal 10887). Universal sink
    /// for stat writes in both SP and MP — DPS-meter inline-hook target.
    /// `__stdcall(pUnit, statId, value, layer)`, `ret 0010`.
    pub const STATLIST_SET_UNIT_STAT: usize = 0x3A740;

    /// `STATLIST_SetStat` (D2Common ordinal 10261). Leaf called by
    /// `STATLIST_SetUnitStat`. Kept as a reference; not currently hooked.
    pub const STATLIST_SET_STAT: usize = 0x3A280;
}

/// Field offsets inside `D2DataTablesStrc` (the struct pointed to by
/// `sgptDataTables`). Values taken from D2MOO `D2DataTbls.h`, confirmed
/// against MedianXL 1.13c in live memory: uniques count=1822 and
/// set-items count=330 with localized names resolving correctly via
/// `wTblIndex` / `wStringId`. See `docs/item-tables-memory.md`.
pub mod data_tables {
    /// `D2MonStatsTxt* pMonStatsTxt` — MonStats.txt records. See
    /// `docs/dps-meter-reverse-engineering.md`. **Layout quirk**: count
    /// is at `+8`, not `+4`, because `pMonStats2Txt` (cosmetic paired
    /// table) sits at `+4` and shares the count.
    pub const MONSTATS_TXT_PTR: usize = 0xA78;
    pub const MONSTATS_TXT_COUNT: usize = 0xA80;

    /// `D2ItemTypesTxt* pItemTypesTxt` — ItemTypes.txt records (D2MOO field
    /// at this offset; verified live against MXL with count=346).
    pub const ITEM_TYPES_TXT_PTR: usize = 0xBF8;
    pub const ITEM_TYPES_TXT_COUNT: usize = 0xBFC;
    pub const SETS_TXT_PTR: usize = 0xC0C;
    pub const SETS_TXT_COUNT: usize = 0xC10;
    pub const SET_ITEMS_TXT_PTR: usize = 0xC18;
    pub const SET_ITEMS_TXT_COUNT: usize = 0xC1C;
    pub const UNIQUE_ITEMS_TXT_PTR: usize = 0xC24;
    pub const UNIQUE_ITEMS_TXT_COUNT: usize = 0xC28;
}

/// D2Sigma.dll offsets (Median XL specific)
pub mod d2sigma {
    /// Bool offset inside the always-show-items host struct. The struct
    /// pointer itself drifts between MXL patches and is resolved at runtime
    /// by `process::resolve_always_show_items_ptr_rva`.
    pub const ALWAYS_SHOW_ITEMS_FLAG: usize = 0x24;

    /// Native tooltip-builder hook. On function entry `[ESP+04]` is the
    /// current tooltip item `UnitAny*`. Used as the primary item-search hover
    /// identity source; see `docs/mxl-item-search-hover-re-session-report.md`.
    ///
    /// Relocated for MXL 2.14: the function itself is byte-for-byte
    /// unchanged (confirmed by diffing the pre-/post-2.14 `D2Sigma.dll`,
    /// matching 119/128 bytes at the new offset, all mismatches being
    /// embedded data-pointer immediates that shifted because `.rdata`
    /// shrank in this build) — only its position in `.text` moved, from
    /// `0xAE020` to `0xB4F80`.
    pub const TOOLTIP_ITEM_HOOK: usize = 0xB4F80;
    pub const TOOLTIP_ITEM_HOOK_RESUME: usize = 0xB4F85;
    pub const TOOLTIP_ITEM_HOOK_PATCH_SIZE: usize = 5;
    pub const TOOLTIP_ITEM_HOOK_PROLOGUE: [u8; TOOLTIP_ITEM_HOOK_PATCH_SIZE] =
        [0x55, 0x8D, 0x6C, 0x24, 0xD8];
    pub const TOOLTIP_ITEM_ARG_STACK_OFFSET: usize = 0x04;
    /// If the trampoline starts with `pushfd; pushad`, the original `[ESP+04]`
    /// argument is readable at `[ESP+28]` until registers/flags are restored.
    pub const TOOLTIP_ITEM_ARG_AFTER_PUSHFD_PUSHAD: usize = 0x28;

    /// Absolute low-memory range where RE found the native UTF-16 tooltip text
    /// buffer. It can remain stale on empty hover, so do not use it as the
    /// primary hovered-item identity source; use the D2Sigma+AE020 pUnit hook.
    pub const TOOLTIP_TEXT_BUFFER_START: usize = 0x00194080;
    pub const TOOLTIP_TEXT_BUFFER_END: usize = 0x00194280;
}

/// D2Lang.dll offsets
pub mod d2lang {
    /// GetStringById function — resolves a string-table ID to a wchar pointer.
    /// Calling convention: ECX = iNameID, returns EAX = *const u16
    pub const GET_STRING_BY_ID: usize = 0x9450;
}

/// Path/Room iteration offsets for finding ground items.
///
/// Chain: `pPlayer → +0x2C (pPath) → +0x1C (pRoom1) → +0x00 (ppRoomsNear)` —
/// step `[2] = 0x1C` is the same `pRoom1` link that the automap BFS
/// (`room1::*`) uses.
pub mod paths {
    /// Offsets to reach pPaths: [0, 0x2C, 0x1C, 0x0]
    pub const TO_PATHS_PTR: [usize; 4] = [0x00, 0x2C, 0x1C, 0x00];
    /// Offsets to reach paths count: [0, 0x2C, 0x1C, 0x24]
    pub const TO_PATHS_COUNT: [usize; 4] = [0x00, 0x2C, 0x1C, 0x24];

    /// Offset from pPath to pUnit (first unit in path)
    pub const PATH_TO_UNIT: usize = 0x74;
}

/// UnitAny structure offsets
pub mod unit {
    pub const UNIT_TYPE: usize = 0x00; // dword
    pub const CLASS: usize = 0x04; // dword
    pub const UNIT_ID: usize = 0x0C; // dword
    /// `dwMode` — current unit-mode token. For monsters, see `mon_mode::*`.
    /// DPS trampoline drops stat-6 writes on already-dead monsters
    /// (corpse visibility refresh, area reload).
    pub const MODE: usize = 0x10; // dword
    pub const UNIT_DATA: usize = 0x14; // dword (pointer to type-specific data)
    pub const PATH: usize = 0x2C; // dword (pointer to Path/Path2/static path)
    pub const INVENTORY: usize = 0x60; // dword (pointer to inventory)
    pub const NEXT_UNIT: usize = 0xE4; // pListNext — walks the game-wide hash-table bucket chain
    /// pRoomNext — walks the unit list belonging to a single `Room1`.
    /// Distinct from `NEXT_UNIT (0xE4, pListNext)`, which leaves the room.
    /// Use this when iterating ground items inside a room.
    pub const ROOM_NEXT: usize = 0xE8;
}

/// `D2C_MonsterModes` enum (from D2MOO `D2DataDefs.h`).
#[allow(dead_code)]
pub mod mon_mode {
    pub const DEATH: u32 = 0;
    pub const NEUTRAL: u32 = 1;
    pub const DEAD: u32 = 12;
}

/// `Room1` field offsets used by the automap-marker BFS.
pub mod room1 {
    /// `Room1** ppRoomsNear` — array of neighbouring Room1 pointers.
    pub const PP_ROOMS_NEAR: usize = 0x00;
    /// `u32 dwRoomsNear` — length of the `ppRoomsNear` array.
    pub const DW_ROOMS_NEAR: usize = 0x24;
    /// Head of the mixed-type unit linked list. Walk it via `unit::ROOM_NEXT`.
    pub const UNIT_FIRST: usize = 0x74;
}

/// Chain `pRoom1 → +0x10 → +0x24 → +0x100 = dwLevelNo`. Unreliable in
/// motion — Room1 changes between rendering rooms and the intermediate
/// pointer leads to neighbour `Level` structs. DPS meter uses
/// `*pAutomapLayer` instead. Kept for a future RE pass.
#[allow(dead_code)]
pub mod level_chain {
    pub const ROOM1_TO_INTERMEDIATE: usize = 0x10;
    pub const INTERMEDIATE_TO_LEVEL: usize = 0x24;
    pub const LEVEL_TO_LEVEL_NO: usize = 0x100;
}

/// `AutomapLayer` field offsets (at `*pAutomapLayer`).
///
/// **Only `P_OBJECTS` is safe to mutate.** Touching `P_FLOORS` / `P_WALLS`
/// corrupts revealed terrain.
pub mod automap_layer {
    pub const P_FLOORS: usize = 0x08; // read-only
    pub const P_WALLS: usize = 0x0C; // read-only
    pub const P_OBJECTS: usize = 0x10; // BST root for icon cells — OK to splice
}

/// `AutomapCell` field offsets (20-byte struct). Calibrated against live
/// 1.13c MXL memory — **do not use the D2BS layout**, which has `nCellNo` at
/// `+0x02` and is wrong for this build.
pub mod automap_cell {
    pub const F_SAVED: usize = 0x00; // u32
    pub const N_CELL_NO: usize = 0x04; // u16
    pub const X_PIXEL: usize = 0x06; // u16
    pub const Y_PIXEL: usize = 0x08; // u16
    pub const W_WEIGHT: usize = 0x0A; // u16
    pub const P_LESS: usize = 0x0C; // AutomapCell*
    pub const P_MORE: usize = 0x10; // AutomapCell*
    pub const SIZE: usize = 20;
    /// Sprite id that renders as a small red cross (good default for loot).
    pub const CROSS_CELL_NO: u16 = 300;
}

/// Static-path layout for **items**. NB: player and item paths share the
/// `UnitAny + 0x2C` pointer but have different struct shapes (player is a
/// dynamic path with fixed-point fields, items are static with raw u32
/// subtiles).
pub mod item_path {
    pub const SUB_X: usize = 0x0C; // u32
    pub const SUB_Y: usize = 0x10; // u32
}

/// Dynamic-path layout for the **player**. Subtile coordinates are the upper
/// word of fixed-point xPos/yPos fields, read as u16.
pub mod player_path {
    pub const SUB_X: usize = 0x02; // u16 (upper word of fixed-point xPos @ +0x00)
    pub const SUB_Y: usize = 0x06; // u16 (upper word of fixed-point yPos @ +0x04)
}

/// ItemData structure offsets (pUnitData for items)
pub mod item_data {
    pub const QUALITY: usize = 0x00; // dword (item quality enum)
    /// `dwSeed` — random seed used to generate the item. Effectively a
    /// stable per-item identifier: persists across area unload/reload
    /// (and in MP across log-out, since the server keeps it). Used by
    /// the loot-history layer to deduplicate the same physical item when
    /// the player leaves an area, returns, and the engine assigns a
    /// fresh `unit_id`.
    pub const SEED: usize = 0x14; // dword
    pub const FLAGS: usize = 0x18; // dword (item flags) - offset 0 + 4 + 5*4 = 0x18
    pub const FILE_INDEX: usize = 0x28; // dword (dwFileIndex)
    pub const ITEM_LEVEL: usize = 0x2C; // dword (dwItemLevel)
    pub const BODY_LOCATION: usize = 0x44; // byte (equipped body slot)
    pub const ITEM_LOCATION: usize = 0x45; // byte (inventory/equipment location enum)
    pub const OWNER_INVENTORY: usize = 0x5C; // dword (owning D2InventoryStrc*)
    pub const NEXT_ITEM: usize = 0x64; // dword (pointer to next item)
    pub const GAME_LOCATION: usize = 0x68; // byte (inventory=3, cube=6, stash=7)
}

/// `D2InventoryStrc` field offsets. Layout from D2MOO
/// (`ThePhrozenKeep/D2MOO/source/D2Common/include/D2Inventory.h`); struct
/// size is `0x40`. Verified live against MXL via
/// `docs/ce-scripts/verify-equipped-weapon.lua`.
pub mod inventory {
    pub const FIRST_ITEM: usize = 0x0C; // D2UnitStrc* pFirstItem
    pub const GRIDS: usize = 0x14; // D2InventoryGridStrc* pGrids
}

/// `D2InventoryGridStrc` field offsets (sizeof = 0x10). The BodyLoc grid
/// is `pInventory->pGrids[INVGRID_BODYLOC=0]`; its `ppItems` is a
/// `D2UnitStrc**` of length 13 indexed by `body_loc::*`.
pub mod inventory_grid {
    pub const PP_ITEMS: usize = 0x0C; // D2UnitStrc** ppItems
    pub const SIZE: usize = 0x10;
}

/// `D2C_PlayerBodyLocs` enum values from D2MOO. The engine moves the
/// active weapon into `RARM` on weapon switch (W key), so reading
/// `ppItems[RARM]` always returns the currently active right-hand item.
pub mod body_loc {
    pub const NONE: usize = 0;
    pub const HEAD: usize = 1;
    pub const NECK: usize = 2;
    pub const TORSO: usize = 3;
    pub const RARM: usize = 4;
    pub const LARM: usize = 5;
    pub const RRIN: usize = 6;
    pub const LRIN: usize = 7;
    pub const BELT: usize = 8;
    pub const FEET: usize = 9;
    pub const GLOVES: usize = 10;
    pub const SWRARM: usize = 11;
    pub const SWLARM: usize = 12;
}

/// Items.txt record offsets (record size = 0x1A8)
pub mod items_txt {
    pub const RECORD_SIZE: usize = 0x1A8;

    pub const MISC: usize = 0x84; // dword
    pub const DESC_STR_ID: usize = 0xB6; // word
    pub const WCLASS: usize = 0xC0; // u32 (4-char weapon class code: "1hs", "bow", "stf", etc.)
    pub const WCLASS_2H: usize = 0xC4; // u32 (two-hand weapon class override)
    pub const SPEED: usize = 0xD8; // i32 (weapon speed modifier / WSM)
    pub const NAME_ID: usize = 0xF4; // word
    pub const STR_BONUS: usize = 0x106; // word
    pub const DEX_BONUS: usize = 0x108; // word
    pub const IS_2H: usize = 0x11C; // byte
    /// `wType[0]` — primary ItemTypes.txt index. Use this to resolve the
    /// item's family (`szCode`) — it differentiates families that share
    /// the same `wclass` (e.g. `swor` vs `axe` vs `mace` are all `1HS`).
    pub const TYPE_0: usize = 0x11E; // word
    /// `wType[1]` — secondary ItemTypes.txt index (often a tier/quality
    /// classifier, e.g. `tier`).
    pub const TYPE_1: usize = 0x120; // word
    pub const IS_1H: usize = 0x13D; // byte
}

/// `D2ItemTypesTxt` record offsets (record size = 0xE4). Layout from D2MOO
/// `D2Common/include/DataTbls/ItemsTbls.h`.
pub mod item_types_txt {
    pub const RECORD_SIZE: usize = 0xE4;
    /// `szCode[4]` — 4-char family code (e.g. `"swor"`, `"axe "`, `"knif"`).
    /// May be zero- or space-padded; trim trailing whitespace before use.
    pub const CODE: usize = 0x00;
    pub const EQUIV1: usize = 0x04; // i16
    pub const EQUIV2: usize = 0x06; // i16
}

/// UniqueItems.txt record offsets (record size = 0x14C).
/// Layout from D2MOO `D2UniqueItemsTxt` struct, confirmed against 1.13c
/// AutoIt layout (`wLvl` @ 0x34).
pub mod unique_items_txt {
    pub const RECORD_SIZE: usize = 0x14C;
    /// `wTblIndex` — string-table index for localized display name.
    /// Pass to `D2Lang::GetStringById` (same as Items.txt NAME_ID).
    /// Engine stores sentinel `5383` if the name lookup fails at load.
    pub const NAME_ID: usize = 0x22; // word
    pub const LEVEL: usize = 0x34; // word (wLvl)
    pub const LEVEL_REQ: usize = 0x36; // word (wLvlReq)
}

/// SetItems.txt record offsets (record size = 0x1B8). Individual set
/// pieces, e.g. "Sigon's Gage". Not to be confused with Sets.txt which
/// holds full-set group bonuses.
pub mod set_items_txt {
    pub const RECORD_SIZE: usize = 0x1B8;
    /// `wStringId` — string-table index, same semantics as Items.txt NAME_ID.
    pub const NAME_ID: usize = 0x24; // word
    pub const LEVEL: usize = 0x30; // word (wLvl)
    pub const LEVEL_REQ: usize = 0x32; // word (wLvlReq)
    pub const SET_ID: usize = 0x2C; // int16 — index into Sets.txt
}

/// `D2StatListEx` field offsets and `D2StatStrc` record layout. Reference
/// for the DPS-meter trampoline (which embeds these as immediates).
#[allow(dead_code)]
pub mod stat_list {
    pub const UNIT_TO_STATS_LIST: usize = 0x5C;
    pub const SL_PSTAT: usize = 0x24;
    pub const SL_STAT_COUNT: usize = 0x28;
    pub const SL_STAT_CAPACITY: usize = 0x2A;
    /// For monsters, `pStat` is allocated inline at this offset.
    pub const SL_INLINE_PSTAT: usize = 0x80;

    pub const STAT_RECORD_SIZE: usize = 8;
    pub const STAT_LAYER: usize = 0x00;
    pub const STAT_NSTAT: usize = 0x02;
    pub const STAT_VALUE: usize = 0x04;

    /// Stat ids from `ItemStatCost.txt`.
    pub const STAT_HITPOINTS: u16 = 6;
    pub const STAT_MAXHP: u16 = 7;
    /// "level" — character/monster level. Already verified live for both
    /// (see `dps_hook/trampoline.rs`'s monster-level read and
    /// `docs/dps-meter-scaling-investigation.md`'s player read, both stat 12).
    pub const STAT_LEVEL: u16 = 12;
}

/// `D2MonStatsTxt` record offsets. Record size = `0x1A8`; indexing is
/// `record_ptr = table_ptr + class_id * 0x1A8`. Reference for the
/// DPS-meter trampoline (embeds these as immediates).
#[allow(dead_code)]
pub mod monstats_txt {
    pub const RECORD_SIZE: usize = 0x1A8;
    pub const W_ID: usize = 0x00;
    /// `0` for wild monsters, `1` for summons. Trampoline filters on this.
    pub const IS_SPAWN: usize = 0x4C;

    pub const MIN_HP_NORMAL: usize = 0xAA;
    pub const MIN_HP_NM: usize = 0xAC;
    pub const MIN_HP_HELL: usize = 0xAE;

    /// `wMaxHP[3]` indexed by difficulty (Normal/NM/Hell).
    pub const MAX_HP_NORMAL: usize = 0xB0;
    pub const MAX_HP_NM: usize = 0xB2;
    pub const MAX_HP_HELL: usize = 0xB4;
}

/// Unit types enum values
pub mod unit_type {
    pub const PLAYER: u32 = 0;
    pub const MONSTER: u32 = 1;
    pub const OBJECT: u32 = 2;
    pub const MISSILE: u32 = 3;
    pub const ITEM: u32 = 4;
    pub const TILE: u32 = 5;
}

/// Item quality enum values
pub mod item_quality {
    pub const NONE: u32 = 0;
    pub const INFERIOR: u32 = 1;
    pub const NORMAL: u32 = 2;
    pub const SUPERIOR: u32 = 3;
    pub const MAGIC: u32 = 4;
    pub const SET: u32 = 5;
    pub const RARE: u32 = 6;
    pub const UNIQUE: u32 = 7;
    pub const CRAFTED: u32 = 8;
    pub const HONORIFIC: u32 = 9;
}

/// Item flags bitmask values
pub mod item_flags {
    pub const IDENTIFIED: u32 = 0x00000010;
    pub const SOCKETED: u32 = 0x00000800;
    pub const ETHEREAL: u32 = 0x00400000;
    pub const RUNEWORD: u32 = 0x04000000;
}
