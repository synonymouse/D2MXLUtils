# Автокомплит в редакторе loot-filter DSL

Справочник о том, как устроен и как расширяется автокомплит имён предметов
в CodeMirror-редакторе правил (`src/editor/RulesEditor.svelte`).

---

## Что делает

- Когда курсор стоит внутри двойных кавычек в правиле (например `"Rin|"`),
  редактор предлагает:
  - **базовые типы** из `items.txt` MedianXL (`Ring`, `Great Axe`, `Kris`,
    руны `Ber`/`Jah`/…) — без метки слева;
  - **сет-предметы** из `SetItems.txt` — метка `set`;
  - **уникалки** из `UniqueItems.txt`, классифицированные по `wLvl`
    (банды из D2Stats.au3:1181-1191, но верхняя граница SSSU снята —
    MXL имеет SSSU до `wLvl 139+`, `<=130` в D2Stats мислабелил):
    - `wLvl 2..100` → `TU` (Tiered Unique);
    - `wLvl 101..115` → `SU` (Sacred Unique);
    - `wLvl 116..120` → `SSU`;
    - `wLvl 121..` (без верхней границы) → `SSSU`;
    - `wLvl == 0` или `1` — запись отбрасывается (квест-уники,
      для автокомплита бесполезны).
- Метка-категория рисуется в слоте иконки слева от имени — используется
  per-type CSS через `.cm-completionIcon-{base,set,tu,su,ssu,sssu}::after` в
  `src/editor/d2rules-theme.ts`.
- Дубликаты уникалок (один и тот же display name на нескольких
  wLvl-записях) схлопываются в одну строку с **наивысшим** kind
  (SSSU > SSU > SU > TU) — чтобы сильнейший тир не терялся при dedup'е.
- Уники, имя которых уже есть в `base_types` (MXL-чарки типа
  `The Butcher's Tooth`, `Azmodan's Heart`), отбрасываются из unique-списков
  — остаются только в base-секции.
- Вне кавычек автокомплит не активируется.
- Выбор из списка вставляет только текст имени (`Ring`) — кавычки и
  regex-якоря (`"Ring$"`) пользователь дописывает сам.

Сознательно **вне scope** (см. раздел "Как расширять" ниже):

- группы сет-бонусов (`Sets.txt` / `Sigon's Complete Steel`);
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
2. **На первом in-game тике** (`tick()` в `notifier.rs`) последовательно
   строятся три in-memory кэша:
   - `build_class_cache()` читает массив `items.txt`
     (`offsets::d2common::ITEMS_TXT*`), для каждой записи берёт `NAME_ID`
     (word @ `0xF4`), резолвит имя через remote-thread-вызов
     `GetStringById`. На выходе — `Vec<ClassInfo>` (поля `base_name`
     и `tier` — последний по-прежнему нужен для рантайм-матчинга
     в `ItemDropEvent.tier`).
   - `build_unique_items_cache()` идёт по `UniqueItems.txt`
     (pointer/count через `sgptDataTables + data_tables::UNIQUE_ITEMS_TXT*`),
     читает `NAME_ID @ 0x22` (display name) и `wLvl @ 0x34`.
     Классификация через `UniqueKind::from_wlvl(wlvl)` — порты банды из
     D2Stats.au3:1181-1191.
   - `build_set_items_cache()` — по `SetItems.txt` (pointer/count через
     `data_tables::SET_ITEMS_TXT*`), NAME_ID @ `0x24`. Классификации нет,
     все — «set».
3. **`items_dictionary_snapshot()`** (`notifier.rs`) собирает три кэша
   в единый `ItemsDictionary { base_types, uniques_tu, uniques_su,
   set_items }`:
   - `base_types` — нормализует хвостовые суффиксы в скобках
     (`(Sacred)` / `(Angelic)` / `(Mastercrafted)` / `(N)`), кроме
     `X Container (NN)` где число — часть имени руны. Сортирует, дедупит.
   - `uniques_tu` / `uniques_su` / `uniques_ssu` / `uniques_sssu` —
     партиционирует по `UniqueKind`, **промоутит дубли по имени к
     наивысшему kind** (`Sssu > Ssu > Su > Tu`). Отбрасывает имена,
     которые уже есть в `base_types` (MXL-чарки). Сортирует.
   - `set_items` — сортировка + dedup, без нормализации.
