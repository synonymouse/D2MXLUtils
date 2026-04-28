# Loot Filter Examples

Scenarios illustrating last-match semantics, groups, and the `hide default` / `show default` directive.

---

## Scenario 1 — Last-match wins

```
unique gold
"Ring$" unique red notify
```

| Item | Matches | Winner | Result |
|---|---|---|---|
| Unique Amulet | line 1 | line 1 | gold, no notification |
| Unique Ring | lines 1, 2 | line 2 | red + notify (last match) |

Swap the order to flip the outcome:

```
"Ring$" unique red notify
unique gold
```

Now Unique Ring matches both, and the second line (`unique gold`) wins — all uniques end up gold with no notification.

**Takeaway:** put general rules first, specific rules last.

---

## Scenario 2 — Stat filtering via regex

```
rare {All Skills} purple notify stat
```

Matches any rare item whose stat text contains "All Skills". With `stat` flag, the notification includes the actual stat text.

Combining multiple conditions is a single regex:

```
rare {(All Skills).*(Faster Cast|Resist)} purple notify stat
```

---

## Scenario 3 — `show default` (implicit)

Game's built-in filter is in charge by default. Rules add highlights and override visibility only where needed. No directive needed — `show default` is implicit.

```
# Highlight uniques and sets in-game
unique gold
set lime

# Notify on the best drops
"Jordan|Tyrael|Windforce" unique gold notify sound1 stat

# Hide trash the game would otherwise show
normal hide
low hide
```

Unmatched items follow the game's decision.

---

## Scenario 4 — `hide default`

Only items whose winning rule has `show` are visible. Everything else is hidden.

```
hide default

unique show gold notify sound1
set show lime notify
"Rune$" show orange
```

Unique Ring → shown gold with notification.
Magic Sword → no match → hidden.
Any Rune → shown orange, no notification.

---

## Scenario 5 — Notify independence

```
unique gold                        # color only, no notification
set lime sound1                    # color + sound, no notification
rare notify                        # notification with defaults
"Jordan" unique gold notify sound1 # color + sound + notification
```

| Item | Color shown | Sound plays | Notification text |
|---|---|---|---|
| Unique Boots | gold | — | — |
| Set Armor | lime | — | — |
| Rare Ring | default | — | yes (default color, silent) |
| Stone of Jordan | gold | yes | yes |

`color` and `sound` never auto-enable `notify`.

---

## Scenario 5b — Multi-tier / multi-quality rules (OR-match)

```
# Hide every non-sacred equipment drop (tiers 1-4) regardless of quality
1 2 3 4 hide

# Narrower: hide only tier 1-4 uniques, keep tier 1-4 magic/rare visible
1 2 3 4 unique hide

# Hide all non-rare low-quality junk in one line
normal low magic hide
```

Multiple quality or tier keywords on one rule are OR-combined. A rule with
both qualities and tiers intersects them: `1 2 3 4 unique hide` matches an
item that is (tier 1 OR 2 OR 3 OR 4) AND unique.

---

## Scenario 5c — Filter by socket count

```
# Notify on any 4-, 5-, or 6-socket Crystal Sword
"Crystal Sword" sockets4 sockets5 sockets6 notify gold

# Force-show every 6-socket item with a red label
sockets6 show notify red

# Hide superior bases without sockets — they're crafting fodder otherwise.
sockets0 superior hide
```

Socket counts OR together just like tiers. `sockets0` means "no sockets";
combine with quality/tier to narrow further. The notifier prepends a
`Socketed (N)` line to the item's stats so you can also match against it
with a `{Socketed \(6\)}` regex if you prefer text-based filtering.

---

## Scenario 6 — Hide but notify

```
normal hide notify sound3
```

Normal items are hidden on the ground, but a notification fires when they drop. Useful for gambling or crafting bases.

---

## Scenario 7 — Groups

### Shared highlight for named uniques

```
[unique gold notify sound1] {
  "Jordan"
  "Tyrael"
  "Windforce"
  "^Griffon"
  "Mara"
}
```

### Group with stat filter

