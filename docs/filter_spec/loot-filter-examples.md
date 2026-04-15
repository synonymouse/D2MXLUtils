# Loot Filter Examples and Complex Combinations

This document analyzes various rule combinations and edge cases to understand the expected behavior of the loot filter system.

---

## Test Scenarios

### Scenario 1: Basic Priority Resolution

**Rules:**
```
A: "." unique gold
B: "Ring$" lime
C: "Ring$" unique {Skills} red notify sound1
```

**Test Items:**

| Item | Matches | Winner | Reason |
|------|---------|--------|--------|
| Unique Ring +Skills | A, B, C | **C** | Stat match = highest priority |
| Unique Ring (no skills) | A, B | **A** | Both have color, A has more flags (2 vs 1) |
| Rare Ring | B | **B** | Only B matches |
| Unique Amulet | A | **A** | Only A matches |

---

### Scenario 2: Stat Match Always Wins

**Rules:**
```
A: "." unique gold notify sound1 name stat    # 5 flags, no stat pattern
B: "Ring$" {Resist} lime                      # 2 flags, has stat pattern
```

**Test Items:**

| Item | Matches | Winner | Reason |
|------|---------|--------|--------|
| Unique Ring +Resist | A, B | **B** | Stat match beats flag count |
| Unique Ring (no resist) | A | **A** | B's stat pattern fails |
| Rare Ring +Resist | B | **B** | Only B matches |

---

### Scenario 3: Color vs No Color

**Rules:**
```
A: "." unique                    # 1 flag, no color
B: "Ring$" lime                  # 1 flag, has color
```

**Test Items:**

| Item | Matches | Winner | Reason |
|------|---------|--------|--------|
| Unique Ring | A, B | **B** | Color flag = priority level 2 |
| Unique Amulet | A | **A** | Only A matches |

---

### Scenario 4: Flag Count Tiebreaker

**Rules:**
```
A: "." unique gold               # 2 flags
B: "." set gold                  # 2 flags
C: "Ring$" unique gold notify    # 3 flags
```

**Test Items:**

| Item | Matches | Winner | Reason |
|------|---------|--------|--------|
| Unique Ring | A, C | **C** | More flags (3 > 2) |
| Unique Amulet | A | **A** | Only A matches |
| Set Ring | B, (not C) | **B** | Only B matches for Set |

---

### Scenario 5: Show/Hide with Global Modes

**Global Mode: Show All**

```
A: "." normal hide
B: "." magic hide
C: "." unique gold notify
```

| Item | Action | Notification |
|------|--------|--------------|
| Normal Sword | Hidden | No |
| Magic Ring | Hidden | No |
| Unique Ring | Shown (gold) | Yes |
| Rare Amulet | Shown (default) | No |

**Global Mode: Hide All**

```
A: "." unique show gold notify
B: "." sacred show lime notify
C: "Rune$" show orange
```

| Item | Action | Notification |
|------|--------|--------------|
| Unique Ring | Shown (gold) | Yes |
| Sacred Armor | Shown (lime) | Yes |
| Ber Rune | Shown (orange) | No |
| Magic Sword | Hidden (default) | No |
| Normal Potion | Hidden (default) | No |

---

### Scenario 6: Notify Independence

**Rules:**
```
A: "." unique gold                  # color, NO notify
B: "Jordan" unique gold notify      # color + notify
C: "." set lime sound1              # color + sound, NO notify
D: "." rare notify                  # notify only, no color
```

**Expected Behavior:**

| Item | Color | Sound | Text Notify |
|------|-------|-------|-------------|
| Stone of Jordan | gold | - | Yes |
| Other Unique | gold | - | **No** |
| Set Item | lime | sound1 | **No** |
| Rare Item | default | - | Yes |

**Key insight:** Sound plays without text notification. Color displays without notification.

---

### Scenario 7: Hide + Notify Combination

**Rules:**
```
A: "." normal hide notify sound1
B: "." magic hide
```

**Expected Behavior:**

| Item | Visible | Notification | Sound |
|------|---------|--------------|-------|
| Normal Sword | No | Yes | Yes |
| Magic Ring | No | No | No |

**Use case:** Player wants to hide low-value items but still be alerted when they drop (e.g., for gambling materials).

---

### Scenario 8: Multiple Stat Patterns (Future Feature)

If we support multiple stat patterns, they should ALL match:

