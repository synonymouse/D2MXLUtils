# Legacy Filter Migration Reference

Complete mapping between the legacy AutoIt drop-notifier filter and the new
DSL (see `loot-filter-spec.md` and `loot-filter-dsl.md`).

This document is intended to be supplied to an LLM as the sole reference for
converting a legacy `.rules` file. It is exhaustive by design.

---

## 1. Terminology

- **Legacy** — the older AutoIt drop notifier. Rule files are plain text
  with the `.rules` extension, one rule per line.
- **New** — the current DSL. Rule files are also plain text with the
  `.rules` extension but with a different grammar and semantics.
- **Rule line** — one non-blank, non-comment line in a legacy file; one rule
  (optionally inside a group) in the new DSL.

---

## 2. Conversion Algorithm

Apply this procedure to each legacy rule file end-to-end:

1. **Preserve comments.** `#` to end-of-line is a comment in both formats.
   Keep every legacy comment verbatim — they are the author's own annotations
   and often the only context for a rule's intent.

2. **Tokenise each non-comment line.** Legacy format:
   `[ "name-regex" ] [ {stat-regex} ]* flag*` where flags are
   space-separated keywords.

3. **Classify the line's legacy intent** using §4. A line falls into exactly
   one of these buckets:
   - `show` — has the `show` flag.
   - `hide` — has the `hide` flag (and no `show`).
   - `notify` — has at least one flag and no `show`/`hide`.
   - `no-op` — has no flags and no name pattern; drop it.

4. **Rewrite flags** using the tables in §6:
   - Translate 1:1-mapped flags verbatim.
   - Remove `name` (§5.6) and `transparent` (§5.7).
   - If the bucket is `notify` — append `notify` to the rule explicitly
     (§5.1).
   - Multi `{…}` stat groups carry over verbatim — the new DSL
     AND-combines them natively (§5.5).

5. **Adapt name patterns** (§5.4). Legacy patterns match the items.txt base
   type only; the new DSL also matches the runtime display name. Patterns
   usually carry over unchanged, but unique-name patterns that could not
   previously work are now a valid option.

6. **Validate regex dialect** (§5.9). The new DSL uses the Rust `regex`
   crate which rejects lookaround and backreferences. Any pattern containing
   `(?=`, `(?!`, `(?<=`, `(?<!`, or `\1`–`\9` must be rewritten.

7. **Preserve source order**, then reorder for last-match semantics (§5.2).
   The new engine is strict last-match-wins; it has no priority ladder.
   General rules go first, specific rules last.

8. **Do not introduce new-DSL-only features** (`hide default`, groups, `map`)
   unless the user explicitly asks. They are listed in §7 as optional
   improvements — flag them as suggestions, do not apply them silently.

9. **Final pass** — walk the checklist in §9.

---

## 3. Grammar at a Glance

### Legacy

```
line       := [ '"' regex '"' ] ( '{' regex '}' )* ( flag )*
flag       := quality | tier | 'eth' | color | 'show' | 'hide'
            | sound | 'name' | 'stat'
comment    := '#' to end-of-line
```

No groups. No file-scope directives. Each line is evaluated independently.

### New

```
filter       := line*
line         := blank | comment | default_mode | rule | group_open | group_close
default_mode := ('hide' | 'show') 'default'      # at most one, file scope
rule         := [ '"' regex '"' ] ( '{' regex '}' )* flag*
group_open   := '[' flag* ']' '{'
group_close  := '}'
flag         := quality | tier | 'eth' | color | 'show' | 'hide'
              | sound | 'notify' | 'stat' | 'map'
```

Groups (one-level, no nesting) and the `default_mode` directive are new-only.
Multiple `{regex}` stat groups per rule are AND-combined, same as legacy.

---

## 4. Legacy Line Buckets

Legacy resolves visibility by scanning all rules that match the item, then
picking one action according to the first-matching case below:

