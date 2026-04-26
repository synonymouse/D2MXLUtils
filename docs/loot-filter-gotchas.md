# Loot Filter — Non-Obvious Edge Cases

Practical companion to `filter_spec/loot-filter-spec.md` and
`filter_spec/loot-filter-dsl.md`. Documents the subtle pitfalls that
surface in real filters and how to avoid them.

---

## 1. No notification fires without an explicit `notify` flag

`color`, `sound`, and `stat` alone do **not** trigger an overlay
notification. They only describe **how** a notification looks **if**
one fires. The `notify` flag is the on/off switch.

### Wrong

```
unique gold sound1            # gold tooltip on the ground, no overlay
"Stone of Jordan" red sound2  # red tooltip, no overlay alert
```

### Right

```
unique gold sound1 notify
"Stone of Jordan" red sound2 notify
```

The same applies inside groups — put `notify` on the group header
so every child inherits it:

```
[unique gold sound1 notify] {
  "Stone of Jordan"
  "Tyrael's Might"
}
```

`map` is also independent of `notify` — a rule with just `map`
places an automap marker silently, no overlay. Combine `notify map`
for both.

---

## 2. Tier digits `1`–`4` skip jewelry, charms, runes, potions

Tier numbers `1`-`4` only cover **weapons and armor**. Rings,
amulets, jewels, quivers, charms, runes, and potions all live at
**tier 0**, regardless of quality.

### Wrong

```
# Intent: hide every magic item until Sacred
1 2 3 4 magic hide
```

A magic amulet has `tier = 0`, fails the tier-axis check, the rule
is skipped, the amulet is shown. Same for magic rings, magic jewels,
magic charms.

### Right

If you want a quality filter across all tiers, omit the tier numbers:

```
magic hide
```

If you want a tier filter that also covers jewelry, include `0`:

```
0 1 2 3 4 magic hide
```

**Rule of thumb:** tier digits are only useful when the filter
target is equipment with an explicit tier number in its base name (e.g.
"Crystal Sword (3)").

---

## 3. Multiple keywords on one rule: AND across axes, OR within an axis

A rule has independent matching axes (name, quality, tier, eth, stat).
Listing several keywords on the **same axis** is OR. Combining
**different axes** is AND.

```
1 2 3 4 magic rare hide
```

reads as `(tier ∈ {1,2,3,4}) AND (quality ∈ {magic, rare})`. An item
failing **either** axis is not hidden. This is a common source of
"why doesn't my hide rule work" bugs.

### Wrong assumption

This rule does **not** mean "hide tier 1, tier 2, tier 3, tier 4,
magic items, and rare items independently". It is a single
intersected condition.

### Right

If you want each independently, write three rules:

```
1 2 3 4 hide
magic hide
rare hide
```

---

## 4. Name patterns are substring-matched, not full-string-matched

`"Foo"` matches any item whose displayed name OR base item type
**contains** "foo" (case-insensitive). It is **not** a full-string
match.

### Trap

```
"Vessel" notify
```

Fires on the consumable "Vessel of …" but also on any rare/unique
item whose generated name happens to contain the substring "vessel".

### Right

Use anchors:

```
"^Vessel"          # starts with Vessel
"^Vessel$"         # exactly "Vessel"
"Crown$"           # ends with Crown — catches Crown, Grand Crown
"^Grand Crown$"    # exactly "Grand Crown"
```

Note: anchors apply to the displayed name **or** the base item
type — whichever side hits first wins (see §6). For equipment that
also drops in normal/white quality (helms, weapons, armor) anchors
on the base item type are reliable; for jewelry, charms, runes, and
other always-named drops, treat patterns as filters over the
displayed name.

---

## 5. `|` inside `"..."` is regex alternation, not a literal pipe

```
"Heavenly|Crate" notify
```

is regex `Heavenly|Crate`, which matches **"Heavenly" alone OR
"Crate" alone**, anywhere in the name. It does **not** mean "items
literally named 'Heavenly|Crate'", nor "items containing both
words". Random affixes containing either word will trigger it.

