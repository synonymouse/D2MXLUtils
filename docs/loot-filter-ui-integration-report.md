# Отчёт: Интеграция фильтрации с UI

**Дата:** 2025-12-11
**Задача:** Подключить бэкенд фильтрации к UI редактора правил

---

## Исходная проблема

В редакторе правил LootFilterTab можно писать правила (например `hide "Eth Rune$"`), но они не работают — предметы не скрываются на земле.

**Причина:** Бэкенд фильтрации был реализован (`DropScanner.set_filter_config()`, `set_filter_enabled()`), но не был подключён к UI:
1. Не было Tauri-команд для вызова этих методов с фронтенда
2. `AppState` не хранил `FilterConfig`
3. `filter_enabled` всегда был `false`
4. Хук `LootFilterHook` не инжектировался автоматически

---

## Что было сделано

### 1. Backend: Расширен AppState (main.rs)

```rust
struct AppState {
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
    // НОВОЕ:
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    filter_enabled: Arc<AtomicBool>,
}
```

### 2. Backend: Добавлены Tauri-команды (main.rs:279-301)

- `set_filter_config(config)` — установить конфиг фильтра
- `set_filter_enabled(enabled)` — включить/выключить фильтрацию
- `get_filter_enabled()` — получить текущий статус

### 3. Backend: Интеграция с сканером (main.rs)

- `start_scanner_internal()` теперь принимает `filter_config` и `filter_enabled`
- После создания `DropScanner` вызываются `set_filter_config()` и `set_filter_enabled()`
- `spawn_auto_scanner()` также передаёт эти параметры

### 4. Backend: Автоинжект хука (notifier.rs)

- Добавлено поле `loot_hook: LootFilterHook` в `DropScanner`
- В `DropScanner::new()` хук автоматически инжектируется
- В `set_filter_enabled()` флаг синхронизируется с хуком
- Реализован `Drop` trait для eject хука при уничтожении сканера

### 5. Backend: Signature scanning (process.rs, loot_filter_hook.rs)

- Добавлен метод `ProcessHandle::scan_pattern()` для поиска по сигнатуре
- `LootFilterHook::inject()` теперь ищет функцию по сигнатуре вместо фиксированного offset
- Это решает проблему совместимости с разными версиями D2Sigma.dll

### 6. Frontend: Переделан UI (LootFilterTab.svelte)

Вместо непонятного toggle "Enable" добавлен segmented control:

```
[Show All] [Apply Rules]
```

- **Show All** — фильтрация выключена, все предметы видны
- **Apply Rules** — фильтрация включена, применяются правила

### 7. Frontend: Синхронизация конфига

- При загрузке профиля (`handleProfileLoad`) — конфиг отправляется в бэкенд
- При сохранении профиля (`handleSaveComplete`) — конфиг синхронизируется
- При переключении режима — вызывается `set_filter_enabled`

---

## Изменённые файлы

| Файл | Изменения |
|------|-----------|
| `src-tauri/src/main.rs` | AppState, команды, передача в сканер |
| `src-tauri/src/notifier.rs` | LootFilterHook интеграция, Drop trait |
| `src-tauri/src/process.rs` | scan_pattern() для signature scanning |
| `src-tauri/src/loot_filter_hook.rs` | Signature scanning вместо фиксированного offset |
| `src/views/LootFilterTab.svelte` | Segmented control, синхронизация конфига |

---

## Текущий статус

### Работает:
- ✅ UI переключатель режимов `[Show All] [Apply Rules]`
- ✅ Конфиг фильтра передаётся в бэкенд
- ✅ Хук находится по сигнатуре (адрес `D2Sigma+CCCD0`)
- ✅ Хук инжектируется успешно
- ✅ Флаг `filter_enabled` синхронизируется
- ✅ Имена предметов получаются корректно (`get_item_name()` работает)

### Не работает:
- ❌ Предметы не скрываются на земле

---

## Диагностика проблемы