| Case | Condition across matching rules | Action |
|---|---|---|
| 1 | any rule has **no** `show`/`hide` (a "notify" rule) | leave visibility alone; build notification pool |
| 2 | any rule has `show` | force shown |
| 3 | any rule has `hide` | force hidden (deferred if that rule also has `{stat}`) |
| 4 | any rule has `{stat}` only | build pool but no visibility change |

Important consequence: a "notify" rule that matches an item **suppresses** any
matching `hide` rule on the same item. See §8.2 for the worked example.

When a line has `show`/`hide` and also a color flag, the shared flag slot
(§5.3) means only one of them survives parsing; the last one in the line wins.

Notifications are chosen from the pool with this priority ladder:

1. The rule whose `{stat}` regex actually matched (if any).
2. Otherwise the rule with a color flag.
3. Otherwise the rule with the most flags.
4. Otherwise the last matching rule.

---

## 5. Semantic Differences (critical)

### 5.1 Implicit vs explicit `notify`

- **Legacy.** Any matching rule without `show`/`hide` is implicitly a
  notification source. `unique gold` fires a gold-text overlay notification.
- **New.** Notifications require the explicit `notify` flag. `unique gold`
  colors the ground tooltip only; nothing is emitted to the overlay.

**Conversion rule.** If a legacy line is in bucket `notify` (§4) — that is, it
has flags but no `show`/`hide` — append `notify` to the new rule.

| Legacy                              | New                                      |
|---|---|
| `unique gold`                       | `unique gold notify`                     |
| `set lime sound1`                   | `set lime sound1 notify`                 |
| `"Jordan" unique gold sound1 stat`  | `"Jordan" unique gold sound1 stat notify`|
| `rare {All Skills} stat`            | `rare {All Skills} stat notify`          |
| `unique hide`                       | `unique hide`                            |
| `"Rune$" show`                      | `"Rune$" show`                           |

### 5.2 Priority ladder vs last-match-wins

- **Legacy.** Per-item pool, narrowed by
  `stats-match > color > flag-count > last`.
- **New.** Strict last-match-wins across all rules in source order.

**Conversion rule.** Order rules **general → specific**, top-down. A rule that
was effective in legacy because of the priority ladder has to be physically
placed later than any rule that would otherwise match the same item.

- A `{stat}`-gated rule that was meant to override generic rules: place it
  after them.
- A specific unique rule (e.g. `"Jordan" unique gold notify sound1 stat`)
  that was meant to override `unique gold notify`: place it after.
- A broad `unique hide` later in the file does **not** need the same special
  treatment in legacy (it was suppressed by notification rules anyway) —
  see §8.2.

### 5.3 Visibility slot collisions

- **Legacy.** `show` and `hide` share a single flag slot with all the color
  keywords. Only one of color-or-show-or-hide survives per rule — whichever
  appears last. So `"X" gold hide` is effectively `"X" hide` (the `hide`
  overwrote `gold`).
- **New.** Color and visibility are independent fields; they can coexist.

**Conversion rule.** When both a color keyword and `show`/`hide` appear on a
legacy line, only the rightmost of the two was actually in effect. Drop the
suppressed one.

| Legacy                       | Effective legacy intent | New                          |
|---|---|---|
| `"X" gold hide`              | `"X" hide`              | `"X" hide`                   |
| `"X" hide gold`              | `"X" gold` (+ implicit notify) | `"X" gold notify`     |
| `"X" gold show`              | `"X" show`              | `"X" show`                   |

### 5.4 Name pattern target

- **Legacy.** Regex runs against the items.txt base type name for the item's
  class, with `\n`-separated tier lines joined by `|` and color codes
  stripped. Examples of what the haystack looks like: `"Ring"`, `"Great Axe"`,
  `"Heavy Belt|(Sacred)"`. The runtime display name (e.g. the unique name
  `"Stone of Jordan"` or the rare affix `"Rune Turn"`) is **not** available
  to the pattern.
