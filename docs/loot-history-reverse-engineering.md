# Loot History — `pInvOwner` Reverse Engineering

## Goal

Find the byte offset (relative to the start of the `ItemData` struct) of the
back-pointer that links an item to its owning `Inventory`. With this field we
can determine whether *the local hero* picked up an item with a single
`ReadProcessMemory` per drop (Variant 2 in the spec), instead of walking the
entire player inventory chain (Variant 3).

The struct in question is what D2 internally calls `ItemData` /
`D2ItemDataStrc` — pointed to by `UnitAny.pUnitData` (`+0x14`) when
`unit_type == 4` (item).

## Sources Probed

| # | Project | URL | Result |
|---|---|---|---|
| 1 | D2MOO | `raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Items/Items.h` | 404 (path wrong) |
| 2 | D2MOO | `raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Units/UnitsTypes.h` | 404 |
| 3 | D2MOO | `raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/Inventory/Inventory.h` | 404 |
| 4 | D2MOO | `raw.githubusercontent.com/ThePhrozenKeep/D2MOO/master/source/D2Common/include/D2Items.h` | 200 — only contains a forward declaration; struct body lives elsewhere (`Units/Units.h`, not yet probed) |
| 5 | D2BS | `raw.githubusercontent.com/noah-/d2bs/master/branch/1.13c/d2bs/D2Structs.h` | 404 (path wrong) |
| 6 | D2BS | `raw.githubusercontent.com/noah-/d2bs/master/D2Structs.h` | **200 — full struct** |
| 7 | BH Maphack | `raw.githubusercontent.com/planqi/slashdiablo-maphack/master/BH/D2Structs.h` | **200 — full struct** |
| 8 | PlugY (Speakus) | `raw.githubusercontent.com/Speakus/plugy/master/Commons/D2UnitStruct.h` | **200 — full struct** |
| 9 | D2 1.11B (jankowskib/d2server) | `raw.githubusercontent.com/jankowskib/d2server/master/d2warden-pvp/D2Structs_111B.h` | **200 — full struct** |

## Findings

All four sources that yielded a struct body agree on the offset of the
back-pointer: **`0x5C`**, the field type is `Inventory*`. The field name
differs across forks (BH/D2BS use `pOwnerInventory`, PlugY uses `ptInventory`,
1.11B uses `pNodeInv`), but the slot is identical.

### Cross-validation table

| Source | Field name | Offset | Slot above (`char[16]` player name) |
|---|---|---|---|
| BH Maphack `D2Structs.h` | `Inventory* pOwnerInventory` | `0x5C` | `personalizedName[16]` @ `0x4A`, `WORD _10` @ `0x5A` |
| D2BS `D2Structs.h` | `Inventory* pOwnerInventory` | `0x5C` | `szPlayerName[16]` @ `0x4A` (2-byte align pad implicit) |
| PlugY (Speakus) `D2UnitStruct.h` | `Inventory* ptInventory` | `+0x5C` | `IName[0x12]` @ `+0x4A` |
| jankowskib `D2Structs_111B.h` | `Inventory* pNodeInv` | `0x5C` | `szPlayerName[16]` @ `0x4A`, `BYTE _3[2]` @ `0x5A` |

Arithmetic: 16-byte player name buffer at `0x4A` ends at `0x5A`. The next
field is a 4-byte pointer; default `#pragma pack` yields a 4-byte alignment,
so the compiler pads `0x5A`–`0x5B` and emits the pointer at `0x5C`. This
matches every source.

The neighbouring slots are also consistent:

- `0x60`: `pPrevInvItem` / `_10` / `pItem`
- `0x64`: `pNextInvItem`
- `0x68`–`0x69`: `BodyLocation`-style bytes (`GameLocation`, `NodePage`,
  `NodePos`, `NodePosOther`)
- `0x84` (where present): `UnitAny* pOwner`

None of the headers apply `#pragma pack` to the `ItemData` struct, so
default 4-byte alignment is in effect — exactly what `#[repr(C)]` produces
on x86 in our crate.

### Semantics

- `NULL` while the item is on the ground or freed.
- Non-NULL points to the `Inventory*` that currently owns the item — could
  be the local player's inventory, a shared stash page, a vendor's storage,
  a corpse, or another player's inventory in a multiplayer game.
- To decide *who* owns an item, dereference once more: `Inventory + 0x08`
  is `pOwner: UnitAny*`. Compare its `unit_id` (`+0x0C`) with our cached
  local `pPlayerUnit` to confirm it is *our* hero (multiplayer-safe).

## Decision

**Offset confirmed: `0x5C`.** Four independent reverse-engineering projects,
spanning D2 versions 1.11B through 1.13d, agree exactly. Confidence is high
enough to use the single-read ownership variant (Variant 2 in the spec)
without further reverse engineering.

D2MOO would have been the most authoritative source (clean reimplementation
with original symbol names) but its `ItemData` struct body was not in the
header probed; given the four-way agreement across D2BS / BH / PlugY / 1.11B
that's not a blocker.

## Action Taken

- Added `item_data::INV_OWNER = 0x5C` to `src-tauri/src/offsets.rs`.
- Field is read as a 4-byte pointer (`u32` on 32-bit D2). NULL means
  "on the ground / freed"; non-NULL is an `Inventory*`.
