# Отчёт о реализации хуков лутфильтра

**Дата:** 2025-12-11
**Обновлено:** 2025-12-11 (после ревью)
**Задача:** Реализация бэкенда для системы hide/show предметов на земле

---

## Резюме

Реализована серверная часть системы фильтрации предметов согласно спецификациям:
- `docs/loot-filter-spec.md` — общая спецификация лутфильтра
- `docs/loot-filter-hook-specification.md` — спецификация хука D2Sigma.dll

✅ Все компоненты успешно компилируются.
✅ Автоматическая фильтрация интегрирована в `tick()`.
✅ Хук защищён верификацией сигнатуры и VirtualProtectEx.

---

## Изменённые файлы

| Файл | Изменения |
|------|-----------|
| `src-tauri/src/d2types.rs` | Добавлено поле `p_unit_data` в `ScannedItem` |
| `src-tauri/src/notifier.rs` | Добавлены `p_unit_data`, `set_item_visibility()`, `context()`, интеграция фильтра в `tick()`, методы `set_filter_config()`, `set_filter_enabled()` |
| `src-tauri/src/rules/mod.rs` | Система приоритетов: `get_action()`, `select_highest_priority_rule()`, `flag_count()`, `has_display_color()` |
| `src-tauri/src/rules/matching.rs` | Добавлен `stat_pattern_matched()` |
| `src-tauri/src/process.rs` | Добавлен `read_buffer_into()` |
| `src-tauri/src/main.rs` | Зарегистрирован модуль, добавлены Tauri-команды |
| `src-tauri/src/loot_filter_hook.rs` | **Новый файл** — модуль хука с VirtualProtectEx и верификацией сигнатуры |

---

## Детали реализации

### 1. Структуры данных

#### ScannedItem (`d2types.rs:124-148`)
```rust
pub struct ScannedItem {
    pub p_unit: u32,
    pub p_unit_data: u32,  // указатель на ItemData
    // ...
}
```

#### ItemDropEvent (`notifier.rs:21-30`)
```rust
pub struct ItemDropEvent {
    // ...существующие поля...
    pub p_unit_data: u32,  // для set_item_visibility
}
```

---

### 2. Система приоритетов правил

Реализована трёхуровневая система приоритетов согласно `loot-filter-spec.md`:

| Приоритет | Критерий | Описание |
|-----------|----------|----------|
| 1 (высший) | Stat Match | Правило имеет `stat_pattern` И он совпал с предметом |
| 2 (средний) | Color Flag | Правило указывает цвет (не hide/show) |
| 3 (низший) | Flag Count | Правило с большим числом критериев более специфично |

#### Новые методы в `rules/mod.rs`:

**`get_action()` (строки 558-584)**
- Собирает все matching правила
- Выбирает победителя по приоритету
- Возвращает `RuleAction`

**`select_highest_priority_rule()` (строки 586-614)**
- Реализует логику приоритетов
- Возвращает ссылку на победившее правило

**`flag_count()` (строки 465-493)**
- Подсчитывает количество активных критериев в правиле
- Используется для определения специфичности

**`has_display_color()` (строки 496-501)**
- Проверяет, указан ли реальный цвет (не hide/show)

#### Новый метод в `rules/matching.rs`:

**`stat_pattern_matched()` (строки 124-138)**
- Проверяет совпадение `stat_pattern` со статами предмета
- Используется для определения приоритета 1

---

### 3. Модуль хука (`loot_filter_hook.rs`)

Полностью новый модуль (~420 строк) для инжекта кода в D2Sigma.dll.

#### Константы
```rust
const HOOK_OFFSET: usize = 0xCBCD0;  // Адрес функции лутфильтра
const PATCH_SIZE: usize = 10;         // Размер патча

/// Ожидаемые байты для верификации сигнатуры (защита от несовместимых версий игры)
const EXPECTED_SIGNATURE: [u8; 9] = [0x83, 0xEC, 0x08, 0x53, 0x55, 0x8B, 0xD9, 0x56, 0x57];
```