4. **scanner-loop в `main.rs`** (`start_scanner_internal`) после каждого
   тика, если ещё не публиковали (`dict_published: bool`), дёргает
   snapshot. На первом `Some(dict)` делает три вещи:
   - пишет в `AppState.items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>`;
   - вызывает `items_cache::save_items_cache` → на диск;
   - эмитит `items-dictionary-updated` с payload = `ItemsDictionary`.
5. **Фронт (`itemsDictionaryStore`,
   `src/stores/items-dictionary.svelte.ts`)** на `init()` делает
   `invoke<ItemsDictionary>("get_items_dictionary")` и подписывается на
   событие `items-dictionary-updated`. Геттер `options` выдаёт плоский
   `AutocompleteOption[]` — `{ label, kind: 'base' | 'set' | 'tu' | 'su' }`.
6. **CodeMirror-extension (`d2rules-autocomplete.ts`)** при каждом
   срабатывании автокомплита вызывает коллбек `() => store.options` и
   маппит на `Completion`, проставляя `type: kind`. CodeMirror
   рендерит `<div class="cm-completionIcon cm-completionIcon-<kind>">` —
   CSS в `d2rules-theme.ts` через `::after { content: ... }` рисует
   в слоте метку (`"set"` / `"TU"` / `"SU"` / пусто для base).

---

## Ключевые файлы

### Backend (Rust)

| Файл | Роль |
|---|---|
| `src-tauri/src/notifier.rs` | Три build-метода (`build_class_cache` / `build_unique_items_cache` / `build_set_items_cache`) + `items_dictionary_snapshot()` → `ItemsDictionary`. Типы `ClassInfo` / `UniqueInfo` / `UniqueKind` / `ItemsDictionary`. |
| `src-tauri/src/items_cache.rs` | `load_items_cache` / `save_items_cache` — JSON с 4 полями (base/TU/SU/set) в `%APPDATA%\com.d2mxlutils.app\items-cache.json`. Версионирование через `CARGO_PKG_VERSION`. |
| `src-tauri/src/main.rs` | `AppState.items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>`. Команда `get_items_dictionary`. Публикация один раз за сессию сканера в `start_scanner_internal`. Загрузка кэша с диска в `setup`. |
| `src-tauri/src/offsets.rs` | `d2common::ITEMS_TXT*` / `SGPT_DATA_TABLES`. `items_txt::{RECORD_SIZE, NAME_ID, CODE}`. `data_tables::{UNIQUE_ITEMS_TXT_*, SET_ITEMS_TXT_*}`. `unique_items_txt::{RECORD_SIZE, NAME_ID, BASE_ITEM_CODE}`. `set_items_txt::{RECORD_SIZE, NAME_ID, BASE_ITEM_CODE}`. `d2lang::GET_STRING_BY_ID`. |
| `src-tauri/src/injection.rs` | `D2Injector::get_string` — remote-thread вызов `GetStringById`. |

### Frontend (Svelte + TS)

