# Автокомплит в редакторе loot-filter DSL

Справочник о том, как устроен и как расширяется автокомплит имён предметов
в CodeMirror-редакторе правил (`src/editor/RulesEditor.svelte`).

---

## Что делает

- Когда курсор стоит внутри двойных кавычек в правиле (например `"Rin|"`),
  редактор предлагает базовые типы предметов из `items.txt` MedianXL
  (`Ring`, `Great Axe`, `Kris`, руны `Ber`/`Jah`/...).
- Вне кавычек автокомплит не активируется.
- Выбор из списка вставляет только текст имени (`Ring`) — кавычки и
  regex-якоря (`"Ring$"`) пользователь дописывает сам.

Сознательно **вне scope** (см. раздел "Как расширять" ниже):

- уникалки (`uniqueitems.txt`) и сеты (`setitems.txt`);
- ключевые слова DSL (`unique`, `eth`, `sound1`, ...) вне кавычек;
- автокомплит внутри стат-паттернов `{...}`.

---

## Архитектура

Источник данных — **память уже приаттаченного D2**. Мы не парсим MPQ, не
бандлим JSON; игра сама уже распарсила `items.txt` в свои таблицы, а у нас
уже есть и pointer на них, и injection для резолва локализованных имён.
Поэтому автокомплит — тонкая надстройка над существующей инфраструктурой
`DropScanner`.

```
    D2 process memory                App (Rust)                   Webview (Svelte)
    ─────────────────                ──────────                   ────────────────

    items.txt array    ──►  DropScanner.class_cache          itemsDictionaryStore
    (pointer +              (Vec<ClassInfo>)                 (runes, reactive)
     count + records)       already built by the
         ▲                  existing NotifierCache                 ▲
         │                       port                               │
         │                         │                                │
         │                         ▼                                │
         │                  items_dictionary_snapshot()             │
         │                  (dedup + sort)                          │
         │                         │                                │
         │                         ▼                                │
         │              start_scanner_internal loop                 │
         │              (main.rs): publish once per                 │
         │              scanner attach                              │
         │                         │                                │
         │                         ├──► Arc<RwLock<Option<Vec>>>    │
         │                         │       in AppState               │
         │                         │                                │
         │                         ├──► items-cache.json on disk     │
         │                         │       (app_data_dir)            │
         │                         │                                │
         │                         └──► emit "items-dictionary-
         │                                    updated" ────────────►┤
         │                                                          │
         │                                invoke("get_items_dictionary")
         │                                       (on store init)     │
         │                                                          ▼
         └── remote_thread через                         d2rulesAutocomplete(
             D2Injector.get_string                           () => store.items
             (резолвит NAME_ID → WCHAR*)                     )
                                                               │
                                                               ▼
                                          CodeMirror popup inside "..." strings
```

---

## Как данные попадают в список подсказок

1. **Сканер аттачится к D2** (`DropScanner::new()` в
   `src-tauri/src/notifier.rs`). При атташе создаётся и инжектор, и
   готовность читать память.
2. **На первом in-game тике** (`tick()` в `notifier.rs`, строки ~164) уже
   существующий код `build_class_cache()` читает массив `items.txt`
   (pointer и count — `offsets::d2common::ITEMS_TXT*`), для каждой
   записи берёт `NAME_ID` (word по смещению `0xF4`) и резолвит его через
   remote-thread-вызов `GetStringById` (`D2Injector::get_string`). На
   выходе — `Vec<ClassInfo>` с чистыми display-именами типа `"Great Axe"`.
3. **`items_dictionary_snapshot()`** (`notifier.rs`) — тонкий getter над
   `class_cache`: фильтрует пустые имена, дедупит (каждый tier — отдельная
   запись в items.txt, имя одно), сортирует.
