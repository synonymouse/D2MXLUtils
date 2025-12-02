# D2MXLUtils Progress Checkpoint — 01.12.2025

## Обзор

Продолжается рефакторинг D2Stats в D2MXLUtils согласно [плану](./d2mxlutils-refactoring.plan.md).

---

## ✅ Выполненная работа

### 1. Базовый каркас Tauri (п.1-2 плана) — *выполнено ранее*
- Инициализирован проект Tauri v2 + Svelte 5 + TypeScript + Tailwind 4
- Настроены базовые команды `start_scanner` / `stop_scanner`
- Настроен IPC между Rust и фронтендом

### 2. Модуль `process.rs` (п.3 плана) — *выполнено ранее*
- RAII-обёртка `ProcessHandle` с автоматическим закрытием через `Drop`
- `open_process_by_window_class("Diablo II")` — поиск окна и открытие процесса
- `read_memory<T>`, `read_buffer`, `write_buffer` — чтение/запись памяти
- `get_module_base` — получение базового адреса DLL через `EnumProcessModules`
- `D2Context` — контекст с базовыми адресами D2Client, D2Common, D2Win, D2Lang, D2Sigma

### 3. Модуль `offsets.rs` (п.5 плана) — ✅ СЕГОДНЯ
Создан модуль с константами оффсетов из оригинального D2Stats.au3:

```
src-tauri/src/offsets.rs
├── d2client::          — оффсеты D2Client.dll
│   ├── PLAYER_UNIT (0x11BBFC)
│   ├── MERCENARY_UNIT, NO_PICKUP_FLAG
│   ├── INJECT_BASE (0xCDE00)
│   ├── inject:: (PRINT, GET_STRING, GET_ITEM_NAME, GET_ITEM_STAT)
│   └── func:: (PRINT_STRING, GET_ITEM_NAME, GET_ITEM_STAT)
├── d2common::          — оффсеты D2Common.dll
│   ├── ITEMS_TXT, GET_UNIT_STAT
│   └── INJECT_GET_UNIT_STAT
├── paths::             — оффсеты для итерации по комнатам/предметам
├── unit::              — смещения полей UnitAny
├── item_data::         — смещения полей ItemData
├── inventory::         — смещения Inventory
├── items_txt::         — структура Items.txt записи
├── unit_type::         — константы типов юнитов (PLAYER, MONSTER, ITEM, ...)
├── item_quality::      — качество предметов (MAGIC, RARE, UNIQUE, ...)
└── item_flags::        — флаги (IDENTIFIED, ETHEREAL, SOCKETED, RUNEWORD)
```

### 4. Модуль `d2types.rs` (п.5 плана) — ✅ СЕГОДНЯ
Созданы `#[repr(C)]` структуры для прямого чтения из памяти игры:

```rust
pub struct UnitAny      // Базовая структура юнита (игроки, монстры, предметы)
pub struct ItemData     // Расширенные данные предмета (качество, флаги, ...)
pub struct UniqueItemsTxt // Запись UniqueItems.txt
pub struct ScannedItem  // Высокоуровневое представление найденного предмета
pub struct Inventory    // Инвентарь игрока
pub enum PrintColor     // Цвета для PrintString
```

### 5. Модуль `injection.rs` (п.4 плана) — ✅ СЕГОДНЯ
Реализован механизм инъекции кода и вызова функций D2:

```rust
pub struct RemoteAlloc      // Выделение памяти в целевом процессе (VirtualAllocEx)
pub fn remote_thread(...)   // Вызов функции через CreateRemoteThread
pub struct D2Injector {
    string_buffer,          // Буфер для строк в памяти игры
    params_buffer,          // Буфер для параметров
    inject_*,               // Адреса инъектированных функций
}
```

Методы `D2Injector`:
- `new()` — создание и инъекция всех функций
- `inject_functions()` — запись машинного кода в память D2
- `get_item_name(pUnit)` — получить имя предмета
- `get_item_stats(pUnit)` — получить статы предмета
- `get_unit_stat(pUnit, statId)` — получить значение стата
- `print_string(text, color)` — вывод текста в чат игры

### 6. Модуль `notifier.rs` (п.6 плана) — ✅ СЕГОДНЯ
Реализован сканер предметов на земле:

```rust
pub struct ItemDropEvent    // Payload события для фронтенда
pub struct DropScanner {
    ctx: D2Context,
    injector: D2Injector,
    seen_items: HashSet<u32>,  // Кэш уже уведомлённых предметов
}
```

Методы:
- `new()` — подключение к D2 и инициализация инжектора
- `is_ingame()` — проверка, в игре ли игрок
- `clear_cache()` — очистка кэша (при входе в новую игру)
- `tick()` — один цикл сканирования, возвращает Vec<ItemDropEvent>
- Итерация по paths → rooms → units → items

### 7. Модуль `rules.rs` (п.7 плана) — ✅ СЕГОДНЯ
Реализована загрузка и применение JSON-правил фильтрации:

