# Loot Filter DSL Syntax

## Basic Format

```
"<name_pattern>" [quality] [tier] [eth] [{stat_pattern}] [color] [sound] [notify] [name] [stat]
```

All components except the quoted pattern are optional.

## Components

### 1. Name Pattern (Required)

A regex pattern in quotes that matches item names.

```
"Ring$"          # Items ending with "Ring"
"Stone of"       # Items containing "Stone of"
"."              # Match ALL items (dot = any character)
"Rune$"          # Items ending with "Rune"
"^Ber"           # Items starting with "Ber"
```

### 2. Quality Flags

| Flag | Quality |
|------|---------|
| `low` | Inferior |
| `normal` | Normal |
| `superior` | Superior |
| `magic` | Magic |
| `set` | Set |
| `rare` | Rare |
| `unique` | Unique |
| `craft` | Crafted |
| `honor` | Honorific |

### 3. Tier Flags (MedianXL)

| Flag | Tier |
|------|------|
| `0`, `1`, `2`, `3`, `4` | Normal tiers |
| `sacred` | Sacred |
| `angelic` | Angelic |
| `master` | Mastercrafted |

### 4. Ethereal Flag

```
eth    # Only match ethereal items
```

### 5. Stat Pattern

Regex pattern in braces that matches item stats.

```
{Skills}                    # Has "Skills" in stats
{[3-5] to All Skills}       # Has +3 to +5 All Skills
{\+\d+ to (Fire|Cold|Lightning)} # Has +X to elemental skills
```

**Note:** Stat patterns give rules **highest priority** in conflict resolution.

### 6. Color Flags

| Flag | Color | Hex |
|------|-------|-----|
| `transparent` | Transparent | #00000000 |
| `white` | White | #FFFFFF |
| `red` | Red | #FF0000 |
| `lime` | Lime | #15FF00 |
| `blue` | Blue | #7878F5 |
| `gold` | Gold | #F0CD8C |
| `grey` | Grey | #9D9D9D |
| `black` | Black | #000000 |
| `pink` | Pink | #FF00FF |
| `orange` | Orange | #FFBF00 |
| `yellow` | Yellow | #FFFF00 |
| `green` | Green | #008000 |
| `purple` | Purple | #9D00FF |

### 7. Visibility Flags

| Flag | Effect |
|------|--------|
| `show` | Force show item (overrides global hide mode) |
| `hide` | Force hide item (overrides global show mode) |

### 8. Sound Flags

| Flag | Sound |
|------|-------|
| `sound1` - `sound6` | Play sound 1-6 |
| `sound_none` | Explicitly no sound |

### 9. Notification Flag

```
notify    # Show overlay notification for this item
```

**Important:** `notify` is independent. Color and sound do NOT auto-enable it.

### 10. Display Flags

| Flag | Effect |
|------|--------|
| `name` | Include item name in notification |
| `stat` | Include item stats in notification |

---

## Comments

Lines starting with `#` are comments:

```
# This is a comment
"Ring$" unique gold notify    # Inline comments also work
```

---

## Examples

### Basic Rules

```
# Notify on all unique items with gold color
"." unique gold notify sound1

# Hide all normal quality items
"." normal hide

# Show sacred items with lime color
"." sacred lime notify

# Ethereal unique items
"." unique eth gold notify sound1
```

### Stat-Based Rules

```
# Rings with +All Skills (highest priority due to stat match)
"Ring$" {All Skills} red notify stat

# Amulets with +3 or more to All Skills
"Amulet" {[3-9] to All Skills} purple notify sound2 stat

# Items with Cannot Be Frozen
"." {Cannot Be Frozen} lime notify
```

### Complex Combinations

```
# Show unique items with color, but NO notification
"." unique gold

# Show AND notify about Stone of Jordan
"Jordan" unique gold notify sound1 name stat

# Hide normal items, but still get notified (sound only)
"." normal hide sound1

# Ethereal sacred weapons with damage stats
"." sacred eth {Damage} red notify sound2 name stat
```

### Typical Filter Setup

```
# High priority: specific valuable items
"Jordan" unique gold notify sound1 name stat
"Tyrael" unique gold notify sound1 name stat

# Medium priority: quality-based
"." unique gold notify sound2
"." set lime notify sound3
"." rare orange

# Low priority: cleanup
"." magic hide
"." normal hide
"." low hide
```

---

## Priority System

When multiple rules match, priority determines the winner:

1. **Stat Match (Highest):** Rules with `{stat_pattern}` that successfully match
2. **Color Flag:** Rules with explicit color
3. **Flag Count (Lowest):** Rules with more flags

### Example Priority Resolution

```
Rule A: "." unique gold                    # 2 flags
Rule B: "Ring$" lime                       # 1 flag
Rule C: "Ring$" unique {Skills} red notify # 4 flags + stat pattern
```

**For Unique Ring with +Skills:**
- All three rules match
- Rule C has stat match -> **Rule C wins**

**For Unique Ring without +Skills:**
- Rules A and B match (C's stat pattern fails)
- Both have color, A has more flags -> **Rule A wins**

---

## Global Mode

The editor UI provides a toggle for global filtering mode:

| Mode | Behavior |
|------|----------|
| **Show All** (default) | Items visible unless `hide` flag matches |
| **Hide All** | Items hidden unless `show` flag matches |

Rules with explicit `show`/`hide` flags override the global mode.
