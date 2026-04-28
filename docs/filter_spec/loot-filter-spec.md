# Loot Filter Specification

## Overview

D2MXLUtils provides a loot filter that controls two things for every ground item:

1. **Visibility** — whether the tooltip is drawn on the ground.
2. **Notification** — whether an overlay alert (text, color, sound) is emitted.

These two decisions are independent. Filter behavior is described by a text DSL (see `loot-filter-dsl.md`).

---

## Core Concepts

### Default mode directive

A file-scope directive controls the default visibility for items that are not forced by a rule:

```
hide default      # hide unmatched items
show default      # show unmatched items (implicit default if the directive is absent)
```

| Directive | Default visibility | Meaning |
|---|---|---|
| `show default` (or absent) | inherited from game | Game's built-in loot filter decides. Rules can override with `show` or `hide`. |
| `hide default`             | hidden              | Only rules with `show` reveal items. Rules without `show` do not reveal. |

The directive may appear at most once per file, at file scope only (never inside a group). The editor shows a read-only indicator reflecting the current mode.

### Last-match wins

When several rules match the same item, the **last matching rule in source order** wins. The winning rule provides the complete outcome (visibility + notification attributes). Rules are not merged across matches.

There is no priority system based on flag count, color presence, or stat-match. Order rules from general to specific, top-down.

### Groups

Rules can share attributes through a group block:

```
[unique gold notify sound1] {
  "Jordan"
  "Tyrael"
  "Windforce"
}
```

At parse time, group attributes are merged into each contained rule. Rule-level attributes override the group's attributes for the same field. Groups cannot be nested.

A group header accepts all rule attributes **except a name pattern**. Name patterns stay per-rule.

---

## Rule Anatomy

```
[name-pattern] [quality] [tier] [eth] [{stat-pattern}]* [color] [show|hide] [sound] [notify] [stat] [map]
```

All components are optional. A line with zero attributes is valid but is a no-op (matches everything, does nothing).

---

## Matching Criteria

A rule matches an item when **all** specified criteria are satisfied.

| Criterion | DSL | Condition |
|---|---|---|
| Name | `"regex"` | regex matches either the runtime display name or the items.txt base type name (case-insensitive OR) |
| Stat | `{regex}...` | every listed regex matches the item stat text (AND), case-insensitive |
| Quality | `unique`, `set`, `rare`, `magic`, `craft`, `honor`, `normal`, `superior`, `low` | item quality equals one of the listed keywords (OR) |
| Tier | `0`–`4`, `sacred`, `angelic`, `master` | MedianXL item tier equals one of the listed keywords (OR) |
| Sockets | `sockets0`–`sockets6` | item socket count equals one of the listed numbers (OR) |
| Ethereal | `eth` | item is ethereal |

Quality, tier, and sockets each accept multiple keywords in a single rule;
the rule matches if the item's value equals any of the listed ones. A rule
with no keyword in a category matches any value in that category.

Invalid regex falls back to plain substring matching.

---

## Visibility

The winning rule's visibility flag plus the default-mode directive determine the outcome:

| Default mode    | Winner flag | Result |
|---|---|---|
| `show default`  | none   | game decides |
| `show default`  | `show` | shown (overrides game's hide) |
| `show default`  | `hide` | hidden |
| `hide default`  | none   | hidden |
| `hide default`  | `show` | shown |
| `hide default`  | `hide` | hidden |

If no rule matches:
- `show default` (or absent directive) → game decides.
- `hide default` → hidden.

---

## Notification

Notifications fire **only** when the winning rule has the `notify` flag.

A fired notification uses the winning rule's:

- `color` — text color (or default if absent)
- `sound` — sound index 1–6 (silent if absent)
- `stat` — include item stats if set

`color`, `sound`, `stat` alone do **not** imply `notify`.

The unique/set name line shown above the base type for Set/TU/SU/SSU/SSSU drops is governed by the **Compact name** notification setting, not by a per-rule flag. When a rule has the `stat` flag, the full two-line header is forced regardless of the Compact name setting.

| Rule | Visibility | Notification |
|---|---|---|
| `unique gold` | game decides | none |
| `unique gold notify` | game decides | gold text |
| `unique gold sound1` | game decides | none |
| `unique gold notify sound1` | game decides | gold text + sound |
| `normal hide` | hidden | none |
| `normal hide notify sound1` | hidden | text + sound |

---

## Map Marker

A rule tagged with `map` drops a red-cross marker on the in-game automap at the matched item's world position.

- Independent of `notify` — silent map pings are supported. A rule with `map` only places the marker without firing an overlay notification.
- Marker placement is skipped for items resolved to `hide` (displaying a cross for something you chose to hide would be contradictory).
- Markers auto-clear on area change (the engine reclaims the entire automap layer). Within an area they're rebuilt as items drop or are picked up.
- BFS scan range covers up to 10 rooms outward from the player — effectively "as far as the engine loads rooms".
- **Markers are sticky within an area**: walking past a marked item keeps the marker on the map even after the room unloads. It returns to the visible automap when you come back. A marker is only removed when the item is picked up (detected by heuristic: missing from BFS while the player is within ~32 subtiles of the marker's position).

| Rule | Marker | Notification |
|---|---|---|
| `unique map` | red cross at drop | none |
| `"Stone of Jordan" unique notify map` | red cross + gold overlay | yes |
| `[sssu map] { . }` | red cross on every SSSU | none |

---

## Evaluation Algorithm

```
decide(item, rules, hide_all):
    winner = None
    for rule in rules:            # rules are already flattened from groups
        if rule.matches(item):
            winner = rule         # last match wins

    if winner is None:
        visibility = HIDDEN if hide_all else GAME_DEFAULT
        notification = None
        return (visibility, notification)

    visibility = resolve_visibility(winner.visibility, hide_all)
    notification = build_notification(winner) if winner.notify else None
    return (visibility, notification)
```

---

## Data Structures

```rust
FilterConfig {
    name: String,       // runtime-only, derived from profile filename
    hide_all: bool,     // set by the `hide default` / `show default` directive
    rules: Vec<Rule>,   // flattened, groups expanded
}

enum Visibility { Default, Show, Hide }

Rule {
    // matching
    name_pattern:  Option<String>,
    stat_patterns: Vec<String>,   // empty = any; non-empty = AND-match (all must hit)
    qualities:     Vec<Quality>,  // empty = any; non-empty = OR-match
    tiers:         Vec<Tier>,     // empty = any; non-empty = OR-match
    ethereal:      bool,

    // actions
    visibility:    Visibility,
    color:         Option<Color>,
    sound:         Option<u8>,
    notify:        bool,
    display_stats: bool,
    map:           bool,   // drop an automap marker at the item's position
}
```

Profiles are persisted as plain `.rules` DSL text — there is no intermediate JSON form. The filename stem is the profile name.

---

## Group Merge Rules

When flattening a group into its rules, for each contained rule:

1. Each field not set on the rule takes the group's value.
2. Each field set on the rule keeps the rule's value (overrides group).
3. `visibility` is one field — `show` on a rule replaces `hide` from a group, and vice versa.
4. `stat_patterns` on a rule replace the group's `stat_patterns` entirely (override, same as every other field). If a child rule has any `{…}`, the group's patterns are dropped for that child — repeat them on the child line to keep them. Use regex alternation inside one `{…}` for OR.

After flattening, rules keep their original source-order position for the last-match algorithm.

---

## Out of Scope

- Ethereal "forbidden" mode (only `eth` = required is supported).
- Item level (`ilvl`) and character level (`clvl`) filtering.
- Nested groups.