4. **scanner-loop в `main.rs`** (`start_scanner_internal`) после каждого
   тика, если ещё не публиковали (`dict_published: bool` — локальный флаг),
   дёргает snapshot. На первом `Some(dict)` делает три вещи:
   - пишет в `AppState.items_dictionary: Arc<RwLock<Option<Vec<String>>>>`;
   - вызывает `items_cache::save_items_cache` → на диск;
   - эмитит `items-dictionary-updated` с payload = `Vec<String>`.
5. **Фронт (`itemsDictionaryStore`,
   `src/stores/items-dictionary.svelte.ts`)** на `init()` делает
   `invoke("get_items_dictionary")` (получает кэш, если он был загружен
   с диска при старте приложения) и подписывается на событие
   `items-dictionary-updated` (live-обновления при работающей игре).
6. **CodeMirror-extension (`d2rules-autocomplete.ts`)** при каждом
   срабатывании автокомплита вызывает коллбек `() => store.items` и
   возвращает список в качестве `CompletionResult`.

---

## Ключевые файлы

### Backend (Rust)

| Файл | Роль |
|---|---|
| `src-tauri/src/notifier.rs` | `DropScanner::items_dictionary_snapshot()` — getter над `class_cache`. Сам `class_cache` строится в `build_class_cache()`, порту `NotifierCache` из `D2Stats.au3`. |
| `src-tauri/src/items_cache.rs` | `load_items_cache` / `save_items_cache` — чтение/запись `items-cache.json` в `app_data_dir`. |
| `src-tauri/src/main.rs` | Поле `items_dictionary` в `AppState`. Команда `get_items_dictionary`. Логика публикации один раз за сессию сканера в `start_scanner_internal`. Загрузка кэша с диска в `setup`. |
| `src-tauri/src/offsets.rs` | `d2common::ITEMS_TXT*` — адреса count/pointer таблицы. `items_txt::RECORD_SIZE` / `NAME_ID` — layout записи. `d2lang::GET_STRING_BY_ID` — функция резолва локализации. |
| `src-tauri/src/injection.rs` | `D2Injector::get_string` — remote-thread вызов `GetStringById`. |

### Frontend (Svelte + TS)

| Файл | Роль |
|---|---|
| `src/stores/items-dictionary.svelte.ts` | `itemsDictionaryStore` — singleton store, runes-based. `init()` / `destroy()` / геттеры `items`, `source`. |
| `src/stores/index.ts` | Реэкспорт `itemsDictionaryStore`. |
| `src/editor/d2rules-autocomplete.ts` | CodeMirror extension. Проверка "внутри ли кавычек" через per-line scanner, возврат `CompletionResult`. |
| `src/editor/d2rules-theme.ts` | `autocompleteTheme` — стили popup. Включён в `getDarkThemeExtensions()` / `getLightThemeExtensions()`. |
| `src/editor/RulesEditor.svelte` | Регистрация extension'а в `buildExtensions()` через `d2rulesAutocomplete(() => itemsDictionaryStore.items)`. |
| `src/views/MainWindow.svelte` | Единственная точка вызова `itemsDictionaryStore.init()` (и `.destroy()` на cleanup). |

---

## Жизненный цикл кэша

Кэш инвалидируется **автоматически** через механизм "каждый новый attach к
D2 пересобирает словарь из живой памяти".

- `DropScanner.class_cache` живёт одну сессию сканера. Каждый раз, когда
  пользователь запускает/перезапускает D2, `spawn_auto_scanner` в
  `main.rs` создаёт новый сканер — `class_cache: None`. На первом in-game
  тике он перестраивается из текущего состояния `items.txt` в памяти.
- Локальный флаг `dict_published` в scanner-loop → **ровно одна**
  публикация за сессию сканера (не спамим событиями и записью на диск).
- **При обновлении MedianXL:** патч требует закрытия D2 → сканер-поток
  видит "D2 closed" (`main.rs:165`), завершается. Следующий запуск игры
  → новый сканер → свежий `build_class_cache` → snapshot отличается от
  предыдущего → `items-cache.json` перезаписан → событие пришло →
  автокомплит сам подхватил новые имена. **Пользователю ничего делать
  не надо.**
