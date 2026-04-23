# Map Marker — Implementation Notes

## Goal

Add a `map` flag to the loot filter DSL that drops a native automap marker
when a rule matches — mirroring the "Show on Map" toggle in MXL's built-in
filter.

Target: Median XL **2.11.7** on D2 **1.13c** engine base. Tool used for RE:
Cheat Engine Lua scripting against `Game.exe`.

## Approach

MXL has its own marker pipeline (separate data store + render hook in
`D2Sigma.dll`). Fully reversing it is high-effort and unnecessary. Instead we
use the engine's native `AutomapCell` primitive directly:

1. Scan the player's nearby rooms for ground items.
2. For each matched item, allocate an `AutomapCell` via D2Client's
   `NewAutomapCell`, fill its fields, and splice it into the layer's
   `pObjects` BST.
3. The engine's per-frame automap renderer walks `pObjects` and draws our
   cells alongside MXL's markers — neither system aware of the other.
4. On area change the engine destroys the layer and our cells go with it.
   No manual cleanup needed.

## Verified D2Client.dll offsets (1.13c, MXL 2.11.7)

| Symbol            | Offset      | Notes                                                                    |
| ----------------- | ----------- | ------------------------------------------------------------------------ |
| `pPlayerUnit`     | `0x11BBFC`  | dword → player `UnitAny`                                                 |
| `pAutomapLayer`   | `0x11C1C4`  | dword → current `AutomapLayer`                                           |
| `NewAutomapCell`  | `0x5F6B0`   | `AutomapCell* __fastcall NewAutomapCell(void)` — pool alloc, uninitialized |

`AddAutomapCell` exists at `0x61320` but **crashes** when invoked from a
remote thread in our setup (cause not pinned down — likely calling
convention / thread context). We bypass it and write into the BST manually,
which works reliably.

## Structures

### `AutomapLayer` (at `*pAutomapLayer`)

| Offset | Type            | Field                                                |
| ------ | --------------- | ---------------------------------------------------- |
| `0x00` | u32             | `nLayerNo`                                           |
| `0x04` | u32             | `fSaved`                                             |
| `0x08` | `AutomapCell*`  | `pFloors` — BST root for terrain floor cells         |
| `0x0C` | `AutomapCell*`  | `pWalls`  — BST root for wall cells                  |
| `0x10` | `AutomapCell*`  | **`pObjects`** — BST root for icon cells             |
| `0x14` | `AutomapCell*`  | `pExtras`                                            |

**Only `pObjects` is safe to mutate.** `pFloors` / `pWalls` are
incrementally populated by the engine as the player walks; touching them
corrupts revealed terrain. In town with no quest icons `pObjects` is
typically `NULL`; in some quest zones the engine itself adds cells here, so
our cleanup must remove only our own cells, not nuke the slot.

### `AutomapCell` (20 bytes)

| Offset | Size | Field                              |
| ------ | ---- | ---------------------------------- |
| `0x00` | u32  | `fSaved` — set to `1`              |
| `0x04` | u16  | `nCellNo` — sprite id              |
| `0x06` | u16  | `xPixel` — cell-space x            |
| `0x08` | u16  | `yPixel` — cell-space y            |
| `0x0A` | u16  | `wWeight`                          |
| `0x0C` | `AutomapCell*` | `pLess` — BST left child  |
| `0x10` | `AutomapCell*` | `pMore` — BST right child |

> **Pitfall.** D2BS's published `AutomapCell` has `nCellNo` at `+0x02` and a
> 16-byte total. That layout is wrong for 1.13c MXL: `fSaved` is a DWORD
> here, which shifts every subsequent field down by 2 bytes. Calibrated
> against live memory with `diagLayer()` dumps.

## Sprite id

`nCellNo = 300` renders as a small red cross — visually appropriate for
loot. Other values mostly produce terrain fragments or nothing usable.
Verified by sweeping ids in CE.

## Coordinate transform

Cell `xPixel` / `yPixel` are in the engine's isometric automap space, scaled
from world subtile coordinates. Calibrated by standing the player on
placed cells (3 calibration points, X fits exact, Y systematic ≤5 unit
offset from rounding):

