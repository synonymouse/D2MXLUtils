# Loot Filter Specification

## Overview

D2MXLUtils provides a loot filtering system that controls visibility and notifications for dropped items. This document describes the desired behavior and architecture.

## Core Concepts

### Global Filter Mode

A toggle in the editor UI controls the default behavior for items that don't match any rule:

| Mode | `default_show_items` | Behavior |
|------|---------------------|----------|
| Show All | `true` | All items visible by default, rules can hide specific items |
| Hide All | `false` | All items hidden by default, rules must explicitly show items |

### Rule Actions (Flags)

Each rule can specify independent actions:

| Flag | Purpose | Default |
|------|---------|---------|
| `show` | Display item on ground (overrides global hide) | inherited from global |
| `hide` | Hide item from ground (overrides global show) | inherited from global |
| `notify` | Show overlay notification about item drop | `false` |
| `sound` | Play sound (sound1-sound6) | none |
| `color` | Notification text color | default |
| `name` | Include item name in notification | `false` |
| `stat` | Include item stats in notification | `false` |

**Key principle:** `notify` is an **independent flag**. Color and sound do NOT auto-enable notifications.

### Flag Independence

```
"Ring" gold         -> show_item=true, notify=false (color only, no notification)
"Ring" gold notify  -> show_item=true, notify=true  (color + notification)
"Ring" sound1       -> show_item=true, notify=false (sound only, no text notification)
"Ring" notify       -> show_item=true, notify=true  (text notification, no color/sound)
"Ring" hide notify  -> show_item=false, notify=true (hidden but notified)
```

---

## Priority System (D2Stats-style)

When multiple rules match an item, the system uses a **three-level priority** to determine which rule wins:

### Priority Levels (Highest to Lowest)

| Level | Criterion | Description |
|-------|-----------|-------------|
| 1 | **Stat Match** | Rule has `{stat_pattern}` AND pattern matches item stats |
| 2 | **Color Flag** | Rule specifies a color (gold, lime, red, etc.) |
| 3 | **Flag Count** | Rule with more flags is more specific |

### Algorithm

```
1. Collect ALL rules that match the item (name, quality, tier, ethereal checks)
2. For each matching rule, check if stat pattern matches (if present)
3. Prioritize:
   a. If any rule has successful stat match -> that rule WINS
   b. Else if any rule has color flag -> that rule WINS
   c. Else rule with most flags WINS
4. Execute winning rule's action
5. If no rules match -> use global default (show_all/hide_all)
```

### Examples

Given item: "Stone of Jordan" (Unique Ring, +1 to All Skills)

```
Rule A: "." unique gold              # matches: unique, has color
Rule B: "Ring$" lime                 # matches: name pattern, has color
Rule C: "Ring$" unique {Skills} red  # matches: name, quality, AND stat pattern
```

**Result:** Rule C wins (stat match = highest priority)

---

Given item: "Unique Ring" (no +Skills)

```
Rule A: "." unique gold              # matches, 2 flags (unique, gold)
Rule B: "Ring$" lime                 # matches, 1 flag (lime)
Rule C: "Ring$" unique {Skills} red  # does NOT match (no Skills stat)
```

**Result:** Rule A wins (more flags than B, C doesn't match)

---

## Matching Criteria

Rules can filter items by:

| Criterion | DSL Syntax | Description |
|-----------|------------|-------------|
| Name pattern | `"pattern"` | Regex match against item name |
| Stat pattern | `{pattern}` | Regex match against item stats |
| Quality | `unique`, `set`, `rare`, etc. | Item quality level |
| Tier | `sacred`, `angelic`, `master`, `0-4` | MedianXL item tier |
| Ethereal | `eth` | Only ethereal items |

### Quality Values

| Keyword | Value | D2 Quality |
|---------|-------|------------|
| `low` | 1 | Inferior |
| `normal` | 2 | Normal |
| `superior` | 3 | Superior |
| `magic` | 4 | Magic |
| `set` | 5 | Set |
| `rare` | 6 | Rare |
| `unique` | 7 | Unique |
| `craft` | 8 | Crafted |
| `honor` | 9 | Honorific |

### Tier Values (MedianXL)

| Keyword | Value | Description |
|---------|-------|-------------|
| `0`-`4` | 0-4 | Normal tiers |
| `sacred` | 5 | Sacred items |
| `angelic` | 6 | Angelic items |
| `master` | 7 | Mastercrafted items |

---

## Implementation Status

### Completed

- [x] DSL parsing (`rules/dsl.rs`)
- [x] Name pattern matching (regex)
- [x] Stat pattern matching (regex)
- [x] Quality matching
- [x] Ethereal matching
- [x] Color definitions
- [x] Sound flags (sound1-6)
- [x] Display flags (name, stat)
- [x] Profile management

### Required Changes

- [ ] **Explicit `notify` flag** - Add to DSL parser, remove auto-enable logic
- [ ] **Priority system** - Implement multi-level priority in `get_action()`
- [ ] **Global mode toggle** - Add UI control, connect to `default_show_items`
- [ ] **Sound independence** - Sound should not auto-enable notify

### Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/rules/dsl.rs` | Add `notify` flag parsing, remove auto-enable |
| `src-tauri/src/rules/mod.rs` | Implement priority algorithm in `get_action()` |
| `src-tauri/src/rules/matching.rs` | Add stat match result tracking |
| `src/views/LootFilterTab.svelte` | Add global mode toggle UI |

---

## Architecture Notes

### Data Flow

```
DSL Text -> parse_dsl() -> FilterConfig -> MatchContext -> RuleAction
                                              ^
                                              |
                                         ItemDropEvent (from notifier)
```

### FilterConfig Structure

```rust
FilterConfig {
    name: String,
    default_show_items: bool,  // Global mode toggle
    default_notify: bool,      // (likely remove, notify should be explicit)
    rules: Vec<Rule>,
    dsl_source: Option<String>,
}
```

### Rule Structure

```rust
Rule {
    // Matching
    name_pattern: Option<String>,
    stat_pattern: Option<String>,
    item_quality: i32,
    tier: Option<i32>,
    ethereal: i32,

    // Actions
    show_item: bool,
    notify: bool,        // MUST be explicit, not auto-enabled
    color: Option<String>,
    sound: Option<u8>,
    display_name: bool,
    display_stats: bool,
}
```