```
[unique {All Skills} red notify stat] {
  "Ring$"
  "Amulet"
  "Circlet"
}
```

Applies to unique rings / amulets / circlets that roll +All Skills.

### Override inside group

```
[hide] {
  normal
  low
  superior
  unique show gold notify    # unique quality overrides hide -> show gold
}
```

Order still matters for last-match: a later rule outside the group can still override.

```
[hide] {
  normal
  low
}
"Scroll of Town Portal" show    # shown even though hidden by group above? See note.
```

**Note:** the "Scroll of Town Portal" rule does not match `normal` or `low` quality keywords (it has no quality attribute), so it is independent. If you need truly overlapping rules, rely on source order.

---

## Scenario 8 — General-then-specific ordering

Canonical structure of a filter:

```
# 1. Broad defaults
magic hide
normal hide
low hide

# 2. Quality-wide highlights
unique gold
set lime
rare orange

# 3. Tier-based specialization
[unique sacred gold notify sound2] {
  "Ring$"
  "Amulet"
  "Circlet"
}

# 4. Stat-specific callouts
unique {All Skills} red notify sound1 stat

# 5. Specific named items (highest priority via being last)
[unique gold notify sound1 stat] {
  "Jordan"
  "Tyrael"
  "Windforce"
}

# 6. Runes (shown, separate colors)
"^(El|Eld|Tir|Nef|Eth|Ith|Tal|Ral|Ort|Thul)$" white
"^(Amn|Sol|Shael|Dol|Hel|Io|Lum|Ko|Fal|Lem)$" yellow
"^(Pul|Um|Mal|Ist|Gul|Vex|Ohm|Lo|Sur|Ber|Jah|Cham|Zod)$" orange notify sound1
```

---

## Complete Filter — leveling

```
# Defaults: hide junk
magic hide
normal hide
low hide

# Uniques / sets visible
unique gold
set lime

# Sacred uniques get extra fanfare
unique sacred gold notify sound2

# Ethereal sacred for rerolls
eth sacred lime notify

# Runes worth grabbing
"^(Pul|Um|Mal|Ist|Gul|Vex|Ohm|Lo|Sur|Ber|Jah|Cham|Zod)$" orange notify sound1

# Named drops are always announced
[unique gold notify sound1 stat] {
  "Jordan"
  "Tyrael"
  "Windforce"
  "Mara"
  "Shako"
}
```

---

## Complete Filter — white-list (`hide default`)

Put `hide default` at the top of the file; everything unmatched is hidden.

```
hide default

# White-list: everything else is hidden
unique show gold notify sound1
set show lime notify sound2

# Stat-gated rares
rare {All Skills} show purple notify stat

# All runes
"Rune$" show orange

# Specific items with full notification
[unique show gold notify sound1 stat] {
  "Jordan"
  "Tyrael"
  "Windforce"
}
```

---

## Scenario N — Automap markers

Drop a red cross on the in-game automap at the location of every matched item.

```
# Silent map ping for SSSUs — no overlay spam, just a marker.
[sssu map] {
  .
}

# Full treatment for specific chase items: overlay + sound + marker.
"Stone of Jordan" unique notify sound1 map
"Tyrael" unique notify sound1 map
```

| Rule | Marker | Overlay |
|---|---|---|
| `unique map` | yes, for every unique | no |
| `unique notify map` | yes | yes |
| `rare {All Skills} map` | yes, only on rares with "All Skills" | no |

Markers are cleared automatically on area change and rebuilt as items drop or get picked up. Items resolved to `hide` are never marked — the marker respects the visibility resolution.

---

## Edge Cases

### No attributes

```
normal
```

Matches any normal item. Produces no action. Valid syntax, no-op.

### Empty rule `.`

```
. gold notify
```

Equivalent to `gold notify` without a name pattern.

### Invalid regex

```
"Ring[" unique gold
```

Falls back to substring matching against "Ring[".

### Conflicting rules for the same item

```
unique gold
unique hide
```

Unique items end up hidden — the second rule is the last match. Reorder to flip.