- **New.** Regex runs against both the runtime display name **and** the
  items.txt base name; an OR-hit on either makes the rule match.

**Conversion rule.** Legacy patterns carry over unchanged. Additionally, the
new DSL allows direct unique-name targeting that was impossible in legacy —
do not introduce it unsolicited, but flag it as an option where relevant
(e.g. a legacy `"Ring$" unique gold` intended to catch Stone of Jordan can
become `"Stone of Jordan" gold notify sound1` in the new DSL).

Legacy tier markers in the haystack (e.g. `|(Sacred)`) are rarely useful —
prefer the `sacred` tier keyword. If a legacy rule uses a literal `\|\(Sacred\)`
regex fragment, rewrite it as the `sacred` tier flag.

### 5.5 Stat patterns: AND-merging

- **Legacy.** Multiple `{…}` groups per line are AND-merged. Every pattern
  must match the item's stat text.
- **New.** Same. Multiple `{…}` on a rule are AND-combined against the
  item's stat blob.

**Conversion rule.** Patterns carry over verbatim. No rewriting needed:

| Legacy                                           | New                                                      |
|---|---|
| `"Ring$" {Skills}{focus}{enemy cold} rare stat`  | `"Ring$" {Skills} {focus} {enemy cold} rare stat notify` |
| `unique {All Skills}{Faster Cast Rate}`          | `unique {All Skills} {Faster Cast Rate} notify`          |

Whitespace between `{…}` groups is optional — `{a}{b}` and `{a} {b}`
parse identically. Prefer spaces for readability.

Use regex alternation inside a single `{…}` when OR is what you want:
`{(Fire|Cold|Lightning) Resist}`. That still works exactly as in legacy.

### 5.6 `name` flag removed

- **Legacy.** The per-rule `name` flag forced the unique/set name line to be
  rendered above the base type in the notification.
- **New.** No per-rule control. A global *Compact name* notification setting
  governs whether the unique/set name line is shown; the per-rule `stat` flag
  forces the full two-line header regardless of that setting.

**Conversion rule.** Drop every occurrence of `name`. If the intent was to
force the name line, leave a comment (`# legacy 'name' flag dropped`) so the
user can flip the global setting if they want.

### 5.7 `transparent` color removed

- **Legacy.** `transparent` was a valid color keyword that rendered
  identically to no-color — effectively a no-op.
- **New.** No such keyword.

**Conversion rule.** Drop `transparent` silently.

### 5.8 Default visibility of unmatched items

- **Legacy.** Every scanned item is force-shown before rule matching. An
  item the game's built-in filter would otherwise hide is still displayed.
- **New.** For an unmatched item (or a matched rule with no `show`/`hide`)
  visibility is left to the game's built-in filter — the engine writes
  nothing.

**Conversion implication.** A legacy filter that relies on "everything is
shown unless I hide it" may silently hide a few item categories under the
game's default filter. If exact parity is required, add a broad terminal
rule like `show` (matches everything) or whitelist specific name patterns.
Flag this as a question only if the legacy file visibly depends on catch-all
visibility; otherwise leave the new defaults alone.

### 5.9 Regex dialect

- **Legacy.** AutoIt's regex engine is PCRE — supports lookaround,
  backreferences, conditionals.
- **New.** Rust `regex` crate (1.x). Patterns are evaluated
  case-insensitively. Missing features:
  - No lookaround (`(?=`, `(?!`, `(?<=`, `(?<!`).
  - No backreferences (`\1`, `\k<name>`).
  - Invalid patterns fall back to case-insensitive substring match.

The new DSL re-references this dialect everywhere regex appears — the
`"name"` pattern and the `{stat}` pattern share it.

**Conversion rule.** Scan every regex (in `"…"` and `{…}` alike) for these
constructs. Rewrite:
- `A(?=B)` (A followed by B) → `AB` if matching the combined span is fine.
- `(?<=A)B` → reword the pattern to include A before B.
- For AND-across-stats, prefer multiple `{…}` groups (§5.5) over a single
  `(?=.*a)(?=.*b)` regex — the DSL handles AND natively.

