# Спецификация хука лутфильтра D2MXLUtils

## Резюме

**Цель достигнута:** Найдена функция `D2Sigma.dll.text+CBCD0` из исходного файла `D2LootFilter.cpp`, которая определяет видимость тултипов предметов на земле.

**Проверено:** Замена начала функции на `xor al,al; ret` скрывает ВСЕ тултипы предметов.

---

## Архитектура рендеринга тултипов MedianXL

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Цепочка вызовов при рендеринге                       │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  D2Sigma.dll.text+1369D0 (Loot.cpp)                                     │
│         │                                                               │
│         ▼                                                               │
│  Цикл по предметам на земле (136AA0 - 136DDA)                           │
│         │                                                               │
│         ├─► FC510: Проверка типа ([ecx] == 4 ITEM?)                     │
│         │                                                               │
│         ├─► CBCD0: ЛУТФИЛЬТР (D2LootFilter.cpp) ◄── ТОЧКА ХУКА          │
│         │         │                                                     │
│         │         ├─► Проверка настроек (город и т.д.)                  │
│         │         ├─► Итерация по правилам фильтра                      │
│         │         ├─► C97F0: Матчинг правила с предметом                │
│         │         └─► Return: TRUE=показать, FALSE=скрыть               │
│         │                                                               │
│         ├─► B7350: Получение имени предмета                             │
│         │                                                               │
│         └─► Отрисовка тултипа (D2Win)                                   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Целевая функция: D2Sigma.dll.text+CBCD0

### Сигнатура

```
bool __thiscall LootFilter_ShouldShowItem(Unit* pUnit)
```

- **Вход:** `ECX = pUnit` (указатель на структуру Unit предмета)
- **Выход:** `AL = 1` (показать) или `AL = 0` (скрыть)
- **ВАЖНО:** Проверено через CE - AL=0 скрывает, AL=1 показывает

### Оригинальный код (начало)

```asm
D2Sigma.dll.text+CBCD0 - 83 EC 08              - sub esp,08
D2Sigma.dll.text+CBCD3 - 53                    - push ebx
D2Sigma.dll.text+CBCD4 - 55                    - push ebp
D2Sigma.dll.text+CBCD5 - 8B D9                 - mov ebx,ecx
D2Sigma.dll.text+CBCD7 - 56                    - push esi
D2Sigma.dll.text+CBCD8 - 57                    - push edi
D2Sigma.dll.text+CBCD9 - 85 DB                 - test ebx,ebx
D2Sigma.dll.text+CBCDB - 75 1B                 - jne D2Sigma.dll.text+CBCF8
```

### Возможные пути возврата

| Адрес | Код | Значение | Описание |
|-------|-----|----------|----------|
| CBDDC | `mov al,01` | TRUE | Быстрый выход (показать) |
| CBDC6 | `mov al,[edi+52]` | default | Ни одно правило не сработало |
| CBDD1 | `mov al,[esi+09]` | rule_flag | Правило сработало, вернуть его флаг |

---

## Структуры данных

### Unit (pUnit)

| Смещение | Размер | Поле | Описание |
|----------|--------|------|----------|
| +0x00 | 4 | dwType | Тип юнита (4 = ITEM) |
| +0x04 | 4 | dwClass | ID класса предмета |
| +0x08 | 4 | ... | ... |
| +0x14 | 4 | pUnitData | Указатель на ItemData |

### ItemData (pUnitData)

| Смещение | Размер | Поле | Описание |
|----------|--------|------|----------|
| +0x00 | 4 | iQuality | Качество предмета |
| +0x18 | 4 | dwFlags | Флаги предмета |
| +0x48 | 1 | iEarLevel | **Поле для фильтрации** |

### Значения iEarLevel (наша конвенция)

| Значение | Описание |
|----------|----------|
| 0 | Предмет не обработан (новый) |
| 1 | Показать предмет (show) |
| 2 | Скрыть предмет (hide) |

---

## Спецификация хука

### Режимы работы

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Три режима фильтрации                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  1. HIDE (скрыть конкретный предмет)                                    │
│     - D2MXLUtils записывает iEarLevel = 2                               │
│     - Хук возвращает FALSE → тултип не отображается                     │
│                                                                         │
│  2. SHOW (показать конкретный предмет)                                  │
│     - D2MXLUtils записывает iEarLevel = 1                               │
│     - Хук возвращает TRUE → тултип отображается                         │
│     - (или пропускает проверку, отдавая решение оригинальному коду)     │
│                                                                         │
│  3. GLOBAL TOGGLE (глобальное вкл/выкл всех тултипов)                   │
│     - Глобальный флаг g_bShowAllLoot                                    │
│     - Если FALSE → хук возвращает FALSE для ВСЕХ предметов              │
│     - Если TRUE → нормальная работа фильтра                             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Псевдокод хука