- **Без D2:** `setup` в `main.rs` вызывает `items_cache::load_items_cache`
  и заполняет `AppState.items_dictionary` из файла. Команда
  `get_items_dictionary` возвращает ровно эти данные.
- **Ручной инвалидации нет.** TTL нет. Версионирования по мод-версии нет.
  Достаточно: удалить `items-cache.json` из `%APPDATA%\D2MXLUtils\`
  (Windows) — он пересоздастся при следующем in-game тике.

**Граничный случай:** пользователь обновил MedianXL, но ещё не запускал
D2 в текущей сессии приложения — автокомплит показывает кэш с диска
(устаревший). Запустил игру, вошёл в партию → через ~1 секунду (время
`build_class_cache`) прилетает событие, store обновляется,
автокомплит синхронен с патчем.

---

## Формат on-disk кэша

Путь: `app_data_dir()/items-cache.json` (на Windows обычно
`%APPDATA%\D2MXLUtils\items-cache.json`).

```json
{
  "base_types": ["Amulet", "Great Axe", "Kris", "Ring", "..."],
  "dumped_at": "2026-04-20T15:23:07Z"
}
```

- `base_types` — отсортированный массив без дубликатов.
- `dumped_at` — RFC-3339 (UTC), **только для диагностики**, в рантайме не
  используется.

При изменении формата (например, при добавлении уникалок/сетов):

- Меняется только `items_cache.rs` и структура `ItemsCacheFile`.
- Парсинг падает при несовместимости → `load_items_cache` возвращает
  `None`, логгер пишет `items cache: parse failed: ...`, кэш будет
  перестроен на следующем attach. Старый файл можно просто удалить.

---

## Как расширять

### Добавить уникалки и сеты

Нужен reverse-engineering двух указателей в `D2Common.dll` + layout
записей:

1. `d2common::UNIQUE_ITEMS_TXT*` (pointer + count) для `uniqueitems.txt`.
2. `d2common::SET_ITEMS_TXT*` (pointer + count) для `setitems.txt`.
3. Layout записи: смещение поля с `NAME_ID` (word). См. D2 mod wiki или
   исходники PD2/D2MR — там всё документировано.

Далее:

1. В `offsets.rs` добавить новые константы.
2. В `notifier.rs` — методы `build_unique_cache()` / `build_set_cache()`
   по образцу `build_class_cache()`. Резолв имени через тот же
   `D2Injector::get_string`.
3. Расширить `items_dictionary_snapshot()` до возвращения структуры:
   ```rust
   pub struct ItemsDictionary {
       pub base_types: Vec<String>,
       pub uniques: Vec<String>,
       pub sets: Vec<String>,
   }
   ```
   **Внимание:** меняется TypeScript-контракт команды и payload события.
4. В `items_cache.rs` — обновить `ItemsCacheFile` структуру.
5. Фронт:
   - Store: поменять тип с `string[]` на `ItemsDictionary`-подобный.
   - `d2rules-autocomplete.ts`: объединять все три списка в options,
     задать для каждого отличный `detail` (`"unique"` / `"set"`) и
     `boost` (чтобы уникалки поднимались выше — имена чаще точнее).

### Добавить keyword autocomplete вне кавычек

Ключевые слова DSL (`unique`, `set`, `eth`, `sound1`, ...) уже
перечислены в `src/editor/d2rules-language.ts` как массивы констант.
Шаги:

1. В `d2rules-autocomplete.ts` после проверки "внутри кавычек?" добавить
   ветку "вне кавычек и вне `{...}`".
2. Собрать `options` из `QUALITY_KEYWORDS | TIER_KEYWORDS | ...` —
   экспортировать их из `d2rules-language.ts` (сейчас они приватные).
3. Вероятно, стоит поменять их тип на `"keyword"` в `Completion` и дать
   каждому категорию (`detail: "quality"`).

Никакого backend-кода для этого не требуется — ключевые слова статичны,
определены на фронте.

### Добавить стат-паттерны `{...}`

Stat-теги — отдельный и более тяжёлый источник данных. Они не лежат в
`items.txt` — это строки локализации, собираемые из таблиц
`ItemStatCost.txt` и подобных. Пойдут по тому же принципу:
указатели → чтение записей → резолв через `GetStringById`. Но layout
записи этих таблиц сложнее, и сами статы имеют плейсхолдеры типа
`+%d to All Skills` — нужна отдельная обработка для подстановки.

---

## Производительность

- `build_class_cache` — разовый для одной сессии сканера. Цена ~500–1500
  мс на MedianXL (~300 записей × один `GetStringById` через remote
  thread, каждый ~1–5 мс). Блокирует scanner-поток; UI-поток не
  затрагивается. Визуально пользователь заметит задержку ~1 сек перед
  появлением первого drop-notification после входа в игру — это
  существующее поведение, не привнесённое автокомплитом.
- Публикация snapshot'а (write Arc + save file + emit event) — единицы
  миллисекунд, одноразово на сессию.
- CodeMirror-фильтрация в popup полностью клиентская. С ~300 записей
  лага нет даже на слабых машинах.

---

## Известные ограничения

1. **Первый запуск без D2 ни разу**: автокомплит пуст. Фолбэк-списка
   осознанно нет (чтобы не расходиться с реальными данными MedianXL).
   Один раз зайти в игру — и файл кэша сохранится навсегда.
2. **Нет контекста правила.** Внутри `"..."` мы не знаем, является ли
   правило `unique`-правилом — предлагаем все base-types одинаково. Это
   обычно и нужно (поскольку одно и то же имя может фигурировать в
   правилах любого quality), но если захочется боостить уникалки в
   `unique` правиле — понадобится парсить контекст правила перед
   курсором.
3. **Regex-якоря не вставляются автоматически.** Пользователь сам
   дописывает `$`/`^` — это сознательное решение, т.к. автоматическая
   подстановка была бы непредсказуемой (иногда надо `"Ring"`, иногда
   `"Ring$"`, иногда `"^Ring"`).
4. **Кэш overwrite без diff.** На каждый новый attach пишем файл, даже
   если содержимое идентично. Пока — не проблема (файл ~10 КБ), но при
   расширении до uniques/sets можно добавить хэш-сравнение в
   `save_items_cache`.

---

## Отладка

- **Словарь пуст?** Проверить `%APPDATA%\D2MXLUtils\items-cache.json`
  (если нет — сканер ни разу успешно не строил cache). Логи в
  `d2mxlutils.log` рядом с exe содержат строки `items cache: ...` и
  `Published items dictionary (N base types)`.
- **Не обновляется после нового патча MedianXL?** Убедиться, что D2
  полностью перезапускалась (scanner thread видит её закрытие по
  `is_diablo2_running()`). Если сомневаетесь — удалить
  `items-cache.json` вручную и перезапустить и приложение, и D2.
- **Popup пустой при типинге внутри кавычек?** Проверить в DevTools
  (`pnpm tauri dev`), что `itemsDictionaryStore.items` не пуст.
  Проверить, что событие `items-dictionary-updated` действительно
  приходит — Tauri events видны в backend-логе.
- **Popup белый/кривой стиль?** `autocompleteTheme` в
  `d2rules-theme.ts` не попал в список возврата
  `getDarkThemeExtensions()`. Проверить порядок extension'ов в
  `RulesEditor.svelte:buildExtensions()` — autocomplete должен идти
  после темы.

---

## См. также

- Спецификация DSL: `docs/filter_spec/loot-filter-dsl.md`
- Исходный AutoIt (референс для `NotifierCache`):
  `docs/index_d2Stats.md` (индекс секций; сам `D2Stats.au3` слишком
  большой, читать только по смещению)
- План первой итерации: `.claude/plans/validated-twirling-tome.md`
