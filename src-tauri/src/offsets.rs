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

    /// Injection function offsets (relative to INJECT_BASE)
    pub mod inject {
        pub const PRINT: usize = 0x01;
        pub const GET_STRING: usize = 0x11;
        pub const GET_ITEM_NAME: usize = 0x21;
        pub const GET_ITEM_STAT: usize = 0x3E;
        /// 6-byte stub: `call NewAutomapCell; ret`. EAX on return = AutomapCell*.
        /// Placed well past INJECT_GET_UNIT_STAT (`0x54` + ~17 bytes) with pad.
        pub const NEW_AUTOMAP_CELL: usize = 0x70;
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
}

/// Field offsets inside `D2DataTablesStrc` (the struct pointed to by
/// `sgptDataTables`). Values taken from D2MOO `D2DataTbls.h`, confirmed
/// against MedianXL 1.13c in live memory: uniques count=1822 and
/// set-items count=330 with localized names resolving correctly via
/// `wTblIndex` / `wStringId`. See `docs/item-tables-memory.md`.
pub mod data_tables {
    pub const SETS_TXT_PTR: usize = 0xC0C;
    pub const SETS_TXT_COUNT: usize = 0xC10;
    pub const SET_ITEMS_TXT_PTR: usize = 0xC18;
    pub const SET_ITEMS_TXT_COUNT: usize = 0xC1C;
    pub const UNIQUE_ITEMS_TXT_PTR: usize = 0xC24;
    pub const UNIQUE_ITEMS_TXT_COUNT: usize = 0xC28;
}

/// D2Sigma.dll offsets (Median XL specific)
pub mod d2sigma {
    /// Dereference once → struct holding the toggle. NULL when not in game.
    pub const ALWAYS_SHOW_ITEMS_PTR: usize = 0x692D8C;
    /// 0 = off, non-zero = on.
    pub const ALWAYS_SHOW_ITEMS_FLAG: usize = 0x24;
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
    pub const UNIT_DATA: usize = 0x14; // dword (pointer to type-specific data)
    pub const PATH: usize = 0x2C; // dword (pointer to Path/Path2/static path)
    pub const INVENTORY: usize = 0x60; // dword (pointer to inventory)
    pub const NEXT_UNIT: usize = 0xE4; // pListNext — walks the game-wide hash-table bucket chain
    /// pRoomNext — walks the unit list belonging to a single `Room1`.
    /// Distinct from `NEXT_UNIT (0xE4, pListNext)`, which leaves the room.
    /// Use this when iterating ground items inside a room.
    pub const ROOM_NEXT: usize = 0xE8;
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
    pub const FILE_INDEX: usize = 0x2C; // dword - offset 0x18 + 4 + 3*4 + 4 = 0x2C
    pub const NEXT_ITEM: usize = 0x64; // dword (pointer to next item)
}

/// Inventory structure offsets
pub mod inventory {
    pub const FIRST_ITEM: usize = 0x0C; // dword
    pub const WEAPON_ID: usize = 0x1C; // dword
}

/// Items.txt record offsets (record size = 0x1A8)
pub mod items_txt {
    pub const RECORD_SIZE: usize = 0x1A8;

    pub const MISC: usize = 0x84; // dword
    pub const NAME_ID: usize = 0xF4; // word
    pub const STR_BONUS: usize = 0x106; // word
    pub const DEX_BONUS: usize = 0x108; // word
    pub const IS_2H: usize = 0x11C; // byte
    pub const WEAPON_TYPE: usize = 0x11E; // word
    pub const IS_1H: usize = 0x13D; // byte
}

/// ItemTypes.txt record offsets (record size = 0xE4)
pub mod item_types_txt {
    pub const RECORD_SIZE: usize = 0xE4;
    pub const EQUIV1: usize = 0x04; // word
    pub const EQUIV2: usize = 0x06; // word
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
