# Flag Combination Logic

When a rule lists several flags (e.g. `1 2 3 4 low normal superior hide` or
`sacred superior magic rare hide`), the parser sorts every token into a
*category* and applies a uniform rule:

> **OR within a category, AND across categories.**

This document explains exactly how a rule's tokens combine into a match
predicate, so you can reason about which items a given rule will actually
affect.

---

## Categories

Each whitespace-separated token in a rule line falls into one of these
categories:

| Category | Tokens | Combine within | Empty list means |
|---|---|---|---|
| **Tier** | `0` `1` `2` `3` `4` `sacred` `angelic` `master` (`mastercrafted`) | OR | any tier |
| **Quality** | `low`/`inferior` `normal` `superior` `magic` `set` `rare` `unique` `craft`/`crafted` `honor`/`honorific` | OR | any quality |
| **Ethereal** | `eth` | — | not required |
| **Name** | `"regex"` (at most one, leading) | regex | any name |
| **Stat** | `{regex}` (zero or more, anywhere) | AND | no stat constraint |
| **Action** | `show` / `hide` | — | inherit / default |
| **Notification** | `notify`, color, `sound1..7`, `sound_none`, `stat`, `map` | — | independent flags |

Duplicate tokens within a category are collapsed silently (`1 1 2 hide` is
equivalent to `1 2 hide`).

---

## How a rule decides whether an item matches

For every dropped item, all of the following must hold:

1. **Tier** — if the rule lists tiers, the item's tier must be one of them.
   If the rule lists no tiers, this check is skipped.
2. **Quality** — if the rule lists qualities, the item's quality must be one
   of them. If the rule lists no qualities, this check is skipped.
3. **Ethereal** — if `eth` is present, the item must be ethereal. Otherwise
   ethereal status is irrelevant.
4. **Name pattern** — if a `"regex"` is present, it must hit the runtime
   display name, the items.txt base type, or the class category.
5. **Stat patterns** — every `{regex}` listed must match somewhere in the
   item's stat blob.

Only when all five checks pass does the rule fire and contribute its
visibility / notification / map flags.

This is implemented in `src-tauri/src/rules/matching.rs:31-59`.

---

## Worked examples

### `1 2 3 4 low normal superior hide`

| Category | Set |
|---|---|
| Tier | `{1, 2, 3, 4}` |
| Quality | `{inferior, normal, superior}` |
| Visibility | `hide` |

**Predicate:** *tier ∈ {1..4}* **AND** *quality ∈ {low, normal, superior}*.

| Example item | Tier OK? | Quality OK? | Hidden? |
|---|---|---|---|
| Tier-2 normal sash | yes | yes | **yes** |
| Tier-3 superior helm | yes | yes | **yes** |
| Tier-1 magic ring | yes | no | no |
| Tier-1 unique ring | yes | no | no |
| Sacred superior helm | no | yes | no |
| Tier-0 rune | no (T0 ∉ {1..4}) | yes (normal rune) | no |

### `sacred superior magic rare hide`

| Category | Set |
|---|---|
| Tier | `{sacred}` |
| Quality | `{superior, magic, rare}` |
| Visibility | `hide` |

**Predicate:** *tier = sacred* **AND** *quality ∈ {superior, magic, rare}*.

Sacred uniques and Sacred sets are not hidden (quality not in the set).
A Tier-4 magic item is not hidden (tier not in the set).

### `1 2 3 4 unique hide`

Hides only Tier 1–4 uniques. Sacred uniques and Tier 1–4 magic items are
unaffected.

### `1 2 3 4 hide`

No quality listed → quality check is skipped → hides **all** Tier 1–4 items
regardless of quality.

### `unique hide`

No tier listed → tier check is skipped → hides **all** uniques at every
tier.

### `eth unique sacred hide`

