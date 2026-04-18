# Loot Filter Specification

## Overview

D2MXLUtils provides a loot filter that controls two things for every ground item:

1. **Visibility** — whether the tooltip is drawn on the ground.
2. **Notification** — whether an overlay alert (text, color, sound) is emitted.

These two decisions are independent. Filter behavior is described by a text DSL (see `loot-filter-dsl.md`).

---

## Core Concepts

### Hide All checkbox

A single checkbox in the editor UI controls the default visibility for items that are not forced by a rule:

| Hide All | Default visibility | Meaning |
|---|---|---|
| off | inherited from game | Game's built-in loot filter decides. Rules can override with `show` or `hide`. |
| on  | hidden              | Only rules with `show` reveal items. Rules without `show` do not reveal. |

There is no separate "Show All" mode — it is just "Hide All = off".

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
[name-pattern] [quality] [tier] [eth] [{stat-pattern}] [color] [show|hide] [sound] [notify] [name] [stat]
```

All components are optional. A line with zero attributes is valid but is a no-op (matches everything, does nothing).

---

## Matching Criteria

A rule matches an item when **all** specified criteria are satisfied.

| Criterion | DSL | Condition |
|---|---|---|
| Name | `"regex"` | item name matches regex, case-insensitive |
| Stat | `{regex}` | item stat text matches regex, case-insensitive |
| Quality | `unique`, `set`, `rare`, `magic`, `craft`, `honor`, `normal`, `superior`, `low` | item quality equals keyword |
| Tier | `0`–`4`, `sacred`, `angelic`, `master` | MedianXL item tier equals keyword |
| Ethereal | `eth` | item is ethereal |

Invalid regex falls back to plain substring matching.

---

## Visibility

The winning rule's visibility flag plus the Hide All checkbox determine the outcome:

| Hide All | Winner flag | Result |
|---|---|---|
| off | none  | game decides |
| off | `show` | shown (overrides game's hide) |
| off | `hide` | hidden |
| on  | none  | hidden |
| on  | `show` | shown |
| on  | `hide` | hidden |

If no rule matches:
- Hide All off → game decides.
- Hide All on  → hidden.

---

## Notification

Notifications fire **only** when the winning rule has the `notify` flag.

A fired notification uses the winning rule's:

- `color` — text color (or default if absent)
- `sound` — sound index 1–6 (silent if absent)
- `name` — include item name if set
- `stat` — include item stats if set

`color`, `sound`, `name`, `stat` alone do **not** imply `notify`.

| Rule | Visibility | Notification |
|---|---|---|
| `unique gold` | game decides | none |
| `unique gold notify` | game decides | gold text |
| `unique gold sound1` | game decides | none |
| `unique gold notify sound1` | game decides | gold text + sound |
| `normal hide` | hidden | none |
| `normal hide notify sound1` | hidden | text + sound |

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
    name: String,
    hide_all: bool,
    rules: Vec<Rule>,          // flattened, groups expanded
    dsl_source: Option<String>,
}

enum Visibility { Default, Show, Hide }

Rule {
    // matching
    name_pattern: Option<String>,
    stat_pattern: Option<String>,
    quality:      Option<Quality>,
    tier:         Option<Tier>,
    ethereal:     bool,

    // actions
    visibility:    Visibility,
    color:         Option<Color>,
    sound:         Option<u8>,
    notify:        bool,
    display_name:  bool,
    display_stats: bool,

    source_line: Option<String>,
}
```

---

## Group Merge Rules

When flattening a group into its rules, for each contained rule:

1. Each field not set on the rule takes the group's value.
2. Each field set on the rule keeps the rule's value (overrides group).
3. `visibility` is one field — `show` on a rule replaces `hide` from a group, and vice versa.
4. `stat_pattern` on a rule replaces the group's `stat_pattern` entirely (no AND-merge). Combine via regex if needed.

After flattening, rules keep their original source-order position for the last-match algorithm.

---

## Out of Scope

- Ethereal "forbidden" mode (only `eth` = required is supported).
- Item level (`ilvl`) and character level (`clvl`) filtering.
- Multiple stat patterns per rule (use one regex with alternation).
- Nested groups.
