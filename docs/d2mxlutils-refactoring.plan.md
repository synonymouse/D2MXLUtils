# D2MXLUtils Refactoring Plan (Tauri + Rust + Svelte)

## Общее описание

Refакторим `D2Stats` в `D2MXLUtils` для игры Diablo 2 LoD с модом MedianXL, оставляя только функционал Drop Notifier. Стек: **Tauri (Rust backend)** + **Svelte + TypeScript + Tailwind**. Все низкоуровневые операции (чтение памяти, инъекции, управление окнами) реализуем на Rust, используя официальный crate [`windows`](https://learn.microsoft.com/ru-ru/windows/dev-environment/rust/rust-for-windows?utm_source=openai) для доступа к WinAPI. Фронтенд — Svelte-оверлей, который получает события из Rust через Tauri.

---

## 1. Создание проекта Tauri + Svelte + Tailwind

**Цель:** Получить базовый каркас приложения.

- **Статус (27.11.2025):**  
  - В корне репозитория инициализирован каркас приложения на **Tauri v2 + Rust**, **Svelte 5 + TypeScript**, **Vite 7** и **Tailwind 4** (через `@tailwindcss/vite` — см. [Using Vite](https://tailwindcss.com/docs/installation/using-vite)).  
  - Веб‑часть живёт в `src/`, backend — в `src-tauri/`, конфиг Tauri приведён к схеме v2 (`tauri.conf.json`).  
  - Пакетный менеджер — `pnpm`, dev‑команда: `pnpm tauri dev` (поднимает Vite‑dev‑сервер и Tauri‑окно).

- Установить `Rust`, `cargo`, `npm`.
- `npm create tauri-app@latest` → выбрать **Svelte + TypeScript**.
- Подключить Tailwind (`tailwind.config.cjs`, `postcss.config.cjs`).
- Структура:
  - `src-tauri/` — Rust backend.
  - `src/` — Svelte UI.

## 2. Rust backend: Tauri-команды и события

**Цель:** Организовать коммуникацию между Rust и UI.

- **Статус (27.11.2025):**
  - Реализованы команды `start_scanner` / `stop_scanner` в `src-tauri/src/main.rs`.
  - Настроено состояние приложения `AppState` с `Mutex`.
  - Подключены capabilities `core:default` для работы IPC.
  - Svelte-фронтенд (`App.svelte`) успешно вызывает команды и слушает события `scanner-status`.

- В `src-tauri/src/main.rs` определить Tauri-команды (`#[tauri::command]`):
  - `start_scanner()` / `stop_scanner()`.
- Использовать `app_handle.emit_all("item-drop", payload)` для отправки уведомлений о дропе.
- Настроить `tauri.conf.json` (Windows, отключаем sandbox, включаем необходимые permissions).

## 3. Rust: слой работы с процессом D2 (WinAPI через crate `windows`)

**Цель:** Повторить `_MemoryOpen`, `_MemoryRead`, `_MemoryWrite`, `UpdateDllHandles`.

- Модуль `process` (`src-tauri/src/process.rs`):
  - Использовать crate `windows`/`windows-sys` для вызова `OpenProcess`, `ReadProcessMemory`, `VirtualAllocEx`, `CreateRemoteThread`, `EnumProcessModules` и т.д.
  - Реализовать функции:
    - `open_process_by_window_class("Diablo II")` → `FindWindowW`, `GetWindowThreadProcessId`, `OpenProcess`.
    - RAII-обёртка `ProcessHandle` (закрытие через `Drop`).
    - `read_memory<T>(address: u32) -> Result<T>`, `read_buffer(address, size) -> Result<Vec<u8>>`, `write_buffer(...)`.
    - `get_module_base("D2Client.dll")` через `EnumProcessModules`/`GetModuleBaseNameW`.
- Менеджер состояния `D2Context { process, d2client_base, d2common_base, ... }` и функция `update_dll_handles()` (аналог AutoIt, но без `LoadLibraryA`).

## 4. Rust: инъекция и вызов внутренних функций D2

**Цель:** Перенести `RemoteThread`, `InjectFunctions`, `GetItemName`, `GetItemStats`.

- Модуль `injection` (`src-tauri/src/injection.rs`):
  - Обёртки над `VirtualAllocEx`, `VirtualFreeEx`, `CreateRemoteThread`, `WaitForSingleObject`, `GetExitCodeThread`.
  - `remote_thread(func_address: u32, param: u32) -> Result<u32>`.
- Настроить смещения относительно `D2Client.dll`: `pD2InjectPrint`, `pD2Client_GetItemName`, `pD2Client_GetItemStat`, `pD2Common_GetUnitStat` (как в AutoIt, строка 2872+).
- Выделить буферы в памяти игры для строк/чисел.
- API для сканера: `get_item_name(p_unit) -> Result<String>`, `get_item_stats(p_unit) -> Result<String>`.

## 5. D2 структуры и оффсеты

**Цель:** Типобезопасные представления `UnitAny`, `ItemData`, константы оффсетов.

- Модуль `d2types` (`src-tauri/src/d2types.rs`): `#[repr(C)] `структуры с нужными полями (по `NotifierMain`).
- Модуль `offsets` (`src-tauri/src/offsets.rs`):
  - `UNIT_LIST_PTR = 0x11BBFC`.
  - Смещения до списков путей, `pUnit`, `pUnitData`, `earLevel` и т.д.
  - Смещения к `Items.txt`, если пригодится.

## 6. Rust: сканер предметов (Drop Notifier Core)

**Цель:** Перенести `NotifierMain`, `NotifierCache`, `ProcessItems`, `OnGroundFilterItems`.

- Модуль `notifier` (`src-tauri/src/notifier/mod.rs`):
  - `DropScanner` с состоянием (`ctx`, кэш увиденных предметов).
  - Метод `tick(&mut self, app_handle)`:
    - Проверяет, в игре ли игрок (аналог `IsIngame`).
    - Обходит Unit-список, находит предметы (`unit_type == 4`).
    - Читает `ItemData` (качество, flags, tier, `earLevel`).
    - Использует `get_item_name` и `get_item_stats` при необходимости.
    - Передаёт данные в фильтр.
    - Для подходящих предметов эмитит `item-drop` в UI.
- Фоновый поток: запуск `DropScanner` с интервалом (например, 200 мс).

## 7. Rust: новое JSON-хранилище правил

**Цель:** Заменить старый `.rules` формат на JSON, как в примере.

- Структура конфигурации:
```json
{
  "default_show_items": true,
  "name": "SimpleFilterSoftNotify",
  "rules": [
    {
      "active": true,
      "automap": false,
      "ethereal": 0,
      "item_quality": 1,
      "max_clvl": 0,
      "max_ilvl": 0,
      "min_clvl": 0,
      "min_ilvl": 0,
      "notify": false,
      "params": {"class": 25},
      "rule_type": 0,
      "show_item": false
    }
  ]
}
```

- Реализовать модуль `rules` (`src-tauri/src/rules.rs`):
  - Сердец/Deserialize через `serde`.
  - `RuleType` enum, интерпретация `item_quality`, `ethereal`, `min/max level`, `params.class` и т.д.
  - Функция `match_rule(item: &ScannedItem) -> Option<RuleAction>`.
- `DropScanner` перед отправкой уведомления проверяет правила и решает:
  - Показывать ли предмет (`show_item`).
  - Делать ли notify (`notify`).
  - Какие дополнительные действия (цвет, звук, auto map).

## 8. Оверлей-окно на Tauri

**Цель:** Добиться поведения как у `electron-overlay-window`.

- Настроить окно Tauri как прозрачное, без рамки, `always_on_top`. **(выполнено 01.12.2025)**
- Через WinAPI выставить флаги `WS_EX_LAYERED | WS_EX_TRANSPARENT` (клик-сквозь). **(выполнено 01.12.2025)**
- Скрыть окно из Alt+Tab (`WS_EX_TOOLWINDOW`). **(выполнено 01.12.2025)**
- Периодически синхронизировать позицию/размер с окном Diablo II (по HWND). **(выполнено 01.12.2025, см. ограничения в полноэкранном режиме — `docs/overlay-fullscreen-notes.md`)**

## 11. Логирование и отладка прав доступа

**Цель:** Иметь удобный способ понимать, что происходит в релизной сборке (особенно вокруг инъекций и прав доступа).

- Модуль `logger` (`src-tauri/src/logger.rs`):
  - Простой файловый логгер, пишущий строки в `d2mxlutils.log` рядом с исполняемым файлом.
  - Функции `info()` и `error()` для записи ключевых событий и ошибок.
- Логирование интегрировано в:
  - `main.rs` — старт/стоп сканера, статусы игры, ошибки эмита событий.
  - `notifier.rs` — инициализация `DropScanner`, ошибки чтения структур, вызовы инъекционных функций.
- На основе логов зафиксирована и описана проблема прав доступа (`ACCESS_DENIED` при `OpenProcess`) при некоторых сценариях запуска релизного exe — см. `docs/d2mxlutils-elevation-issue.md`.

## 9. Svelte + Tailwind UI

**Цель:** Простой оверлей уведомлений.

- `src/App.svelte` + компоненты `NotificationList`, `NotificationItem`.
- Tailwind стили + анимации.
- Svelte store `notifications` (очередь уведомлений, автоудаление).
- Подписка на `item-drop` через `@tauri-apps/api/event`.
- Отображение данных, пришедших от фильтра (цвет границы, текст, доп.инфо).

## 10. Порядок реализации

1. Инициализация проекта (Tauri + Svelte + Tailwind).
2. Заглушечный `item-drop` event для проверки UI.
3. Реализация модуля `process` (открытие/чтение памяти через `windows` crate).
4. Описание структур и оффсетов (`d2types`, `offsets`).
5. Модуль `injection` и функции `get_item_name`/`get_item_stats`.
6. Реализация `DropScanner` (цикл сканирования и события).
7. Модуль правил, чтение JSON-конфига и фильтрация.
8. Настройка оверлей-окна (клики насквозь, синхронизация координат).
9. Финал: полировка UI, обработка ошибок, логирование.

---

## TODOs

1. `setup-tauri`: Инициализировать проект Tauri с фронтендом Svelte + TypeScript + Tailwind. **(выполнено)**
2. `rust-backend-base`: Настроить базовые Tauri-команды и event-эмиттер. **(выполнено)**
3. `rust-process-layer`: Реализовать модуль процесса (через crate `windows`). **(выполнено)**
4. `rust-d2-types-offsets`: Описать структуры и оффсеты. **(выполнено 01.12.2025)**
5. `rust-injection-layer`: Реализовать инъекции и вызовы внутренних функций D2. **(выполнено 01.12.2025, требуется доработка байткода и протокола вызова)**
6. `rust-drop-scanner`: Собрать DropScanner и сканирование предметов. **(выполнено 01.12.2025, базовое сканирование без имён/статов через инъекции)**
7. `rust-rules-json`: Загрузка и интерпретация JSON-фильтров (новая структура). **(выполнено 01.12.2025)**
8. `overlay-window`: Реализовать поведение оверлея в Tauri. **(базовое окно и синхронизация реализованы 01.12.2025, ограничения fullscreen — см. `docs/overlay-fullscreen-notes.md`)**
9. `svelte-ui`: Создать Svelte/Tailwind UI и связать с событиями.
10. `diagnostics-logging`: Добавить файловый логгер и задокументировать проблемы прав доступа. **(выполнено 02.12.2025, см. `logger.rs` и `docs/d2mxlutils-elevation-issue.md`)**

---

## Progress Log

**01.12.2025** — базовая реализация backend‑слоёв (`process`, `offsets`, `d2types`, `injection`, `notifier`, `rules`), фоновый сканер, события в Tauri.

**01.12.2025 (вечер)** — первый успешный прогон с живой Diablo II (MedianXL): сканер цепляется к процессу, находит предметы и шлёт события `item-drop` в UI; инъекционный слой временно работает в ограниченном режиме (без вызова `get_item_name` / `get_item_stats` до доработки.