---

## 6. Keyword Mapping Tables

### 6.1 Quality

All keywords map 1:1.

| Legacy     | New        |
|---|---|
| `low`      | `low`      |
| `normal`   | `normal`   |
| `superior` | `superior` |
| `magic`    | `magic`    |
| `set`      | `set`      |
| `rare`     | `rare`     |
| `unique`   | `unique`   |
| `craft`    | `craft`    |
| `honor`    | `honor`    |

Multiple quality keywords on one rule OR-combine on both sides.

### 6.2 Tier

All keywords map 1:1.

| Legacy    | New        |
|---|---|
| `0`       | `0`        |
| `1`       | `1`        |
| `2`       | `2`        |
| `3`       | `3`        |
| `4`       | `4`        |
| `sacred`  | `sacred`   |
| `angelic` | `angelic`  |
| `master`  | `master`   |

Tier `0` means non-equipment or tier-0 equipment (items without a tier
suffix in their base name). Multi-tier on one rule OR-combines on both sides.

### 6.3 Ethereal

| Legacy | New  |
|---|---|
| `eth`  | `eth` |

### 6.4 Colors

| Legacy        | New          |
|---|---|
| `transparent` | **drop**     |
| `white`       | `white`      |
| `red`         | `red`        |
| `lime`        | `lime`       |
| `blue`        | `blue`       |
| `gold`        | `gold`       |
| `grey`        | `grey`       |
| `black`       | `black`      |
| `pink`        | `pink`       |
| `orange`      | `orange`     |
| `yellow`      | `yellow`     |
| `green`       | `green`      |
| `purple`      | `purple`     |

`show` and `hide` also live in this flag slot in legacy (§5.3) but are
separate fields in the new DSL.

### 6.5 Visibility

| Legacy | New   |
|---|---|
| `show` | `show` |
| `hide` | `hide` |

### 6.6 Sound

| Legacy       | New          |
|---|---|
| `sound1`     | `sound1`     |
| `sound2`     | `sound2`     |
| `sound3`     | `sound3`     |
| `sound4`     | `sound4`     |
| `sound5`     | `sound5`     |
| `sound6`     | `sound6`     |
| `sound7`     | `sound7`     |
| `sound_none` | `sound_none` |

### 6.7 Display flags

| Legacy | New                                                               |
|---|---|
| `name` | **drop** — governed globally by *Compact name* setting (§5.6)     |
| `stat` | `stat`                                                            |

### 6.8 Synthetic flag added during conversion

| Condition                                    | Add to new rule |
|---|---|
| Legacy line had flags but no `show`/`hide`   | `notify`        |

---

## 7. Optional New-Only Features

Do not introduce these silently. List them as suggestions after the straight
translation is done.

### 7.1 `hide default` / `show default`

`hide default` turns the filter into a whitelist: unmatched items are
hidden. Legacy had no clean equivalent — the typical workaround was a
stack of broad per-quality `hide` rules (`normal low magic hide`, etc.)
with explicit `show` rules for the whitelist. Candidate trigger: the
legacy file begins with several broad `hide` rules covering most
qualities.

### 7.2 Groups `[attrs] { … }`

A group shares attributes across several rules. Use when the legacy file has
a run of lines differing only in the name pattern:

```
# Legacy
"Jordan" unique gold sound1
"Tyrael" unique gold sound1
"Windforce" unique gold sound1
```
→
```
# New, equivalent (notify added per §5.1)
[unique gold notify sound1] {
  "Jordan"
  "Tyrael"
  "Windforce"
}
```

Groups flatten to the same rules at parse time; last-match semantics are
preserved.

### 7.3 `map` flag

Drops a red cross on the in-game automap at the item's position. No legacy
equivalent. Offer it for chase items or broad low-noise filters.

---

## 8. Worked Examples

### 8.1 Simple color-only rule — add `notify`

