# Loot Filter Refactor — Bug List

Результат ревью рефакторинга loot filter против новой спецификации в `docs/filter_spec/`.
Исходная база: `HEAD = 6c9b497`. Ниже — сводка из оригинального ревью с отметкой
о состоянии каждого пункта.

---

## Critical

### C1. Оверлей игнорирует `filter: Notification` ✅ RESOLVED

Фронтенд теперь мапит backend'овскую `Notification`:
- `ItemDrop` интерфейс содержит `filter?: NotificationFilter` (`src/views/OverlayWindow.svelte`, `src/components/Notification.svelte`)
- `filter.color` идёт в CSS через палитру `notifyColors`; winning rule color перекрывает quality color
- Звук проигрывается через `playSound(filter.sound, soundVolume)` на событии `item-drop`
- `display_stats` учитывается; `matched_stat_line` подсвечивается отдельным стилем
- Флаг `display_name` убран из спеки вовсе — имя теперь всегда показывается

### C2. Сканер никогда не проставляет `tier` ✅ RESOLVED

Per-class tier-таблица строится при первом тике сканера из items.txt
(`DropScanner::build_class_cache` → `ClassInfo.tier`). `ItemDropEvent.tier`
заполняется из `class_tier(class)` в `to_event`.

---

## Important

### I2. Двойной `decide()` каждой итерации сканера ✅ RESOLVED

Функция `apply_filter_to_all_items` полностью удалена. `DropScanner::tick`
выполняет единственный проход `decide()` на новый item.

### I3. `parse_attrs_into`: ошибка "Group headers cannot contain a name pattern" непредсказуема ✅ RESOLVED

Проверка на кавычку теперь делается до `split_whitespace` — при обнаружении
`"` в group-header source эмитится один чёткий error и парсинг линии
прекращается (нет каскада "Unknown flag" на обрывках имени). Аналогично
в `validate_dsl::validate_tokens`. Покрыто тестами
`group_header_with_quoted_name_emits_single_error` и
`validator_group_header_with_quoted_name_single_error`.

### I4. Regex stat-pattern не поддерживает `{n,m}` квантификаторы ✅ RESOLVED

`extract_stat_pattern` теперь балансирует `{` / `}` со счётчиком глубины
(сохраняя `\{` / `\}` escape'ы). `{All Skills.{2,5}}` парсится корректно
в `All Skills.{2,5}`. Покрыто тестом `stat_pattern_allows_regex_quantifier`.

### I5. `sound_none` → `Some(0)` — семантика "silence" не выражена типом ✅ RESOLVED

`Some(0)` на уровне `Rule.sound` остаётся служебным маркером "явной тишины"
(нужен для перебивания группового `soundN`), но `FilterConfig::decide`
нормализует его в `None` при сборке `Notification`. Фронтенд никогда не
видит `sound=0`. Документация добавлена к `sound_none` в dsl.rs и к
маркеру в mod.rs. Покрыто тестом
`sound_none_overrides_group_sound_and_emits_no_sound_in_notification`.

---

## Minor

### M1. `NotifyColor::to_hex` помечен unused ✅ RESOLVED

Метод удалён — фронтенд маппит имя цвета в CSS-переменные напрямую
(`notifyColors` в `Notification.svelte`), hex на бэкенде не нужен.

### M2. Дефолтный шаблон фильтра содержит мёртвое правило ✅ RESOLVED

Из `DEFAULT_FILTER` в `src/views/LootFilterTab.svelte` убран deprecated
флаг `name` (3 упоминания). Шаблон больше не выдаёт "Unknown flag" warning'и
при первой загрузке.

### M3. Устаревшая плановая документация ✅ RESOLVED

`docs/d2mxlutils-refactoring.plan.md` и `.cursor/plans/d2m-8b56206f.plan.md`
удалены.

### M4. Оверлей поллит настройки каждые 2 сек ✅ RESOLVED

`save_settings` в `src-tauri/src/settings.rs` эмитит событие
`settings-updated` с payload'ом после успешного сохранения.
`OverlayWindow.svelte` слушает это событие и вызывает `settingsStore.load()`
вместо `setInterval`. Задержка синка — мгновенная.

### M5. `#` внутри stat-pattern `{...}` не обрезается как комментарий ✅ CONFIRMED

`strip_inline_comment` корректно обрабатывает `#` внутри `{}` и `""`.
Поведение спроектировано так намеренно.

### M6. `validate_dsl` не диагностирует `hide default <extra>` ✅ RESOLVED

`parse_default_mode` теперь возвращает enum `DefaultModeParse`
(`NotDirective | Directive(bool) | ExtraTokens(keyword)`). Лишние токены
после `hide default` / `show default` дают явный error: "'hide default'
is a file-scope directive and cannot have additional tokens". Работает
в обоих путях — `parse_dsl` и `validate_dsl`. Покрыто тестами
`hide_default_with_extras_is_error` и
`validator_flags_hide_default_with_extras`.

---

## Spec conformance snapshot

| Область | Статус |
|---|---|
| Default mode directive (`show/hide default`) | ✅ полностью |
| Rule anatomy — name/stat/quality/eth/color/visibility/sound/notify | ✅ |
| Rule anatomy — tier | ✅ |
| Last-match-wins, AND, regex fallback, visibility resolution | ✅ |
| Group flattening + header merge + rule-overrides-group | ✅ |
| Nested groups / unterminated groups rejected | ✅ |
| Editor ↔ parser keyword alignment | ✅ полное |
| Frontend: profiles, unsaved-changes, active profile persist | ✅ |
| Оверлей применяет `color`/`sound`/`display_stats` | ✅ |

---

## Ключевые пути

- Spec: `docs/filter_spec/loot-filter-spec.md`, `loot-filter-dsl.md`, `loot-filter-examples.md`
- Backend: `src-tauri/src/rules/{mod.rs,dsl.rs,matching.rs}`
- Сканер: `src-tauri/src/notifier.rs`
- Tauri: `src-tauri/src/main.rs`
- Profiles: `src-tauri/src/profiles.rs`
- Settings: `src-tauri/src/settings.rs`, `src/stores/settings.svelte.ts`
- Фронт фильтра: `src/views/LootFilterTab.svelte`, `src/components/ProfileSelector.svelte`
- Оверлей: `src/views/OverlayWindow.svelte`, `src/components/Notification.svelte`
- Редактор: `src/editor/d2rules-language.ts`, `d2rules-theme.ts`