#### Структура LootFilterHook
```rust
pub struct LootFilterHook {
    hook_address: usize,        // D2Sigma.dll + 0xCBCD0
    trampoline_address: usize,  // Адрес нашего кода
    g_show_all_loot: usize,     // Глобальный флаг (Alt-режим)
    g_filter_enabled: usize,    // Глобальный флаг (вкл/выкл)
    original_bytes: [u8; 10],   // Сохранённые оригинальные байты
    is_injected: bool,
    process_handle: HANDLE,
}
```

#### Публичные методы

| Метод | Описание |
|-------|----------|
| `new()` | Создать новый экземпляр (не инжектированный) |
| `inject(&mut self, ctx: &D2Context)` | Инжектировать хук в D2Sigma.dll |
| `eject(&mut self, ctx: &D2Context)` | Удалить хук, восстановить оригинальные байты |
| `set_show_all(&self, ctx, show: bool)` | Установить глобальный флаг показа всех предметов |
| `set_filter_enabled(&self, ctx, enabled: bool)` | Включить/выключить фильтр |
| `is_injected(&self)` | Проверить, инжектирован ли хук |

#### Защитные механизмы (добавлено после ревью)

**Верификация сигнатуры** (`inject()`, шаг 6):
```rust
if self.original_bytes[..9] != EXPECTED_SIGNATURE {
    return Err(format!(
        "Signature mismatch at D2Sigma+{:X}. Expected {:02X?}, got {:02X?}. Wrong game version?",
        HOOK_OFFSET, EXPECTED_SIGNATURE, &self.original_bytes[..9]
    ));
}
```

**VirtualProtectEx** (`inject()` и `eject()`):
```rust
// Снятие защиты перед записью
VirtualProtectEx(ctx.process.handle, self.hook_address, PATCH_SIZE, PAGE_EXECUTE_READWRITE, &mut old_protect)?;

// Запись патча
ctx.process.write_buffer(self.hook_address, &jmp_patch)?;

// Восстановление защиты
VirtualProtectEx(ctx.process.handle, self.hook_address, PATCH_SIZE, old_protect, &mut old_protect);
```

#### Логика хука (x86 assembly)

```
1. Проверить g_bShowAllLoot
   - Если FALSE → return FALSE (скрыть всё)

2. Проверить g_bFilterEnabled
   - Если FALSE → вызвать оригинальный код

3. Проверить pUnit != NULL
   - Если NULL → вызвать оригинальный код

4. Получить pUnitData = [pUnit+0x14]
   - Если NULL → вызвать оригинальный код

5. Проверить iEarLevel = [pUnitData+0x48]
   - 2 → return FALSE (скрыть)
   - 1 → return TRUE (показать)
   - 0 → вызвать оригинальный код
```

---

### 4. Функция записи видимости

#### `set_item_visibility()` (`notifier.rs`)
```rust
pub fn set_item_visibility(&self, p_unit_data: u32, visible: bool) -> Result<(), String> {
    if p_unit_data == 0 {
        return Err("p_unit_data is null".to_string());
    }
    let value: u8 = if visible { 1 } else { 2 };
    let addr = p_unit_data as usize + item_data::EAR_LEVEL;
    self.ctx.process.write_buffer(addr, &[value])
}
```

Значения `iEarLevel`:
- `0` — не обработан (default, решает оригинальный код)
- `1` — показать предмет
- `2` — скрыть предмет

---

### 5. Интеграция фильтра в tick() (добавлено после ревью)

#### DropScanner (`notifier.rs`)

Новые поля:
```rust
pub struct DropScanner {
    // ...существующие поля...
    filter_config: Option<Arc<RwLock<FilterConfig>>>,
    filter_enabled: bool,
}
```

Новые методы:
```rust
/// Установить конфиг фильтра
pub fn set_filter_config(&mut self, config: Arc<RwLock<FilterConfig>>)

/// Включить/выключить автоматическую фильтрацию
pub fn set_filter_enabled(&mut self, enabled: bool)

/// Проверить, включена ли фильтрация
pub fn is_filter_enabled(&self) -> bool
```