```
cell_x = round((sub_x − sub_y) · 8 / 5)
cell_y = round((sub_x + sub_y) · 4 / 5)
```

Y residual is well within the icon footprint, so no per-axis offset
correction is needed.

## Reading positions

Player and items use **different** path layouts. Both reached via
`UnitAny + 0x2C → pPath`, but the struct shape differs.

### Player (dynamic path)

```
sub_x = *(u16*)(pPath + 0x02)      // upper word of fixed-point xPos
sub_y = *(u16*)(pPath + 0x06)      // upper word of fixed-point yPos
```

The full DWORD at `+0x00` / `+0x04` is `(subtile << 16) | fractional`.

### Item (static path)

```
sub_x = *(u32*)(pPath + 0x0C)
sub_y = *(u32*)(pPath + 0x10)
```

The first 0x0C bytes are header (room pointer + flags); coords come after.

## Item discovery

Ground items don't appear in the global per-type unit hash table the way
players/monsters do, and they're absent from the
`pPath → pRoom1 → ppRoomsNear` "paths" iteration (which only enumerates
active entities — monsters/objects with their own `PathEntry`). They live
in `Room1`'s mixed-type unit linked list:

```
head    = *(Room1 + 0x74)
next    = *(UnitAny + 0xE8)        // pRoomNext
```

> **Pitfall.** `UnitAny + 0xE4` is `pListNext` (chains entries inside the
> game-wide unit hash table buckets); `+0xE8` is `pRoomNext` (chains units
> within a single Room1). For room scanning we need `+0xE8`. The existing
> `notifier.rs` walks `+0xE4` via `paths::PATH_TO_UNIT` and as a result
> doesn't actually find items via its Room1 iteration — items reach the
> notifier through `LootFilterHook` instead. That's fine for existing
> behavior, but our marker scanner cannot rely on the same path; it must
> walk `+0xE8` itself.

A single `pRoom1` is a small area (a handful of tiles), so items dropped
even one tile off the player's exact subtile may land in a sibling room.
Walk a BFS over `ppRoomsNear` to expand the scan zone.

### Room1 fields used

| Offset | Field                                |
| ------ | ------------------------------------ |
| `0x00` | `ppRoomsNear` (`Room1**`)            |
| `0x24` | `dwRoomsNear` (u32)                  |
| `0x74` | head of mixed-type unit linked list  |

### Scan algorithm (BFS, depth 4)

```
visited  = { player.pPath.pRoom1 }
frontier = [ player.pPath.pRoom1 ]

for depth in 1..=4:
    next = []
    for room in frontier:
        unit = *(room + 0x74)
        while unit != NULL:
            if unit.type == ITEM (4):
                yield unit
            unit = *(unit + 0xE8)
        if depth < 4:
            for i in 0..room.dwRoomsNear:
                near = room.ppRoomsNear[i]
                if near != NULL and near not in visited:
                    visited.add(near)
                    next.push(near)
    frontier = next
```

Depth 4 was tuned by experiment: 2 missed items at the edges of the
visible automap, 4 covers the area a player can plausibly care about
without exploding the scan cost.

## Marker placement

Per matched item:

1. `cell = NewAutomapCell()` — pool alloc; do not free (the engine reclaims
   the whole layer on area change).
2. Fill fields (struct above).
3. Splice into `pObjects` BST as a leaf. Order is irrelevant — the renderer
   walks the entire tree. Simplest insertion: walk down `pLess` until it's
   `NULL`, attach there.

```c
void insert(AutomapLayer* layer, AutomapCell* new_cell) {
    AutomapCell** slot = (AutomapCell**)((char*)layer + 0x10);
    while (*slot != NULL) {
        slot = (AutomapCell**)((char*)*slot + 0x0C);  // pLess
    }
    *slot = new_cell;
}
```

## Lifecycle

- **Area change**: engine tears down the `AutomapLayer`. Our cells go with
  it — no action needed on our side.
