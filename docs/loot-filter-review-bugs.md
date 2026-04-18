# Loot Filter Refactor — Bug List

Результат ревью рефакторинга loot filter против новой спецификации в `docs/filter_spec/`.
База: `HEAD = 6c9b497` (только spec), голова: несохранённые изменения в working tree.

Статус: 31 unit-тест проходит, `cargo check` чист. Нужны фиксы C1 и C2 перед коммитом.

---

## Critical

### C1. Оверлей игнорирует `filter: Notification`

**Файлы:**
- `src/views/OverlayWindow.svelte` (строки 8–16)
- `src/components/Notification.svelte` (строки 2–10, 21–31)

Backend (`src-tauri/src/notifier.rs:340–343` + struct `ItemDropEvent.filter: Option<Notification>`)
заполняет уведомление из winning rule — цвет, звук, `display_name`, `display_stats`.

Spec требует:
> "A fired notification uses the winning rule's: color — text color (or default if absent);
> sound — sound index 1–6 (silent if absent); name — include item name if set;
> stat — include item stats if set"

Но фронтенд-интерфейс `ItemDrop` в обоих svelte-файлах **не содержит поля `filter`**:

```ts
interface ItemDrop {
  unit_id: number; class: number; quality: string; name: string;
  stats: string; is_ethereal: boolean; is_identified: boolean;
}
```

`Notification.svelte` всегда красит имя через легаси-таблицу `qualityColors`, игнорируя
`filter.color`. Звук не воспроизводится нигде. Флаги `display_name` / `display_stats`
не читаются — имя и статы показываются всегда.

**Эффект:** правила `"Ber" rare notify sound1` не издают звука, `unique red notify`
не покажет красным, `notify` без `name` всё равно покажет имя.

**Фикс:**
- расширить `ItemDrop` интерфейс полем `filter`
- передать `filter` в `Notification.svelte`
- замапить `filter.color` в CSS (сериализовать hex на бэкенде или продублировать таблицу)
- проиграть звук при `filter.sound`
- скрывать имя/статы при отсутствии флагов

---

### C2. Сканер никогда не проставляет `tier` — правила с тиром мертвы ✅ RESOLVED

**Fix:** реализована per-class tier-таблица, строится один раз при первом тике сканера
из items.txt через инъекцию `D2Lang_GetStringById`. Подробности см. коммит и
`src-tauri/src/notifier.rs::build_tier_cache`.

- Shellcode `GetString` теперь реальный вызов `D2Lang+0x9450` (`src-tauri/src/injection.rs`)
- `D2Injector::new` принимает `d2_lang` (`src-tauri/src/notifier.rs:75`)
- `D2Injector::get_string` — публичный метод для чтения string-table записей
- Константы `d2lang::GET_STRING_BY_ID`, `d2common::ITEMS_TXT_COUNT` в `offsets.rs`
- `DropScanner.tier_cache: Option<Vec<ItemTier>>` + ленивый `build_tier_cache()`
- `ItemDropEvent.tier` заполняется из `class_tier(class)` в `to_event` и `apply_filter_to_all_items`
- Новый тест `tier_zero_matches_untiered_items` проверяет что рун/амулетов матчится keyword `0`

**Стоимость:** ~2-3 сек синхронной работы при первом тике после аттача к игре.
После этого — O(1) lookup в `Vec`.

---

## Important

### I2. Двойной `decide()` каждой итерации сканера

**Файл:** `src-tauri/src/notifier.rs` строки 283–284 (основной цикл) и 448–449
(`apply_filter_to_all_items`).

Каждый новый item за один tick проходит полный `matches` дважды. При 100+ items
на земле со сложной регулярки-stat-паттерной ощутимо.

**Фикс:** в `apply_filter_to_all_items` пропускать items, уже обработанные в
основном цикле текущего tick, либо объединить две фазы.

---

### I3. `parse_attrs_into`: ошибка "Group headers cannot contain a name pattern" непредсказуема

**Файл:** `src-tauri/src/rules/dsl.rs`, строки 485–492.

Проверка `lower.starts_with('"')` применяется к токену после `split_whitespace`.
Для заголовка группы `"Stone of Jordan"` токенизация даст `"stone`, `of`, `jordan"` —
только первый триггерит ошибку, остальные выдают warning "Unknown flag".

**Фикс:** извлекать `"..."` до `split_whitespace` (как в `extract_name_pattern`),
либо детектировать открывающий quote и пропускать до закрывающего при подсчёте токенов.

---

### I4. Regex stat-pattern не поддерживает `{n,m}` квантификаторы