Hides only Sacred ethereal uniques (all three constraints AND'd).

### `"Ring$" rare {All Skills} {Faster Cast}`

A rule with no quality/tier still has constraints: the name must match
`Ring$` *and* both stat patterns must hit. Quality and tier are unrestricted.

---

## Why the same word can't appear in two categories

Tokens are looked up in this order: quality → tier → ethereal/visibility/etc.
→ color → sound. Each token belongs to exactly one category, so there is no
ambiguity in `magic` (quality) vs `master` (tier) vs `gold` (color).

---

## Interaction with the rest of the spec

- **Last-match wins.** A later rule in the file that also matches replaces
  the earlier rule's outcome (`src-tauri/src/rules/mod.rs:253`). So a broad
  `1 2 3 4 ... hide` near the end of a profile can hide items that an
  earlier `unique show gold notify` had revealed, unless the later rule
  excludes uniques from its quality set.
- **`hide default` directive.** Sets the file-wide fallback to hide. Rules
  with explicit `show` still reveal items; rules with `hide` are redundant
  but harmless.
- **Group headers.** A `[…] { … }` header is parsed as a rule with no name
  pattern. Each child rule inherits any unset header field, so
  `[1 2 3 4] { magic hide; rare hide }` filters magic-or-rare items at
  Tier 1–4 — the tier set is inherited, the quality set is per-child.
- **Within a group, child overrides header.** If both header and child set
  the same category (e.g. both list qualities), the child's list wins
  outright; it does not merge.

---

## Common pitfalls

These are the mistakes users make most often when they haven't internalized
the OR-within / AND-across model. In each case the rule is syntactically
valid but does **not** filter what the author intended.

### 1. "I want to hide low-tier junk *or* low-quality junk"

```
1 2 3 4 low normal superior hide
```

**Expectation:** hides anything that is either Tier 1–4 *or* low/normal/superior.

**Reality:** hides only items that are **both** Tier 1–4 **and**
low/normal/superior. A Sacred normal helm and a Tier-2 magic ring are
both untouched.

**Fix:** split into two rules.

```
1 2 3 4 hide
low normal superior hide
```

### 2. Last-match-wins only matters when two rules match the *same* item

```
sacred low normal superior magic hide
sacred unique notify map
```

This pair works exactly as the author expects: the first rule hides sacred
items of low/normal/superior/magic quality; the second highlights sacred
uniques. The two predicates are **disjoint** — no sacred item is both
"magic" and "unique" — so last-match-wins never fires. Each item is
decided by the only rule that matches it.

The pitfall appears when the rule sets *do* overlap:

```
sacred unique gold notify        # highlight sacred uniques
sacred show                       # "just show all sacred"
```

`sacred show` matches every sacred item, **including** sacred uniques,
so its predicate is a superset of the first rule's. Because it comes
later, last-match-wins discards the `gold notify` for sacred uniques and
they end up silently shown like the rest.

**Rule of thumb:** when you write a new rule, ask "does its predicate
overlap with any earlier rule's predicate?". If yes, the new rule wins
for the overlap. If no (as in your example above), source order is
irrelevant.

**Fix when overlap is unwanted:** put the narrow, decorated rule **after**
the broad one, or exclude the overlap from the broad rule:

```
sacred show                       # broad
sacred unique gold notify         # narrow, wins for sacred uniques
```

### 3. Rules don't accumulate — later matches replace earlier ones

```
unique gold notify
set lime notify
```

**Expectation:** uniques get gold, sets get lime — both popups stay.

**Reality:** correct in isolation, because the predicates are disjoint.
But if you add a third rule

```
"Ring$" red notify
```

it matches both unique rings *and* set rings, and last-match-wins
overwrites the gold/lime colors with red. Users assume per-flag merging
("the gold from rule 1 plus the red from rule 3"); the engine instead
picks **one** winning rule and uses *its* flags wholesale.

**Fix:** put narrow rules **after** broad rules, and accept that the
winner's flags fully replace earlier ones. If you want some flags from
rule A and others from rule B, you have to write the combined rule
explicitly.

### 4. "Hide all magic items below Sacred"

```
magic 1 2 3 4 hide
```

**Expectation:** hides magic items at Tier 1–4.

**Reality:** that's exactly what it does — but Tier-0 magic items
(jewels, charms, etc.) are **not** hidden because `0` isn't in the tier
list. Users often forget Tier-0 exists as a distinct tier.

**Fix:** add `0` to the tier list, or omit the tier list entirely if you
mean "all tiers".

```
magic 0 1 2 3 4 hide
# or
magic hide
```

### 5. "I added `hide` to a rule with notifications and now nothing pops up"

```
"Stone of Jordan" unique gold notify hide
```

**Expectation:** hide the item icon on the ground but still pop a
notification.

**Reality:** when the rule resolves to `hide`, the notification still
fires (notify is independent), but if the user *also* has `hide default`
and a later broader rule re-matches without `notify`, the notification is
lost. More commonly, users write `hide` thinking it means "hide from
overlay" and end up suppressing the floor label they wanted.

**Fix:** decide which you mean. `hide` = remove the floor label. Use
`notify` for the popup. Don't combine them unless you genuinely want both.

### 6. "Why does `magic hide` also hide my rare items?"

It doesn't — but users are sometimes confused by what happens after
`hide default`:

```
hide default
magic show
```

**Expectation:** show magic, hide everything else (including normal/rare).

**Reality:** correct — but rare items are now hidden *because of
`hide default`*, not because of the `magic show` rule. If the user then
removes `magic show` thinking they're "un-hiding magic", they actually
un-hide nothing — `hide default` is still active.

**Fix:** remember that `hide default` is a file-wide switch independent
of any single rule.

### 7. "I want sacred uniques *or* angelic sets"

```
sacred angelic unique set show gold notify
```

**Expectation:** sacred uniques OR angelic sets.

**Reality:** matches **{sacred, angelic} × {unique, set}** — that's four
combinations including sacred sets and angelic uniques, which the user
didn't ask for.

**Fix:** two rules.

```
sacred unique show gold notify
angelic set show gold notify
```

### 8. "I want any rare ring with All Skills *or* FCR"

```
"Ring$" rare {All Skills} {Faster Cast Rate} notify
```

**Expectation:** rare rings with either stat.

**Reality:** rare rings with **both** stats. Stat patterns are AND'd.

**Fix:** use regex alternation inside one `{…}`.

```
"Ring$" rare {All Skills|Faster Cast Rate} notify
```

### 9. "Why does my `eth` rule miss non-ethereal items I wanted to keep?"

```
eth hide
```

**Expectation:** hide ethereal junk while leaving normal items alone.

**Reality:** correct in isolation — but users sometimes write `eth show`
under `hide default` expecting it to *also* show non-ethereal items of the
same type. `eth` only narrows; it never broadens.

**Fix:** if you want both, write two rules or omit `eth`.

### 10. Name pattern must come first on the line

```
unique set "Ring$" gold notify
```

**Expectation:** any unique-or-set ring, highlighted gold.

**Reality:** the linter flags `"Ring$"` as `Unknown flag`. The grammar is
`rule := [name] attr*` — a quoted name pattern is recognized **only as
the leading token** of a rule line. In the middle of the token list it's
just an unknown word.

The same constraint applies inside a group body: each child rule starts
with its own optional name, then attributes. Group **headers** can't
contain a name pattern at all.

**Fix:** move the quoted pattern to the front.

```
"Ring$" unique set gold notify
```

### 11. "Group with hide and a single override doesn't notify"

```
[hide] {
  unique gold sound1
}
```

**Expectation:** hide everything in the group but still ding+highlight uniques.

**Reality:** the unique rule inherits `hide` from the header (no `show`
to override) and has no `notify`, so:
1. The unique is hidden.
2. No notification fires (color/sound without `notify` does nothing).
The user sees neither the floor label nor the popup.

**Fix:** add `show` and `notify` to the override.

```
[hide] {
  unique show gold sound1 notify
}
```

---

## Mental model summary

Think of each rule as a single conjunction of category predicates:

```
match(item) =
    (tiers.is_empty()       || item.tier     ∈ tiers)
 && (qualities.is_empty()   || item.quality  ∈ qualities)
 && (!eth_required          || item.is_ethereal)
 && (name_pattern.is_none() || name_regex.matches(item))
 && stat_patterns.all(pat -> pat.matches(item.stats))
```

If you want OR semantics across categories (e.g. "any unique, OR any sacred
item"), write **two separate rules** — `unique show` then `sacred show` —
not one combined rule. There is no within-rule OR between categories.