```c
// Глобальные переменные (в выделенной памяти)
bool g_bFilterEnabled = true;   // Фильтр включён
bool g_bShowAllLoot = true;     // Показывать все тултипы (Alt режим)

// Хук функции CBCD0
bool __thiscall Hook_ShouldShowItem(Unit* pUnit) {
    // 1. Глобальное отключение всех тултипов
    if (!g_bShowAllLoot) {
        return false;  // Скрыть ВСЕ
    }

    // 2. Если фильтр отключён - показать всё
    if (!g_bFilterEnabled) {
        return true;  // Показать ВСЁ
    }

    // 3. Проверка pUnit
    if (pUnit == NULL) {
        return Original_ShouldShowItem(pUnit);
    }

    // 4. Получить pUnitData
    ItemData* pUnitData = (ItemData*)pUnit->pUnitData;  // [pUnit+0x14]
    if (pUnitData == NULL) {
        return Original_ShouldShowItem(pUnit);
    }

    // 5. Проверить наш флаг фильтрации
    uint8_t iEarLevel = pUnitData->iEarLevel;  // [pUnitData+0x48]

    switch (iEarLevel) {
        case 2:  // HIDE
            return false;  // Скрыть этот предмет

        case 1:  // SHOW
            return true;   // Показать этот предмет

        default: // 0 = не обработан
            // Вызвать оригинальную функцию для обработки
            // встроенным лутфильтром MedianXL
            return Original_ShouldShowItem(pUnit);
    }
}
```

### Ассемблерная реализация хука

```asm
; ============================================================
; Hook_ShouldShowItem
; Вставляется в начало D2Sigma.dll.text+CBCD0
; ECX = pUnit
; ============================================================

Hook_Start:
    ; --- Проверка глобального флага показа ---
    cmp byte ptr [g_bShowAllLoot], 0
    je Return_False              ; Если выключено - скрыть всё

    ; --- Проверка включения фильтра ---
    cmp byte ptr [g_bFilterEnabled], 0
    je Original_Code             ; Если фильтр выключен - пропустить

    ; --- Проверка pUnit ---
    test ecx, ecx
    jz Original_Code             ; pUnit == NULL - пропустить

    ; --- Получить pUnitData ---
    mov eax, [ecx+14h]           ; eax = pUnitData
    test eax, eax
    jz Original_Code             ; pUnitData == NULL - пропустить

    ; --- Проверить iEarLevel ---
    movzx eax, byte ptr [eax+48h] ; eax = iEarLevel

    cmp al, 2                    ; HIDE?
    je Return_False

    cmp al, 1                    ; SHOW?
    je Return_True

    ; --- iEarLevel == 0: вызвать оригинальный код ---
    jmp Original_Code

Return_False:
    xor al, al                   ; AL = 0 (скрыть)
    ret

Return_True:
    mov al, 1                    ; AL = 1 (показать)
    ret

Original_Code:
    ; Оригинальные инструкции (которые мы перезаписали)
    sub esp, 08
    push ebx
    push ebp
    mov ebx, ecx
    push esi
    push edi
    ; JMP на продолжение оригинальной функции
    jmp [Original_Continue]      ; CBCD9 или далее
```

---

## Интеграция с D2MXLUtils

### Компонент 1: Запись iEarLevel (notifier.rs)

```rust
/// Установить видимость предмета на земле
pub fn set_item_visibility(&self, p_unit_data: u32, visible: bool) -> Result<(), Error> {
    let value: u8 = if visible { 1 } else { 2 };
    self.process.write_memory(
        p_unit_data as usize + offsets::item_data::EAR_LEVEL,
        &[value]
    )
}

// В цикле обработки предметов:
fn process_ground_item(&mut self, item: &GroundItem, rule: &Rule) {
    if !rule.show_item {
        // Правило говорит "hide"
        self.set_item_visibility(item.p_unit_data, false)?;
    } else {
        // Правило говорит "show" или default
        self.set_item_visibility(item.p_unit_data, true)?;
    }
}
```

### Компонент 2: Инжект хука (injection.rs)