**Legacy**
```
unique gold
set lime
```

**New**
```
unique gold notify
set lime notify
```

Why: bucket `notify` in §4 → §5.1 adds `notify`.

### 8.2 Legacy quirk — notify suppresses hide

**Legacy**
```
unique gold
unique hide
```

Legacy outcome: item is **shown** with a gold-text notification. The
`unique gold` line puts the item in bucket 1 of §4, which short-circuits
the `hide` action.

**Direct translation would be wrong** — the new DSL is last-match, so
`unique hide` would win and hide the item with no notification.

**Semantically equivalent new**
```
unique gold notify
```

Drop the `hide` line; it never took effect in legacy. If the user actually
wanted uniques hidden, they should say so — flag this as a question during
the interactive conversion.

### 8.3 Single stat group

**Legacy**
```
rare {All Skills} purple stat
```

**New**
```
rare {All Skills} purple stat notify
```

### 8.4 Multi-stat AND — native

**Legacy**
```
unique {All Skills} {Faster Cast Rate} red stat
```

Legacy intent: unique with BOTH "All Skills" and "Faster Cast Rate".

**New — verbatim (same AND semantics)**
```
unique {All Skills} {Faster Cast Rate} red stat notify
```

Order of the `{…}` groups is irrelevant — each pattern is matched
independently against the item's stat blob. Every matching line is
highlighted in the notification.

### 8.5 Priority ladder → explicit ordering

**Legacy**
```
unique gold                                  # broad: every unique, gold notification
"Jordan" unique gold sound1 stat             # specific: SoJ with sound + stats
unique {All Skills} red stat                 # stat-gated: any unique with All Skills, red
```

All three are notify-bucket rules (no `show`/`hide`). For a unique SoJ that
rolls "All Skills", all three match. Legacy's priority ladder picks one:
the stat-gated rule first (priority 1, because the `{…}` regex actually
matched), else the rule with a specific color (priority 2), else the rule
with more flags (priority 3), else last.

**New — reorder so the intended winner is last**
```
unique gold notify
unique {All Skills} red stat notify
"Jordan" unique gold notify sound1 stat
```

For a unique SoJ that rolls "All Skills", the third rule (being last)
wins, yielding gold text + sound + stats. For a non-SoJ unique with "All
Skills", the second rule wins — red text + stats. For any other unique,
the first rule wins — plain gold notification.

### 8.6 Show/hide collision

**Legacy**
```
"Rune$" orange hide
```

`orange` was overwritten by `hide` in the same flag slot (§5.3). Effective
legacy intent: hide runes.

**New**
```
"Rune$" hide
```

### 8.7 Name-only pattern (targeting a specific unique)

**Legacy — impossible to write directly**
```
"Ring$" unique gold stat          # matches Stone of Jordan, but also every other unique ring
```

**New — direct unique-name match is now valid**
```
"Stone of Jordan" gold stat notify sound1
```

Flag as a suggestion; do not apply unsolicited.

### 8.8 No-op line (drop it)

**Legacy**
```
normal
```

No flags, nothing to emit. Omit from the new file.

---

## 9. Conversion Checklist

Walk this list before handing the converted file back.

- [ ] Every "notify" bucket rule (§4) has `notify` appended.
- [ ] `name` removed everywhere.
- [ ] `transparent` removed everywhere.
- [ ] Color + `show`/`hide` collisions resolved per §5.3 (rightmost wins in
      legacy).
- [ ] Multi-group `{a}{b}…` carried over verbatim — AND semantics are native in the new DSL (§5.5).
- [ ] Regexes contain no lookaround or backreferences.
- [ ] Rule order reviewed for last-match-wins intent (general → specific).
- [ ] Legacy suppress-by-notify quirks (§8.2) resolved in consultation with
      the user, not silently preserved.
- [ ] Comments from the legacy file retained.
- [ ] New-only features (`hide default`, groups, `map`) offered as
      suggestions, not applied unsolicited.