Хук инжектируется, имена предметов получаются, но фильтрация не работает.

### Вероятная причина:

**Трамплин-код хука некорректен** — нужно проверить сгенерированный машинный код в Cheat Engine.

Возможные проблемы:
1. Неправильные адреса глобальных флагов в коде хука
2. Неправильные смещения для чтения `iEarLevel`
3. Неправильный JMP на продолжение оригинальной функции
4. `iEarLevel` не записывается в память предмета

---

## Что нужно сделать

### Приоритет 1: Проверить трамплин-код в Cheat Engine

Пользователь предоставит дамп инъектированного кода из CE. Нужно сверить:
- Адреса `g_show_all_loot` и `g_filter_enabled` в коде
- Смещение `+0x48` для `iEarLevel`
- JMP адрес на продолжение оригинальной функции (`hook_address + 9`)

**Файл:** `src-tauri/src/loot_filter_hook.rs:generate_trampoline_code()`

### Приоритет 2: Проверить запись iEarLevel

Добавить логирование в `set_item_visibility()` чтобы убедиться что значение записывается.

```rust
log_info(&format!(
    "set_item_visibility: p_unit_data=0x{:08X}, visible={}, writing to 0x{:08X}",
    p_unit_data, visible, p_unit_data as usize + 0x48
));
```

**Файл:** `src-tauri/src/notifier.rs:set_item_visibility()`

### Приоритет 3: Добавить debug-логирование в tick()

После применения фильтра логировать:
- Какое правило сработало
- Какой action был получен (`show_item: true/false`)
- Результат вызова `set_item_visibility()`

### Приоритет 4: Проверить смещение iEarLevel

Убедиться что `+0x48` — правильное смещение для `iEarLevel` в текущей версии игры.

**Файл:** `src-tauri/src/offsets.rs` — константа `item_data::EAR_LEVEL`

---

## Данные для следующей сессии

Пользователь предоставит дамп инъектированного кода из Cheat Engine:
1. **Трамплин-код** — дизассемблированный код по адресу trampoline (например `0x0A8D0000`)
2. **Патч в D2Sigma** — что записалось по адресу `D2Sigma+CCCD0`
3. **Значения глобальных флагов** — содержимое `g_show_all` и `g_filter_en`

Это позволит:
- Сверить сгенерированный код с ожидаемым
- Найти ошибку в `generate_trampoline_code()`
- Исправить хук

---

## Полезные ссылки

- Спецификация хука: `docs/loot-filter-hook-specification.md`
- Общая спецификация: `docs/loot-filter-spec.md`
- Предыдущий отчёт: `docs/loot-filter-implementation-report.md`

---

## Архитектура (для понимания)

```
Frontend (LootFilterTab)
    │
    ├─► set_filter_config(config)  ──► AppState.filter_config
    │
    └─► set_filter_enabled(true)   ──► AppState.filter_enabled
                                            │
                                            ▼
                                   DropScanner.set_filter_enabled()
                                            │
                                            ├─► self.filter_enabled = true
                                            │
                                            └─► loot_hook.set_filter_enabled()
                                                    │
                                                    ▼
                                            [g_filter_enabled] = 1
                                            (в памяти игры)

DropScanner.tick() цикл:
    │
    ├─► scan_unit() → ScannedItem
    │       └─► get_item_name() → "" (ПРОБЛЕМА!)
    │
    ├─► to_event() → ItemDropEvent
    │
    └─► if filter_enabled:
            │
            ├─► filter.get_action(&ctx) → RuleAction
            │       └─► Правила не матчатся (имя пустое)
            │
            └─► set_item_visibility(p_unit_data, show_item)
                    └─► Записывает iEarLevel в [pUnitData+0x48]

D2Sigma.dll (хук):
    │
    └─► LootFilter_ShouldShowItem(pUnit)
            │
            ├─► Проверяет g_filter_enabled
            ├─► Читает iEarLevel из [pUnitData+0x48]
            │
            └─► Возвращает TRUE/FALSE
```
