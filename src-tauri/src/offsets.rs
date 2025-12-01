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
    
    /// Injection function offsets (relative to INJECT_BASE)
    pub mod inject {
        pub const PRINT: usize = 0x01;
        pub const GET_STRING: usize = 0x11;
        pub const GET_ITEM_NAME: usize = 0x21;
        pub const GET_ITEM_STAT: usize = 0x3E;
    }
    
    /// Internal D2Client functions
    pub mod func {
        /// PrintStringToChat function
        pub const PRINT_STRING: usize = 0x7D850;
        /// GetItemName internal
        pub const GET_ITEM_NAME: usize = 0x914F0;
        /// GetItemStats internal  
        pub const GET_ITEM_STAT: usize = 0x560B0;
    }
}

/// D2Common.dll offsets
pub mod d2common {
    /// Pointer to Items.txt data
    pub const ITEMS_TXT: usize = 0x9FB98;
    
    /// GetUnitStat function
    pub const GET_UNIT_STAT: usize = 0x38B70;
    
    /// D2Common_GetUnitStat injection offset (relative to D2Client inject base)
    pub const INJECT_GET_UNIT_STAT: usize = 0x54;
}

/// Path/Room iteration offsets for finding ground items
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
    pub const UNIT_TYPE: usize = 0x00;      // dword
    pub const CLASS: usize = 0x04;          // dword
    pub const UNIT_ID: usize = 0x0C;        // dword
    pub const UNIT_DATA: usize = 0x14;      // dword (pointer to type-specific data)
    pub const INVENTORY: usize = 0x60;      // dword (pointer to inventory)
    pub const NEXT_UNIT: usize = 0xE4;      // dword (pointer to next unit in list) - at offset 0x14 + 52*4 = 0xE4
}

/// ItemData structure offsets (pUnitData for items)
pub mod item_data {
    pub const QUALITY: usize = 0x00;        // dword (item quality enum)
    pub const FLAGS: usize = 0x18;          // dword (item flags) - offset 0 + 4 + 5*4 = 0x18
    pub const FILE_INDEX: usize = 0x2C;     // dword - offset 0x18 + 4 + 3*4 + 4 = 0x2C
    pub const EAR_LEVEL: usize = 0x48;      // byte - offset 0x2C + 4 + 7*4 = 0x48
    pub const NEXT_ITEM: usize = 0x64;      // dword (pointer to next item)
}

/// Inventory structure offsets
pub mod inventory {
    pub const FIRST_ITEM: usize = 0x0C;     // dword
    pub const WEAPON_ID: usize = 0x1C;      // dword
}

/// Items.txt record offsets (record size = 0x1A8)
pub mod items_txt {
    pub const RECORD_SIZE: usize = 0x1A8;
    
    pub const MISC: usize = 0x84;           // dword
    pub const NAME_ID: usize = 0xF4;        // word
    pub const STR_BONUS: usize = 0x106;     // word
    pub const DEX_BONUS: usize = 0x108;     // word
    pub const IS_2H: usize = 0x11C;         // byte
    pub const WEAPON_TYPE: usize = 0x11E;   // word
    pub const IS_1H: usize = 0x13D;         // byte
}

/// ItemTypes.txt record offsets (record size = 0xE4)
pub mod item_types_txt {
    pub const RECORD_SIZE: usize = 0xE4;
    pub const EQUIV1: usize = 0x04;         // word
    pub const EQUIV2: usize = 0x06;         // word
}

/// UniqueItems.txt record offsets
pub mod unique_items_txt {
    pub const LEVEL: usize = 0x34;          // word (at 13*4 = 0x34)
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