Интеграция в `tick()`:
```rust
while p_unit != 0 {
    if let Some(scanned) = self.scan_unit(p_unit) {
        let event = Self::to_event(scanned);

        // Автоматическое применение фильтра
        if self.filter_enabled {
            if let Some(ref filter_arc) = self.filter_config {
                if let Ok(filter) = filter_arc.read() {
                    let ctx = MatchContext::new(&event);
                    let action = filter.get_action(&ctx);
                    let _ = self.set_item_visibility(event.p_unit_data, action.show_item);
                }
            }
        }

        events.push(event);
    }
    // ...
}
```

---

### 6. Tauri-команды

#### `apply_item_filter` (`main.rs:279-303`)
Применяет фильтр к предмету по `p_unit_data`.

```typescript
// Frontend usage:
await invoke('apply_item_filter', { pUnitData: item.p_unit_data, visible: false });
```

> **Note:** Для автоматической фильтрации во время сканирования используйте встроенную интеграцию DropScanner, которая переиспользует D2Context сканера. Эта команда создаёт новый D2Context при каждом вызове для простоты и thread-safety.

#### `get_item_filter_action` (`main.rs:305-314`)
Возвращает действие для предмета на основе правил фильтра.

```typescript
// Frontend usage:
const action = await invoke('get_item_filter_action', { config: filterConfig, item: itemEvent });
// action: { show_item: boolean, notify: boolean, color?: string, sound?: string }
```

---

### 7. Вспомогательные изменения

#### `read_buffer_into()` (`process.rs:84-103`)
Новый метод для чтения в существующий буфер (нужен для сохранения оригинальных байт хука).

#### Non-Windows stubs (`notifier.rs`)
Добавлены stub-методы для компиляции на не-Windows платформах:
- `set_filter_config()`
- `set_filter_enabled()`
- `is_filter_enabled()`
- `context()`
- `set_item_visibility()`

---

## Архитектура потока данных

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Текущий поток (полностью реализовано)                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  DropScanner.tick()                                                     │
│         │                                                               │
│         ├─► scan_unit() → ScannedItem (с p_unit_data)                   │
│         │                                                               │
│         ├─► to_event() → ItemDropEvent (с p_unit_data)                  │
│         │                                                               │
│         ├─► [НОВОЕ] Если filter_enabled:                                │
│         │       get_action() → set_item_visibility()                    │
│         │                                                               │
│         └─► emit "item-drop" → Frontend                                 │
│                                                                         │
│  Frontend может вызвать:                                                │
│         │                                                               │
│         ├─► get_item_filter_action(config, item) → RuleAction           │
│         │                                                               │
│         └─► apply_item_filter(p_unit_data, visible) → записать iEarLevel│
│                                                                         │
│  D2Sigma.dll+CBCD0 (хук, готов к инжекту)                               │
│         │                                                               │
│         └─► Читает iEarLevel → возвращает TRUE/FALSE                    │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Что осталось (следующие шаги)

| Компонент | Описание | Сложность |
|-----------|----------|-----------|
| UI для включения фильтрации | Кнопка/тогл для вызова `set_filter_enabled()` | Низкая |
| Передача FilterConfig из UI | При загрузке/изменении профиля вызывать `set_filter_config()` | Низкая |
| Автоинжект хука | Инжектировать `LootFilterHook` при старте сканера | Средняя |
| UI для Alt-режима | Кнопка для `set_show_all()` | Низкая |

---

## Тестирование

### Компиляция
```bash
cd src-tauri && cargo check
# Успешно, ошибок нет
```

### Рекомендуемые тесты
1. Запустить игру, запустить сканер
2. Вызвать `apply_item_filter(p_unit_data, false)` для предмета
3. Проверить, что `iEarLevel` записан (через отладчик или Cheat Engine)
4. Инжектировать хук и проверить, что предмет скрылся
5. **Новое:** Проверить автоматическую фильтрацию:
   - Установить `filter_config` через `set_filter_config()`
   - Включить фильтрацию через `set_filter_enabled(true)`
   - Уронить предмет и убедиться, что `iEarLevel` записывается автоматически

---

## Ссылки

- Спецификация: `docs/loot-filter-spec.md`
- Спецификация хука: `docs/loot-filter-hook-specification.md`
