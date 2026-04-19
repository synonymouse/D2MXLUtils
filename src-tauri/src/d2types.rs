//! D2 memory structures
//! Based on D2Stats.au3 from MedianXL
//!
//! Note: These are #[repr(C)] structs that match the game's memory layout.
//! We read them directly from process memory.

use crate::offsets::{item_flags, item_quality, unit_type};

/// UnitAny - base structure for all game units (players, monsters, items, etc.)
///
/// AutoIt definition:
/// ```
/// DllStructCreate("dword iUnitType;dword iClass;dword pad1;dword dwUnitId;dword pad2;dword pUnitData;dword pad3[52];dword pUnit;")
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnitAny {
    pub unit_type: u32,   // 0x00: Unit type (0=player, 1=monster, 4=item, etc.)
    pub class: u32,       // 0x04: Class/type ID within unit type
    pub _pad1: u32,       // 0x08
    pub unit_id: u32,     // 0x0C: Unique unit identifier
    pub _pad2: u32,       // 0x10
    pub p_unit_data: u32, // 0x14: Pointer to type-specific data (ItemData for items)
    pub _pad3: [u32; 52], // 0x18-0xE0 padding
    pub p_next_unit: u32, // 0xE4: Pointer to next unit in list (was called pUnit in AutoIt)
}

impl Default for UnitAny {
    fn default() -> Self {
        Self {
            unit_type: 0,
            class: 0,
            _pad1: 0,
            unit_id: 0,
            _pad2: 0,
            p_unit_data: 0,
            _pad3: [0; 52],
            p_next_unit: 0,
        }
    }
}

impl UnitAny {
    pub fn is_item(&self) -> bool {
        self.unit_type == unit_type::ITEM
    }

    pub fn is_monster(&self) -> bool {
        self.unit_type == unit_type::MONSTER
    }

    pub fn is_player(&self) -> bool {
        self.unit_type == unit_type::PLAYER
    }
}

/// ItemData - extended data for item units
///
/// AutoIt definition:
/// ```
/// DllStructCreate("dword iQuality;dword pad1[5];dword iFlags;dword pad2[3];dword dwFileIndex;dword pad2[7];byte iEarLevel;")
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ItemData {
    pub quality: u32,    // 0x00: Item quality (magic, rare, unique, etc.)
    pub _pad1: [u32; 5], // 0x04-0x14 padding
    pub flags: u32,      // 0x18: Item flags (identified, ethereal, etc.)
    pub _pad2: [u32; 3], // 0x1C-0x24 padding
    pub file_index: u32, // 0x28: Index into items.txt (actually at 0x2C based on calc)
    pub _pad3: [u32; 7], // 0x2C-0x44 padding
    pub ear_level: u8,   // 0x48: For ears - level of killed player
}

impl ItemData {
    pub fn is_identified(&self) -> bool {
        (self.flags & item_flags::IDENTIFIED) != 0
    }

    pub fn is_ethereal(&self) -> bool {
        (self.flags & item_flags::ETHEREAL) != 0
    }

    pub fn is_socketed(&self) -> bool {
        (self.flags & item_flags::SOCKETED) != 0
    }

    pub fn is_runeword(&self) -> bool {
        (self.flags & item_flags::RUNEWORD) != 0
    }

    pub fn quality_name(&self) -> &'static str {
        match self.quality {
            item_quality::NONE => "None",
            item_quality::INFERIOR => "Inferior",
            item_quality::NORMAL => "Normal",
            item_quality::SUPERIOR => "Superior",
            item_quality::MAGIC => "Magic",
            item_quality::SET => "Set",
            item_quality::RARE => "Rare",
            item_quality::UNIQUE => "Unique",
            item_quality::CRAFTED => "Crafted",
            item_quality::HONORIFIC => "Honorific",
            _ => "Unknown",
        }
    }
}

/// UniqueItemsTxt - record from UniqueItems.txt
///
/// AutoIt definition:
/// ```
/// DllStructCreate("dword pad1[13];word wLvl;")
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct UniqueItemsTxt {
    pub _pad1: [u32; 13], // 0x00-0x30 padding
    pub level: u16,       // 0x34: Required level
}

/// Scanned item info - higher level representation for the notifier
#[derive(Debug, Clone)]
pub struct ScannedItem {
    /// Raw unit pointer (for injection calls)
    pub p_unit: u32,
    /// Pointer to ItemData structure
    pub p_unit_data: u32,
    /// Unique unit ID
    pub unit_id: u32,
    /// Item class (index into items.txt)
    pub class: u32,
    /// Item quality
    pub quality: u32,
    /// Item flags
    pub flags: u32,
    /// Whether item is ethereal
    pub is_ethereal: bool,
    /// Whether item is identified
    pub is_identified: bool,
    /// Item name (retrieved via injection)
    pub name: Option<String>,
    /// Item stats text (retrieved via injection)
    pub stats: Option<String>,
    /// File index from ItemData
    pub file_index: u32,
    /// Ear level (if applicable)
    pub ear_level: u8,
}

impl ScannedItem {
    pub fn from_unit(unit: &UnitAny, item_data: &ItemData, p_unit: u32) -> Self {
        Self {
            p_unit,
            p_unit_data: unit.p_unit_data,
            unit_id: unit.unit_id,
            class: unit.class,
            quality: item_data.quality,
            flags: item_data.flags,
            is_ethereal: item_data.is_ethereal(),
            is_identified: item_data.is_identified(),
            name: None,
            stats: None,
            file_index: item_data.file_index,
            ear_level: item_data.ear_level,
        }
    }

    pub fn quality_name(&self) -> &'static str {
        match self.quality {
            item_quality::NONE => "None",
            item_quality::INFERIOR => "Inferior",
            item_quality::NORMAL => "Normal",
            item_quality::SUPERIOR => "Superior",
            item_quality::MAGIC => "Magic",
            item_quality::SET => "Set",
            item_quality::RARE => "Rare",
            item_quality::UNIQUE => "Unique",
            item_quality::CRAFTED => "Crafted",
            item_quality::HONORIFIC => "Honorific",
            _ => "Unknown",
        }
    }
}

/// Inventory structure for reading equipped items
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Inventory {
    pub _pad1: [u32; 3],   // 0x00-0x08
    pub p_first_item: u32, // 0x0C: First item in inventory
    pub _pad2: [u32; 3],   // 0x10-0x18
    pub weapon_id: u32,    // 0x1C: Currently equipped weapon ID
}

/// Color codes for D2 print functions
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum PrintColor {
    White = 0,
    Red = 1,
    Green = 2,
    Blue = 3,
    Gold = 4,
    Grey = 5,
    Black = 6,
    Tan = 7,
    Orange = 8,
    Yellow = 9,
    DarkGreen = 10,
    Purple = 11,
}
