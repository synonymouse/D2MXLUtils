# Loot Filter DSL Syntax

## Grammar

```
filter      := line*
line        := blank | comment | rule | group_open | group_close
comment     := '#' any*
rule        := [name] attr*
group_open  := '[' attr* ']' '{'
group_close := '}'
name        := '"' regex '"'
attr        := quality
             | tier
             | 'eth'
             | stat_pattern
             | color
             | visibility
             | sound
             | 'notify'
             | 'name'
             | 'stat'
stat_pattern := '{' regex '}'
```

Groups cannot be nested.

---

## Rule Components

### Name pattern (optional)

Regex in double quotes, matched case-insensitively against the item name.

```
"Ring$" unique gold
"Stone of Jordan" notify
"^(Ber|Jah|Sur|Lo|Ohm|Vex)$" orange
```

Omit the quotes to match any name:

```
unique gold
set lime notify
```

`"."` is equivalent to omitting the pattern.

---

### Quality

| Keyword | Quality |
|---|---|
| `low` | Inferior |
| `normal` | Normal |
| `superior` | Superior |
| `magic` | Magic |
| `set` | Set |
| `rare` | Rare |
| `unique` | Unique |
| `craft` | Crafted |
| `honor` | Honorific |

---

### Tier (MedianXL)

| Keyword | Tier |
|---|---|
| `0`, `1`, `2`, `3`, `4` | normal tiers |
| `sacred` | Sacred |
| `angelic` | Angelic |
| `master` | Mastercrafted |

---

### Ethereal

```
eth     # only ethereal items
```

---

### Stat pattern

Regex in braces, matched case-insensitively against the item's stat text.

```
{All Skills}
{\+[3-5] to All Skills}
{(Fire|Cold|Lightning) Resist}
```

---

### Color

One of:

`transparent`, `white`, `red`, `lime`, `blue`, `gold`, `grey`, `black`, `pink`, `orange`, `yellow`, `green`, `purple`.

Color alone does not produce a notification. Pair with `notify` to emit one.

---

### Visibility

| Keyword | Effect |
|---|---|
| `show` | force show this item (overrides Hide All and game's built-in hide) |
| `hide` | force hide this item |

Absent → default visibility applies (game decides, or Hide All applies).

---

### Sound

| Keyword | Effect |
|---|---|
| `sound1`–`sound6` | sound index used by notification |
| `sound_none` | explicit silence |

Sound alone does not produce a notification. Pair with `notify`.

---

### Notify

```
notify    # emit an overlay notification for this item
```

Independent from color and sound. Required for any notification to fire.

---

### Display flags

| Keyword | Effect |
|---|---|
| `name` | include item name in the notification |
| `stat` | include item stats in the notification |

---

## Groups

```
[shared-attrs] {
  rule1
  rule2
  ...
}
```

- Header accepts all rule attributes **except a name pattern**.
- Each rule in the body absorbs the header attributes.
- Rule-level attributes override the group's for the same field.
- Groups cannot be nested.
- A rule inside a group is evaluated in its file position (groups are flattened, order preserved).

### Example: shared highlight

```
[unique gold notify sound1 name] {
  "Jordan"
  "Tyrael"
  "Windforce"
}
```

Flattens to:

```
"Jordan" unique gold notify sound1 name
"Tyrael" unique gold notify sound1 name
"Windforce" unique gold notify sound1 name
```

### Example: shared stat filter

```
[unique {All Skills} red notify stat] {
  "Ring$"
  "Amulet"
  "Circlet"
}
```

### Example: override inside group

```
[hide] {
  normal
  low
  superior
  unique show gold notify    # show overrides hide from group
}
```

---

## Comments

```
# Full-line comment
unique gold notify    # Inline comment
```

---

## Evaluation

Rules (including those expanded from groups) are processed in source order for every dropped item. The **last rule that matches** determines the outcome. See `loot-filter-spec.md` for full semantics.

---

## Quick Reference

```
# General form
[name-pattern] [quality] [tier] [eth] [{stat-pattern}] [color] [show|hide] [sound] [notify] [name] [stat]

# Atoms
quality    := low | normal | superior | magic | set | rare | unique | craft | honor
tier       := 0 | 1 | 2 | 3 | 4 | sacred | angelic | master
color      := transparent | white | red | lime | blue | gold | grey | black
            | pink | orange | yellow | green | purple
visibility := show | hide
sound      := sound1 | sound2 | sound3 | sound4 | sound5 | sound6 | sound_none
```