```rust
pub enum RuleType       // Class, Quality, Name, All
pub enum EtherealMode   // Any, Required, Forbidden
pub struct RuleParams   // class, name, stat_id, stat_min/max
pub struct RuleAction   // show_item, notify, automap, color, sound
pub struct Rule         // Одно правило фильтрации
pub struct FilterConfig // Конфиг с массивом правил
```

Методы:
- `Rule::matches(&item)` — проверка соответствия предмета правилу
- `FilterConfig::load(path)` / `save(path)` — работа с JSON-файлами
- `FilterConfig::get_action(&item)` — определение действия для предмета
- `create_sample_config()` — пример конфигурации

### 8. Обновлённый `main.rs`
- Фоновый поток сканирования с интервалом 200мс
- События: `scanner-status`, `game-status`, `item-drop`
- Команды: `start_scanner`, `stop_scanner`, `get_scanner_status`
- Автоматическое определение входа/выхода из игры

### 9. Первый прогон с запущенной Diablo II (MedianXL) — ✅ СЕГОДНЯ

- Пересобран backend как 32‑битный (`i686-pc-windows-msvc`), dev‑запуск из административной консоли
- Успешное прикрепление к процессу игры, корректное определение модулей `D2Client.dll` / `D2Common.dll`
- `DropScanner` стабильно находит предметы на земле и шлёт события `item-drop` в UI:
  - пример лога: `Found item: Death Spur (Unique)`, `Found item: Eth Rune (Normal)`
- Инъекционный слой `injection.rs` доведён до рабочего состояния: байткод инъекций сверен с оригинальным `D2Stats.au3`, безопасные вызовы `get_item_name` / `get_item_stats` работают без крашей клиента и переживают повторные перезапуски сканера

---

## 🔲 Предстоящая работа

### 8. Оверлей-окно (п.8 плана) — ✅ СЕГОДНЯ
- [x] Настроить окно Tauri как прозрачное, без рамки, `always_on_top` (отдельное окно `overlay`)
- [x] Установить флаги WS_EX_LAYERED | WS_EX_TRANSPARENT (клик-сквозь)
- [x] Скрыть из Alt+Tab (WS_EX_TOOLWINDOW)
- [x] Периодическая синхронизация позиции/размера с окном Diablo II по HWND (FindWindowW + GetWindowRect + MoveWindow/SetWindowPos)
- [x] Режим overlay в Svelte (`App.svelte`): отдельный layout для окна `overlay`, прозрачный фон, стек уведомлений по drop’ам
- [ ] Исследовать и при необходимости донастроить режим отображения для максимально бесшовного borderless fullscreen (ограничения эксклюзивного fullscreen зафиксированы в `docs/overlay-fullscreen-notes.md`)

### 9. Svelte UI (п.9 плана)
- [ ] Компоненты NotificationList, NotificationItem
- [ ] Svelte store для очереди уведомлений
- [ ] Подписка на события `item-drop`, `scanner-status`, `game-status`
- [ ] Tailwind стили и анимации
- [ ] Настройки фильтра в UI

### 10. Интеграция правил в сканер
- [ ] Загрузка FilterConfig при старте
- [ ] Применение правил перед отправкой события
- [ ] UI для редактирования правил

### 11. Финальная полировка
- [ ] Обработка ошибок и reconnect при потере процесса
- [ ] Логирование (tracing или log crate)
- [ ] Настройки приложения (путь к конфигу, звуки, и т.д.)
- [ ] Иконка и метаданные приложения

---

## 📁 Текущая структура src-tauri/src

```
src-tauri/src/
├── main.rs         — точка входа, Tauri команды, фоновый сканер
├── process.rs      — работа с процессом D2 (WinAPI)
├── offsets.rs      — константы смещений памяти
├── d2types.rs      — repr(C) структуры D2
├── injection.rs    — инъекция кода и вызов функций D2
├── notifier.rs     — DropScanner, сканирование предметов
└── rules.rs        — JSON-фильтры правил
```

---

## 🧪 Следующие шаги

1. Реализовать overlay-окно (поведение как у `electron-overlay-window`)
2. Обновить Svelte UI: отдельный оверлей над окном Diablo II, визуальное оформление уведомлений, авто‑очистка
3. Подключить JSON‑правила (`rules.rs`) к `DropScanner` и добавить простой UI для выбора/редактирования фильтра

---

## Обновление 02.12.2025 (логирование и проблема прав доступа)

- Добавлен модуль `logger.rs` — простой файловый логгер, пишущий строки в `d2mxlutils.log` рядом с exe.
- Логирование подключено к основным путям выполнения в `main.rs` и `notifier.rs`, чтобы видеть:
  - старт/стоп сканера и статусы игры;
  - адреса модулей D2Client/D2Common и результаты инъекций;
  - ошибки чтения структур и вызова инъектированных функций.
- С помощью логов зафиксирована проблема `Access is denied (0x80070005)` при `OpenProcess` в релизной сборке при запуске exe напрямую; поведение задокументировано отдельно в `docs/d2mxlutils-elevation-issue.md`.