### Right — single literal

```
"Heavenly Crate" notify
```

### Right — closed alternation, anchored

```
"^(Heavenly|Astral|Apocalyptic) Crate$" notify
```

### Right — multiple separate rules / a group

```
[notify] {
  "Heavenly Crate"
  "Astral Crate"
}
```

---

## 6. Name patterns search BOTH the displayed name and the base item type

When a rule has a `"<pattern>"`, the regex is checked twice:

1. against the item's **displayed name** (unique name, rare affix
   combination, etc.);
2. against the **base item type** ("Ring", "Great Axe", "Athulua's
   Hand", …).

A hit on **either** counts as a match. Useful for catching a base
item regardless of generated affix — but a footgun when the same
substring also appears in unrelated affixes or unique names.

### Surprise

```
"Crown" rare notify
```

Intent: catch rare Crown / Grand Crown helms. Actually fires on:

- a rare Crown or Grand Crown ✓
- a rare item with a generated affix containing "crown" anywhere
- any item whose displayed name happens to contain "crown"

Anchor or enumerate to constrain it:

```
"^(Crown|Grand Crown)$" rare notify
```

---

## 7. `notify`, `show`/`hide`, and `map` are independent flags

The winning rule applies its flags independently:

| Flag combo                   | Visibility           | Notification | Marker |
|---|---|---|---|
| `unique gold`                | game default         | none         | no     |
| `unique gold notify`         | game default         | gold + sound | no     |
| `"X" hide notify sound2`     | hidden               | text + sound | no     |
| `"X" map`                    | game default         | none         | yes    |
| `"X" hide map`               | hidden               | none         | **no** (markers respect hide) |

Two practical consequences:

- **A `hide` rule still notifies** if it has `notify` — useful for
  crafting bases you want kept off the ground but announced.
- **`map` is suppressed by `hide`** at render time. A marker on
  something you chose to hide would be contradictory.

---

## 8. With `hide default`, a `notify`-only rule does NOT reveal the item

```
hide default
"X" notify             # NOT shown — visibility is Default → hide_all hides it
"X" show notify        # shown + notify
```

A rule without an explicit `show` has default visibility. Under
`hide default`, default resolves to Hide. Add `show` if you want the
item revealed.

---

## 9. A child rule's `{...}` patterns REPLACE the group's, not merge

```
[unique {All Skills} red notify stat] {
  "Ring$"                                    # inherits {All Skills}
  "Amulet$" {Faster Cast Rate}               # OVERRIDES → only {Faster Cast Rate}
  "Circlet$" {All Skills} {Faster Cast Rate} # repeat both → both required
}
```

If a child rule lists any `{...}`, the group's stat patterns are
dropped for that child. Repeat them on the child line if you want to
keep them. Same field-replacement rule applies to every group attribute
(spec §"Group Merge Rules"), but stats trip people up most often
because the override is silent.

---

## 10. Stat patterns auto-enable stat display in the notification

A rule with any `{stat-pattern}` automatically includes the matched
stat lines in its notification — no explicit `stat` flag needed.
Adding `stat` is harmless but redundant.

---

## 11. Diagnosing "why did this rule fire?"

When the wrong rule wins, do not guess. Two reliable techniques:

**Check the log.** First enable **Verbose filter logging** in the
General tab — it is off by default. Once enabled, every dropped
item is recorded in `d2mxlutils.log` next to the executable with
its displayed name, base item type, quality, tier, and the
matched filter rule (if any). Read it before theorising.

**Bisect with distinct sounds.** When a group of rules is the
suspect, temporarily assign each child a unique sound:

```
[notify] {
  "Mystic Orb"      sound1
  "Heavenly Crate"  sound2
  "Vessel"          sound3
}
```

Reproduce the drop — the sound that plays identifies the culprit
pattern. Faster than re-reading regexes.

**Last-match wins is non-negotiable.** When two rules match, the
later one in source order provides the **entire** decision
(visibility, notify, color, sound, map). Decisions are not merged
across multiple matches. Order rules general → specific, top-down.
