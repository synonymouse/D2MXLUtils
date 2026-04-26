# Loot Filter DSL Syntax

## Grammar

```
filter        := line*
line          := blank | comment | default_mode | rule | group_open | group_close
comment       := '#' any*
default_mode  := ('hide' | 'show') 'default'
rule          := [name] attr*
group_open    := '[' attr* ']' '{'
group_close   := '}'
name          := '"' regex '"'
attr          := quality
               | tier
               | 'eth'
               | stat_pattern
               | color
               | visibility
               | sound
               | 'notify'
               | 'stat'
               | 'map'
stat_pattern  := '{' regex '}'
```

`attr*` permits repetition, so a rule may carry **multiple** `{regex}` stat
patterns. All listed patterns must match the item's stat text (AND).

Groups cannot be nested. The `default_mode` directive is only valid at file scope and may appear at most once per file.

---

## Default mode directive

A single file-scope directive controls how unmatched items are treated.

```
hide default      # hide all items unless a rule explicitly shows them
show default      # show items per the game's built-in filter (this is the default)
```

- Allowed only at file scope — never inside a `[...] { ... }` group.
- At most **one** occurrence per file. Duplicates are a parse error.
- Absent → equivalent to `show default`.
- Position in the file is free (top, bottom, middle). Convention: first non-comment line.

With `hide default`, only rules with an explicit `show` flag reveal items. Without it, the game's built-in filter decides whatever no rule forces.

---

## Rule Components

### Name pattern (optional)

Regex in double quotes, matched case-insensitively. The rule matches if the
regex hits **either** the item's runtime display name (e.g. the rare affix
`"Rune Turn"` or the unique name `"Stone of Jordan"`) **or** the items.txt
base type name (e.g. `"Ring"`, `"Amulet"`, `"Great Axe"`). So `"Ring$"` will
match any ring regardless of quality or generated affix.

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

Multiple quality keywords on one rule **OR together** — the rule matches if the
item's quality is any of the listed ones. Example: `magic rare unique hide`
hides magic, rare, and unique items. Duplicates are collapsed.

---

### Tier (MedianXL)

| Keyword | Tier |
|---|---|
| `0`, `1`, `2`, `3`, `4` | normal tiers |
| `sacred` | Sacred |
| `angelic` | Angelic |
| `master` | Mastercrafted |

Multiple tier keywords on one rule **OR together** — the rule matches if the
item's tier is any of the listed ones. Example: `1 2 3 4 hide` hides
tier 1–4 items. Combine with a quality to intersect: `1 2 3 4 unique hide`
hides only tier 1–4 uniques.

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

A rule may list **multiple** stat patterns. All of them must match the
item's stat blob for the rule to fire (AND). Each pattern is matched
independently — the patterns may appear in any order in the item's stats.

```
rare {All Skills} {Faster Cast Rate}                 # both required
"Amulet" rare {[3-9] to All Skills} {focus} {enemy fire} stat notify
```

Use alternation inside a single `{…}` to express OR:

```
rare {(Fire|Cold|Lightning) Resist}                  # any one of three
```

Every line whose text matches **any** of the rule's patterns is highlighted
in the notification when `stat` (or an implicit stat-pattern trigger) is
active. Cross-line patterns (e.g. `{(?s)a.*b}`) can still cause the rule
to match but don't highlight any single line.

---

### Color

One of:

`white`, `red`, `lime`, `blue`, `gold`, `grey`, `black`, `pink`, `orange`, `yellow`, `green`, `purple`.

Color alone does not produce a notification. Pair with `notify` to emit one.

---

### Visibility

| Keyword | Effect |
|---|---|
| `show` | force show this item (overrides `hide default` and the game's built-in hide) |
| `hide` | force hide this item |

Absent → default visibility applies (game decides, or `hide default` applies).

---

### Sound

| Keyword | Effect |
|---|---|
| `sound1`–`sound7` | sound index used by notification |
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
| `stat` | include item stats in the notification |

The unique/set name line is shown automatically for Set/TU/SU/SSU/SSSU drops, controlled by the **Compact name** notification setting (not by a per-rule flag). Other rarities always render as a single base-type line.

---

### Map marker

```
map    # drop a red-cross marker on the automap at the item's position
```

Independent from `notify`. Markers are placed on the native in-game automap, cleared automatically on area change, and refreshed as items drop or are picked up. Items resolved to `hide` are not marked.

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
[unique gold notify sound1] {
  "Jordan"
  "Tyrael"
  "Windforce"
}
```

Flattens to:

```
"Jordan" unique gold notify sound1
"Tyrael" unique gold notify sound1
"Windforce" unique gold notify sound1
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
# File-scope directive (at most one, optional)
hide default      # hide unmatched items
show default      # show unmatched items (implicit default)

# General rule form
[name-pattern] [quality] [tier] [eth] [{stat-pattern}]* [color] [show|hide] [sound] [notify] [stat] [map]

# Atoms
quality    := low | normal | superior | magic | set | rare | unique | craft | honor
tier       := 0 | 1 | 2 | 3 | 4 | sacred | angelic | master
color      := white | red | lime | blue | gold | grey | black
            | pink | orange | yellow | green | purple
visibility := show | hide
sound      := sound1 | sound2 | sound3 | sound4 | sound5 | sound6 | sound7 | sound_none
```