```rust
pub struct LootFilterHook {
    hook_address: usize,           // D2Sigma.dll + CBCD0
    original_bytes: [u8; N],       // Сохранённые оригинальные байты
    trampoline_address: usize,     // Адрес нашего кода
    g_show_all_loot: usize,        // Адрес глобального флага
    g_filter_enabled: usize,       // Адрес флага фильтра
}

impl LootFilterHook {
    pub fn inject(&mut self, process: &Process, d2sigma_base: usize) -> Result<()> {
        // 1. Вычислить адрес хука
        self.hook_address = d2sigma_base + 0xCBCD0;

        // 2. Выделить память для trampoline
        self.trampoline_address = process.allocate_memory(256)?;

        // 3. Выделить память для глобальных флагов
        self.g_show_all_loot = process.allocate_memory(1)?;
        self.g_filter_enabled = process.allocate_memory(1)?;

        // 4. Инициализировать флаги
        process.write_memory(self.g_show_all_loot, &[1u8])?;  // TRUE
        process.write_memory(self.g_filter_enabled, &[1u8])?; // TRUE

        // 5. Записать код хука в trampoline
        let hook_code = self.generate_hook_code();
        process.write_memory(self.trampoline_address, &hook_code)?;

        // 6. Сохранить оригинальные байты
        process.read_memory(self.hook_address, &mut self.original_bytes)?;

        // 7. Записать JMP на наш хук
        let jmp_code = self.generate_jmp(self.hook_address, self.trampoline_address);
        process.write_memory(self.hook_address, &jmp_code)?;

        Ok(())
    }

    pub fn set_global_show(&self, process: &Process, show: bool) -> Result<()> {
        process.write_memory(self.g_show_all_loot, &[show as u8])
    }

    pub fn set_filter_enabled(&self, process: &Process, enabled: bool) -> Result<()> {
        process.write_memory(self.g_filter_enabled, &[enabled as u8])
    }

    pub fn eject(&self, process: &Process) -> Result<()> {
        // Восстановить оригинальные байты
        process.write_memory(self.hook_address, &self.original_bytes)?;
        // Освободить память
        process.free_memory(self.trampoline_address)?;
        Ok(())
    }
}
```

### Компонент 3: Хоткеи (frontend)

| Хоткей | Действие | Функция |
|--------|----------|---------|
| Alt (hold) | Показать все тултипы | `g_bShowAllLoot = false` (инверсия Alt) |
| F7 | Toggle фильтра | `g_bFilterEnabled = !g_bFilterEnabled` |

---

## Адреса и смещения

### D2Sigma.dll

| Смещение | Описание |
|----------|----------|
| +0xCBCD0 | Функция лутфильтра (точка хука) |
| +0x1369D0 | Главная функция рендеринга тултипов (Loot.cpp) |
| +0x6C5388 | Глобальный указатель на состояние |

### Структуры

| Структура | Смещение | Поле |
|-----------|----------|------|
| Unit | +0x14 | pUnitData |
| ItemData | +0x48 | iEarLevel |

---

## Что есть и чего не хватает

### Есть (готово к реализации)

| Компонент | Статус | Описание |
|-----------|--------|----------|
| Точка хука | ✅ | D2Sigma.dll+CBCD0 |
| Сигнатура функции | ✅ | ECX=pUnit, AL=result |
| Смещение iEarLevel | ✅ | pUnitData+0x48 |
| Смещение pUnitData | ✅ | pUnit+0x14 |
| Парсинг DSL (hide/show) | ✅ | Уже работает в rules/dsl.rs |
| Сканирование предметов | ✅ | Уже работает в notifier.rs |

### Нужно реализовать

| Компонент | Сложность | Описание |
|-----------|-----------|----------|
| `set_item_visibility()` | Легко | Запись в iEarLevel |
| Генерация машинного кода хука | Средне | Ассемблер → байты |
| `LootFilterHook::inject()` | Средне | Инжект и управление хуком |
| Интеграция с UI | Легко | Хоткеи, настройки |

---

## План реализации

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Этапы реализации                                │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Этап 1: Базовый функционал                                             │
│  ├─ Добавить offset EAR_LEVEL в offsets.rs                              │
│  ├─ Реализовать set_item_visibility() в notifier.rs                     │
│  └─ Вызывать при обработке правил с hide/show                           │
│                                                                         │
│  Этап 2: Хук (без хука iEarLevel игнорируется!)                         │
│  ├─ Создать модуль loot_filter_hook.rs                                  │
│  ├─ Реализовать генерацию машинного кода                                │
│  ├─ Реализовать inject/eject                                            │
│  └─ Интегрировать с жизненным циклом приложения                         │
│                                                                         │
│  Этап 3: Глобальные флаги                                               │
│  ├─ Добавить g_bShowAllLoot для Alt-режима                              │
│  ├─ Добавить g_bFilterEnabled для toggle                                │
│  └─ Реализовать хоткеи                                                  │
│                                                                         │
│  Этап 4: UI интеграция                                              │
│  └─ Настройки поведения                                                 │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Риски и ограничения

| Риск | Вероятность | Митигация |
|------|-------------|-----------|
| Обновление MedianXL сломает адреса | Средняя | Сигнатурный поиск функции |
| Конфликт с встроенным фильтром MXL | Низкая | Наш хук проверяет ДО встроенного |
| Античит (если появится) | Низкая | Использовать те же методы что D2Stats |
| Краш при неправильном хуке | Средняя | Тщательное тестирование, graceful eject |

---

## Заключение

Реверс-инжиниринг завершён успешно. Найдена идеальная точка для хука - функция `D2Sigma.dll.text+CBCD0` из `D2LootFilter.cpp`.

Функция:
- Получает `pUnit` в регистре `ECX`
- Возвращает `bool` в `AL` (TRUE=показать, FALSE=скрыть)
- Вызывается для каждого предмета перед отрисовкой тултипа

Все необходимые данные для реализации собраны. Можно приступать к написанию кода хука.