- **Within an area**: items get picked up, drop in/out of scan range, etc.
  The marker set must be rebuilt on a cadence (every drop event, or per
  scan tick).
- **Detaching old markers**: keep our placed cells in a side list. On
  rebuild, for each previously-placed cell find its parent in the BST and
  replace the link with `NULL` (or with one of the cell's children if
  preserving the tree shape matters — it doesn't for rendering, so `NULL`
  is fine since we're going to attach fresh cells anyway).
- **Never touch `pFloors` / `pWalls`.**

---

## Implementation pointers (for the Rust port — to be planned next)

Touch points:

- `src-tauri/src/offsets.rs` — add `D2CLIENT_NEW_AUTOMAP_CELL = 0x5F6B0`,
  `D2CLIENT_AUTOMAP_LAYER = 0x11C1C4`, `Room1` field offsets, item static
  path coord offsets, `UnitAny + 0xE8` for `pRoomNext`.
- `src-tauri/src/injection.rs` — new injected stub that, given a
  `(cell_x, cell_y, cell_no)` triple, calls `NewAutomapCell`, writes the
  fields, and performs the BST splice. Same machinery as existing
  `GetItemName` / `GetStringById` injections.
- `src-tauri/src/notifier.rs` — separate scanner pass walking the Room1
  BFS for items (the existing `LootFilterHook` doesn't expose item world
  position — we read it ourselves from `pPath + 0x0C/+0x10`).
- `src-tauri/src/rules/dsl.rs` + `matching.rs` — new `map` keyword,
  surfaced as a bool on the matched-rule effect.
- Marker bookkeeping: per-area `Vec<AutomapCellPtr>` with detach-on-rebuild.

The full plan goes in a separate document.

---

## Diagnostic Lua script (Cheat Engine)

Self-contained script that exposes every primitive we used during RE.
Attach CE to `Game.exe`, open the Lua engine (`Ctrl+Alt+L`), paste, and
call any of the functions below from the Lua console.

### Function catalogue

| Function | Use |
| --- | --- |
| `placeAtSelf()` | Sanity check: drops a red cross on the player. Confirms `pAutomapLayer`, `NewAutomapCell`, the cell struct, the coord formula, and the pPath read are all working. |
| `markAllItems()` | The real thing — BFS-scans 4 hops of nearby rooms, places a red cross on every ground item. Call again to refresh after picking up / new drops. |
| `clearMarkers()` | Wipes `pObjects` to `NULL`. Use to reset between experiments. |
| `placeAt(x, y)` | Place one cell at raw cell-space (xPixel, yPixel). For coord-formula experiments. |
| `dumpItemPath()` | Dumps the first ground item's pPath struct + reference player subtile. Use when adding a new game version / verifying coord offsets. |
| `dumpRoom1()` | Dumps current `Room1` first 0xC0 bytes. Use to find unit-list head if the `+0x74` offset shifts. |
| `diagLayer()` | Walks `pFloors` / `pWalls` / `pObjects` BSTs and prints each cell. Use to verify our marker actually got inserted. |
| `getPlayerSubtile()` | Returns `(sub_x, sub_y)` of the player. Building block. |
| `worldToCell(sx, sy)` | Returns `(cell_x, cell_y)` per the formula. Building block. |
| `findAllItems(depth)` | Returns array of `pUnit` for ground items within BFS depth (default 4). Building block. |

### Script

```lua
-- ==========================================================================
-- D2MXLUtils — automap marker diagnostic
-- D2 1.13c engine, Median XL 2.11.7. See map-marker-reverse-engineering.md.
-- ==========================================================================

-- D2Client.dll offsets
local PLAYER_UNIT      = 0x11BBFC
local AUTOMAP_LAYER    = 0x11C1C4
local NEW_AUTOMAP_CELL = 0x5F6B0

-- AutomapLayer field offsets
local LAYER_FLOORS  = 0x08
local LAYER_WALLS   = 0x0C
local LAYER_OBJECTS = 0x10

-- AutomapCell field offsets (20 bytes; D2BS layout is wrong for this build)
local CELL_FSAVED  = 0x00   -- u32
local CELL_NCELLNO = 0x04   -- u16
local CELL_XPIXEL  = 0x06   -- u16
local CELL_YPIXEL  = 0x08   -- u16
local CELL_WWEIGHT = 0x0A   -- u16
local CELL_PLESS   = 0x0C   -- ptr
local CELL_PMORE   = 0x10   -- ptr

-- UnitAny field offsets
local UNIT_TYPE      = 0x00
local UNIT_PATH      = 0x2C
local UNIT_ROOM_NEXT = 0xE8   -- pRoomNext (NOT +0xE4 = pListNext)

-- Room1 field offsets
local ROOM1_PP_NEAR     = 0x00   -- ppRoomsNear (Room1**)
local ROOM1_DW_NEAR     = 0x24   -- dwRoomsNear (u32)
local ROOM1_UNIT_FIRST  = 0x74   -- head of mixed-type unit list

-- Player dynamic-path field offsets (subtile = upper word of fixed-point xPos)
local PLAYER_PATH_X = 0x02   -- u16
local PLAYER_PATH_Y = 0x06   -- u16

-- Item static-path field offsets (subtile = full DWORD)
local ITEM_PATH_X = 0x0C   -- u32
local ITEM_PATH_Y = 0x10   -- u32

-- Sprite id that renders as a red cross
local SPRITE_RED_CROSS = 300

-- Unit type enum
local UNIT_TYPE_ITEM = 4

-- ==========================================================================
-- Core helpers
-- ==========================================================================

function d2c()
  local a = getAddress("D2Client.dll")
  if a == nil or a == 0 then error("D2Client.dll not found") end
  return a
end

function getPlayerSubtile()
  local pPlayer = readPointer(d2c() + PLAYER_UNIT) or 0
  if pPlayer == 0 then return nil end
  local pPath = readPointer(pPlayer + UNIT_PATH) or 0
  if pPath == 0 then return nil end
  local sx = readSmallInteger(pPath + PLAYER_PATH_X)
  local sy = readSmallInteger(pPath + PLAYER_PATH_Y)
  return sx, sy
end

function worldToCell(sx, sy)
  local cx = math.floor((sx - sy) * 8 / 5 + 0.5)
  local cy = math.floor((sx + sy) * 4 / 5 + 0.5)
  return cx, cy
end

-- Allocate one cell via NewAutomapCell, fill fields, return its address.
local function newCell(cx, cy, spriteNo)
  local cell = executeCodeEx(0, nil, d2c() + NEW_AUTOMAP_CELL)
  if cell == 0 then return 0 end
  writeInteger     (cell + CELL_FSAVED,  1)
  writeSmallInteger(cell + CELL_NCELLNO, spriteNo)
  writeSmallInteger(cell + CELL_XPIXEL,  cx)
  writeSmallInteger(cell + CELL_YPIXEL,  cy)
  writeSmallInteger(cell + CELL_WWEIGHT, 0)
  writeInteger     (cell + CELL_PLESS,   0)
  writeInteger     (cell + CELL_PMORE,   0)
  return cell
end

-- Splice cell into pObjects BST as the leftmost leaf.
local function insertCell(pLayer, newCell)
  local root = readPointer(pLayer + LAYER_OBJECTS) or 0
  if root == 0 then
    writeInteger(pLayer + LAYER_OBJECTS, newCell)
    return
  end
  local p = root
  while true do
    local pLess = readPointer(p + CELL_PLESS) or 0
    if pLess == 0 then
      writeInteger(p + CELL_PLESS, newCell)
      return
    end
    p = pLess
  end
end

function clearMarkers()
  local pLayer = readPointer(d2c() + AUTOMAP_LAYER) or 0
  if pLayer == 0 then print("no automap layer") return end
  writeInteger(pLayer + LAYER_OBJECTS, 0)
  print("pObjects cleared")
end

-- ==========================================================================
-- Single-cell placement (calibration / experiments)
-- ==========================================================================

function placeAt(cx, cy)
  local pLayer = readPointer(d2c() + AUTOMAP_LAYER) or 0
  if pLayer == 0 then print("no automap layer") return end
  writeInteger(pLayer + LAYER_OBJECTS, 0)   -- single-marker mode
  local cell = newCell(cx, cy, SPRITE_RED_CROSS)
  if cell == 0 then print("alloc fail") return end
  writeInteger(pLayer + LAYER_OBJECTS, cell)
  print(string.format(">>> placed at xPixel=%d yPixel=%d", cx, cy))
end

function placeAtSelf()
  local sx, sy = getPlayerSubtile()
  if sx == nil then print("no player") return end
  local cx, cy = worldToCell(sx, sy)
  print(string.format("player (%d, %d) -> cell (%d, %d)", sx, sy, cx, cy))
  placeAt(cx, cy)
end

-- ==========================================================================
-- Item discovery (BFS over near Room1's, depth 4)
-- ==========================================================================

function findAllItems(maxDepth)
  maxDepth = maxDepth or 4
  local items = {}
  local pPlayer = readPointer(d2c() + PLAYER_UNIT) or 0
  if pPlayer == 0 then return items end
  local pPath = readPointer(pPlayer + UNIT_PATH) or 0
  if pPath == 0 then return items end
  local pRoom1 = readPointer(pPath + 0x1C) or 0
  if pRoom1 == 0 then return items end

  local visited = { [pRoom1] = true }
  local frontier = { pRoom1 }

  for depth = 1, maxDepth do
    local nextFrontier = {}
    for _, room in ipairs(frontier) do
      local pUnit = readPointer(room + ROOM1_UNIT_FIRST) or 0
      while pUnit ~= 0 do
        if readInteger(pUnit + UNIT_TYPE) == UNIT_TYPE_ITEM then
          table.insert(items, pUnit)
        end
        pUnit = readPointer(pUnit + UNIT_ROOM_NEXT) or 0
      end
      if depth < maxDepth then
        local ppNear = readPointer(room + ROOM1_PP_NEAR) or 0
        local nNear  = readInteger(room + ROOM1_DW_NEAR) or 0
        if ppNear ~= 0 then
          for i = 0, nNear - 1 do
            local near = readPointer(ppNear + 4 * i) or 0
            if near ~= 0 and not visited[near] then
              visited[near] = true
              table.insert(nextFrontier, near)
            end
          end
        end
      end
    end
    frontier = nextFrontier
    if #frontier == 0 then break end
  end
  return items
end

-- ==========================================================================
-- Mark every ground item with a red cross
-- ==========================================================================

function markAllItems()
  local pLayer = readPointer(d2c() + AUTOMAP_LAYER) or 0
  if pLayer == 0 then print("no automap layer") return end

  -- Wipe previous batch. NB: in some quest zones the engine itself adds
  -- cells here — for production code we'd track our own and detach
  -- selectively; for diagnostic use this is fine.
  writeInteger(pLayer + LAYER_OBJECTS, 0)

  local items = findAllItems()
  print(string.format("found %d items in nearby rooms", #items))

  for i, pUnit in ipairs(items) do
    local pPath = readPointer(pUnit + UNIT_PATH) or 0
    if pPath ~= 0 then
      local sx = readInteger(pPath + ITEM_PATH_X)
      local sy = readInteger(pPath + ITEM_PATH_Y)
      if sx and sy and sx > 0 and sy > 0 then
        local cx, cy = worldToCell(sx, sy)
        local cell = newCell(cx, cy, SPRITE_RED_CROSS)
        if cell ~= 0 then
          insertCell(pLayer, cell)
          print(string.format("  [%d] sub=(%d,%d) cell=(%d,%d) @0x%X",
            i, sx, sy, cx, cy, cell))
        end
      end
    end
  end
end

-- ==========================================================================
-- Debug dumps
-- ==========================================================================

local function findFirstItem()
  local items = findAllItems(1)
  return items[1]
end

function dumpItemPath()
  local pUnit = findFirstItem()
  if pUnit == nil then print("no items in nearby rooms") return end

  local cls    = readInteger(pUnit + 0x04)
  local uid    = readInteger(pUnit + 0x0C)
  local pPath  = readPointer(pUnit + UNIT_PATH)
  print(string.format("ITEM unitId=%d class=%d pUnit=0x%X pPath=0x%X",
    uid, cls, pUnit, pPath))
  if pPath == 0 then print("  no pPath!") return end

  print("--- raw pPath dwords ---")
  for off = 0, 0x20, 4 do
    local v = readInteger(pPath + off)
    print(string.format("  +0x%02X = 0x%08X  (%d)", off, v & 0xFFFFFFFF, v))
  end

  local psx, psy = getPlayerSubtile()
  print(string.format("--- player subtile reference: (%d, %d) ---", psx or -1, psy or -1))
  print(string.format("item subtile (DWORD@+0x0C,+0x10): (%d, %d)",
    readInteger(pPath + ITEM_PATH_X), readInteger(pPath + ITEM_PATH_Y)))
end

function dumpRoom1()
  local pPlayer = readPointer(d2c() + PLAYER_UNIT) or 0
  local pPath   = readPointer(pPlayer + UNIT_PATH) or 0
  local pRoom1  = readPointer(pPath + 0x1C) or 0
  print(string.format("pRoom1 = 0x%X", pRoom1))
  for off = 0, 0xC0, 4 do
    local v = readInteger(pRoom1 + off)
    if v then
      v = v & 0xFFFFFFFF
      local mark = ""
      if v > 0x01000000 and v < 0x40000000 then mark = " <ptr?>" end
      print(string.format("  +0x%02X = 0x%08X%s", off, v, mark))
    end
  end
end

function diagLayer()
  local pLayer = readPointer(d2c() + AUTOMAP_LAYER) or 0
  if pLayer == 0 then print("no automap layer") return end
  print(string.format("pLayer = 0x%X", pLayer))
  local sx, sy = getPlayerSubtile()
  if sx then print(string.format("Player: x=%d y=%d", sx, sy)) end
  print("Layer fields:")
  for off = 0, 0x20, 4 do
    local v = readInteger(pLayer + off)
    print(string.format("  +0x%02X = 0x%08X  (%d)", off, v & 0xFFFFFFFF, v))
  end

  local function walk(label, off)
    local head = readPointer(pLayer + off) or 0
    print(string.format("--- %s @ pLayer+0x%02X (head=0x%X) ---", label, off, head))
    local depth = 0
    local function rec(p)
      if p == 0 or depth > 8 then return end
      local indent = string.rep("  ", depth)
      print(string.format("%s[0x%X] fSaved=%d nCellNo=%d x=%d y=%d w=%d pLess=0x%X pMore=0x%X",
        indent,
        p,
        readInteger(p + CELL_FSAVED),
        readSmallInteger(p + CELL_NCELLNO),
        readSmallInteger(p + CELL_XPIXEL),
        readSmallInteger(p + CELL_YPIXEL),
        readSmallInteger(p + CELL_WWEIGHT),
        readPointer(p + CELL_PLESS) or 0,
        readPointer(p + CELL_PMORE) or 0))
      depth = depth + 1
      rec(readPointer(p + CELL_PLESS) or 0)
      depth = depth - 1
    end
    rec(head)
  end

  walk("pFloors",  LAYER_FLOORS)
  walk("pWalls",   LAYER_WALLS)
  walk("pObjects", LAYER_OBJECTS)
end

print("D2MXLUtils automap diagnostic loaded.")
print("Try: placeAtSelf(), markAllItems(), clearMarkers(), dumpItemPath()")
```

### Quick smoke test

1. Enter game, attach CE, paste script.
2. `placeAtSelf()` — red cross under your character. Confirms layer + alloc + struct + coord formula.
3. Drop 2-3 items around you. `markAllItems()` — cross on each.
4. Pick up one, `markAllItems()` again — that cross gone, others remain.
5. Walk to another area. `markAllItems()` — markers from before are gone (layer reset on area change), new ones can be placed.

If any step misbehaves, run the matching dump (`dumpItemPath`,
`dumpRoom1`, `diagLayer`) and compare against the offsets table at the
top of this document.