**Rules:**
```
A: "Ring$" {All Skills} {Faster Cast}    # Both patterns must match
B: "Ring$" {All Skills}                   # Single pattern
```

**Test Items:**

| Item | Stats | Matches |
|------|-------|---------|
| Ring +Skills +FCR | Has both | A, B both match, A more specific |
| Ring +Skills only | Has one | Only B matches |

---

## Complex Filter Examples

### Example 1: Farming Filter

```
# Top tier - always notify with fanfare
"Jordan" unique gold notify sound1 name stat
"Tyrael" unique gold notify sound1 name stat
"Windforce" unique gold notify sound1 name stat

# Uniques - show with color, notify selectively
"." unique {All Skills} gold notify sound2 stat
"." unique gold

# Sets - show, notify on good ones
"." set {All Skills} lime notify stat
"." set lime

# High runes
"Ber|Jah|Lo|Ohm|Vex|Sur" show orange notify sound2

# Rares with good stats
"Ring$|Amulet" rare {All Skills} purple notify stat
"Circlet" rare {All Skills} purple notify stat

# Cleanup - hide junk
"." magic hide
"." normal hide
"." low hide
```

### Example 2: Leveling Filter

```
# Progression uniques
"." unique sacred show gold notify sound1
"." unique angelic show gold notify sound1

# Any unique visible
"." unique gold

# Sacred items worth checking
"." sacred eth lime notify

# Runes for runewords
"Rune$" show orange notify

# Hide clutter
"Potion$" normal hide
"." normal hide
"." low hide
```

### Example 3: Minimal Notification Filter

```
# Only notify on the best items
"Jordan|Tyrael|Windforce" unique gold notify sound1 name stat

# Show uniques with color (no notification spam)
"." unique gold

# Show sets with color (no notification)
"." set lime

# Hide everything else
"." magic hide
"." normal hide
"." low hide
```

---

## Edge Cases to Consider

### 1. Empty Pattern

```
"" unique gold    # What does this match?
```

**Recommendation:** Treat as invalid or match nothing.

### 2. Conflicting Show/Hide

```
A: "Ring$" show
B: "Ring$" hide
```

**Resolution:** Priority system determines winner. If equal, order matters? Or error?

**Recommendation:** Priority system handles it. Document that show/hide are mutually exclusive per rule.

### 3. No Flags

```
"Ring$"    # Pattern only, no flags
```

**Behavior:** Matches rings, uses global default for show/hide, no notification.

### 4. Case Sensitivity

```
"RING"   # Should match "Ring"?
```

**Current:** Case-insensitive matching (confirmed in code).

### 5. Regex Errors

```
"Ring[" unique    # Invalid regex
```

**Current:** Falls back to substring match (confirmed in code).

---

## Priority Algorithm Pseudocode

```python
def get_winning_rule(item, rules):
    matching_rules = []

    for rule in rules:
        if matches(item, rule):
            stat_matched = False
            if rule.stat_pattern:
                stat_matched = regex_match(item.stats, rule.stat_pattern)
            matching_rules.append({
                'rule': rule,
                'stat_matched': stat_matched,
                'has_color': rule.color is not None,
                'flag_count': count_flags(rule)
            })

    if not matching_rules:
        return None  # Use global default

    # Priority 1: Stat match
    stat_matches = [r for r in matching_rules if r['stat_matched']]
    if stat_matches:
        return max(stat_matches, key=lambda r: r['flag_count'])['rule']

    # Priority 2: Color flag
    color_matches = [r for r in matching_rules if r['has_color']]
    if color_matches:
        return max(color_matches, key=lambda r: r['flag_count'])['rule']

    # Priority 3: Flag count
    return max(matching_rules, key=lambda r: r['flag_count'])['rule']
```

---

## Summary

| Priority | Criterion | Notes |
|----------|-----------|-------|
| 1 (Highest) | Stat pattern match | `{pattern}` must match item stats |
| 2 | Color flag present | Any color keyword |
| 3 (Lowest) | Flag count | More flags = more specific |

| Flag Type | Independence | Notes |
|-----------|--------------|-------|
| `notify` | Independent | Does NOT auto-enable with color/sound |
| `sound` | Independent | Plays without text notification |
| `color` | Independent | Displays without notification |
| `show`/`hide` | Overrides global | Explicit visibility control |