**Файл:** `src-tauri/src/rules/dsl.rs`, `extract_stat_pattern` строки 545–577.

Первый `}` закрывает stat-pattern. `{All Skills.{2,5}}` разобрётся как
`All Skills.{2,5` + остаток `}`. Экранирование `\{` работает, но неочевидно.

**Фикс:** считать `{` / `}` балансированно внутри stat-pattern (учитывая
`\{` / `\}` escapes), либо задокументировать в spec требование экранировать
внутренние `}` через `\}`.

---

### I5. `sound_none` → `Some(0)` — семантика "silence" не выражена типом

**Файл:** `src-tauri/src/rules/dsl.rs` строки 471–474.

Грамматически корректно, но ни `NotifyColor::to_hex`, ни фронтенд-оверлей
не обрабатывают `sound=0` как "тишина". Станет релевантно после фикса C1.

**Фикс:** либо использовать `Option<NonZeroU8>`, либо явно трактовать `Some(0)`
как "silence" в UI-слое с комментарием/контрактом.

---

## Minor

### M1. `NotifyColor::to_hex` помечен unused

**Файл:** `src-tauri/src/rules/mod.rs` строка 147.
Warning `method to_hex is never used` — следствие того, что фронтенд не читает
`filter.color` (C1). После фикса C1 метод либо перестанет быть dead, либо можно
сериализовать hex напрямую в JSON.

---

### M2. Дефолтный шаблон фильтра содержит мёртвое правило ✅ RESOLVED (via C2)

После фикса C2 правило `sacred eth gold notify sound1 name` в
`src/views/LootFilterTab.svelte:27` стало рабочим.

---

### M3. Устаревшая плановая документация

`docs/d2mxlutils-refactoring.plan.md:96` и `.cursor/plans/d2m-8b56206f.plan.md`
всё ещё упоминают `default_show_items`, `RuleAction`, `to_dsl`. В коде этого нет.

---

### M4. Оверлей поллит настройки каждые 2 сек

`src/views/OverlayWindow.svelte` строки 95–97: `setInterval(() => settingsStore.load(), 2000)`.
TODO уже стоит. Не связано с рефакторингом фильтра напрямую.

---

### M5. `#` внутри stat-pattern `{...}` не обрезается как комментарий

`strip_inline_comment` (`dsl.rs` строки 523–541) корректно экранирует `#`
внутри `{}` и `""`. Поведение правильное — отметка для подтверждения.

---

### M6. `validate_dsl` не диагностирует `hide default <extra>`

Если пользователь пишет `hide default unique`, `parse_default_mode` возвращает
`None` (требует ровно 2 токена), и line уходит в rule-parse: `hide` →
visibility, `default` → unknown-flag warning, `unique` → quality. Пользователь
может не понять, что директива не применилась.

**Фикс:** специально диагностировать `hide default <extra tokens>`.

---

## Spec conformance snapshot

| Область | Статус |
|---|---|
| Default mode directive (`show/hide default`) | ✅ полностью |
| Rule anatomy — name/stat/quality/eth/color/visibility/sound/notify | ✅ |
| Rule anatomy — tier | ⚠️ парсится, scanner не даёт (C2) |
| Rule anatomy — `name`/`stat` flags применение | ❌ C1 |
| Last-match-wins, AND, regex fallback, visibility resolution | ✅ |
| Group flattening + header merge + rule-overrides-group | ✅ |
| Nested groups / unterminated groups rejected | ✅ |
| Editor ↔ parser keyword alignment | ✅ полное |
| Frontend: profiles, unsaved-changes, active profile persist | ✅ |
| Оверлей применяет `color`/`sound`/`display_name`/`display_stats` | ❌ C1 |

---

## Ключевые пути

- Spec: `docs/filter_spec/loot-filter-spec.md`, `loot-filter-dsl.md`, `loot-filter-examples.md`
- Backend: `src-tauri/src/rules/{mod.rs,dsl.rs,matching.rs}`
- Сканер: `src-tauri/src/notifier.rs`
- Tauri: `src-tauri/src/main.rs`
- Profiles: `src-tauri/src/profiles.rs`
- Settings: `src-tauri/src/settings.rs`, `src/stores/settings.svelte.ts`
- Фронт фильтра: `src/views/LootFilterTab.svelte`, `src/components/ProfileSelector.svelte`
- **Оверлей (не обновлён, C1):** `src/views/OverlayWindow.svelte`, `src/components/Notification.svelte`
- Редактор: `src/editor/d2rules-language.ts`, `d2rules-theme.ts`