| Файл | Роль |
|---|---|
| `src/stores/items-dictionary.svelte.ts` | `itemsDictionaryStore` — singleton store, runes-based. Типы `ItemsDictionary` / `AutocompleteKind` / `AutocompleteOption`. Геттеры `dict`, `options`, `source`; методы `init()` / `destroy()`. |
| `src/stores/index.ts` | Реэкспорт `itemsDictionaryStore`. |
| `src/editor/d2rules-autocomplete.ts` | CodeMirror extension. Проверка "внутри ли кавычек" через per-line scanner. Маппит `AutocompleteOption.kind` на `Completion.type` (CodeMirror рендерит из него CSS-класс `cm-completionIcon-<kind>`). |
| `src/editor/d2rules-theme.ts` | `autocompleteTheme` — стили popup + `::after { content: ... }` per kind для меток `set` / `TU` / `SU`. Включён в `getDarkThemeExtensions()` / `getLightThemeExtensions()`. |
| `src/editor/RulesEditor.svelte` | Регистрация extension'а в `buildExtensions()` через `d2rulesAutocomplete(() => itemsDictionaryStore.options)`. |
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
  Достаточно: удалить `items-cache.json` из `%APPDATA%\com.d2mxlutils.app\`
  (Windows) — он пересоздастся при следующем in-game тике.

**Граничный случай:** пользователь обновил MedianXL, но ещё не запускал
D2 в текущей сессии приложения — автокомплит показывает кэш с диска
(устаревший). Запустил игру, вошёл в партию → через ~1 секунду (время
`build_class_cache`) прилетает событие, store обновляется,
автокомплит синхронен с патчем.

---

## Формат on-disk кэша

Путь: `app_data_dir()/items-cache.json`. На Windows Tauri v2
резолвит `app_data_dir()` через bundle identifier (из
`tauri.conf.json`), не через `productName`, поэтому реальный путь —
`%APPDATA%\com.d2mxlutils.app\items-cache.json`.

```json
{
  "schema": "1.2.1",
  "base_types": ["Amulet", "Great Axe", "Kris", "Ring", "..."],
  "uniques_tu": ["Doombringer", "Schaefer's Hammer", "..."],
  "uniques_su": ["Eaglehorn", "Hellrush", "..."],
  "uniques_ssu": ["..."],
  "uniques_sssu": ["..."],
  "set_items": ["Civerb's Ward", "Sigon's Gage", "..."],
  "dumped_at": "2026-04-20T15:23:07Z"
}
```

- `schema` — версия приложения, которой был записан кэш (берётся из
  `CARGO_PKG_VERSION`). При `load_items_cache` сравнивается с текущей
  версией приложения; при несовпадении файл игнорируется и кэш ждёт
  пересборки из живой памяти. Это даёт бесплатную принудительную
  инвалидацию: бампнул версию в `Cargo.toml` / `tauri.conf.json` — все
  существующие кэши автоматически отбрасываются на следующем запуске.
  Используй при изменении логики нормализации имён или формата файла.
- `base_types` / `uniques_tu` / `uniques_su` / `uniques_ssu` /
  `uniques_sssu` / `set_items` — отсортированные массивы без
  дубликатов. Поля, отсутствующие в старых файлах, десериализуются в
  пустые массивы через `#[serde(default)]` — старый кэш подхватится
  «частично», полноценно заполнится на первом in-game tick'е.
- `dumped_at` — RFC-3339 (UTC), **только для диагностики**, в рантайме не
  используется.

При изменении формата (например, при добавлении уникалок/сетов):

- Меняется только `items_cache.rs` и структура `ItemsCacheFile`.
- Парсинг падает при несовместимости → `load_items_cache` возвращает
  `None`, логгер пишет `items cache: parse failed: ...`, кэш будет
  перестроен на следующем attach. Старый файл можно просто удалить.

---

## Как расширять

### Уникалки и сеты — уже сделано

См. разделы выше и `docs/item-tables-memory.md`. Константы в
`offsets.rs::{data_tables, unique_items_txt, set_items_txt, items_txt::CODE}`.
Кэши строятся `build_unique_items_cache` / `build_set_items_cache` на
`notifier.rs`.

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

- `build_class_cache` / `build_unique_items_cache` / `build_set_items_cache` —
  разовые для одной сессии сканера, выполняются последовательно на
  первом in-game тике. Объёмы на MedianXL 1.13c: ~2439 items.txt
  записей, ~1822 UniqueItems.txt, ~330 SetItems.txt — каждая запись
  это один `GetStringById` через remote thread (~1–5 мс). Суммарно
  ~5–20 сек, блокирует scanner-поток на первом тике. UI-поток не
  затрагивается. Пользователь заметит задержку перед первым
  drop-notification после входа в игру, но список автокомплита
  доступен через кэш на диске без ожидания.
- Публикация snapshot'а (write Arc + save file + emit event) — единицы
  миллисекунд, одноразово на сессию.
- CodeMirror-фильтрация в popup полностью клиентская. С ~1500-2000
  записей (base + TU + SU + set) лага нет.

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

- **Словарь пуст?** Проверить `%APPDATA%\com.d2mxlutils.app\items-cache.json`
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
