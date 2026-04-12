# Анализ работы флагов hide/show в лутфильтре

## Резюме

Флаги `hide`/`show` в текущей версии D2MXLUtils **не работают**, потому что:

1. **Отсутствует DropFilter.dll** - внешняя DLL, необходимая для патчинга рендеринга предметов
2. **Отсутствует функция DisplayItemOnGround** - код записи в память структуры предмета
3. **Отсутствует код инжекта DropFilter.dll** - функции InjectDropFilter/EjectDropFilter

---

## Как работает hide/show в D2StatsOldVersion.au3

### Архитектура (два взаимосвязанных механизма)

**Важно:** DisplayItemOnGround и DropFilter.dll работают **вместе**, а не независимо!

```
┌─────────────────────────────────────────────────────────────────┐
│                        Цикл работы                              │
├─────────────────────────────────────────────────────────────────┤
│  1. D2Stats сканирует предметы на земле                         │
│  2. Применяет правила фильтра (hide/show)                       │
│  3. DisplayItemOnGround записывает значение в iEarLevel (0x48)  │
│  4. DropFilter.dll при рендеринге ЧИТАЕТ iEarLevel              │
│  5. Если iEarLevel == 2 → предмет НЕ рисуется                   │
└─────────────────────────────────────────────────────────────────┘
```

#### Механизм 1: Запись маркера (DisplayItemOnGround)

```autoit
; D2StatsOldVerison.au3:1004-1006
func DisplayItemOnGround($pUnitData, $iShow)
    _MemoryWrite($pUnitData + 0x48, $g_ahD2Handle, $iShow ? 1 : 2, "byte")
endfunc
```

**Что такое поле 0x48 (iEarLevel)?**

Это поле `iEarLevel` в структуре ItemData:
```autoit
; D2StatsOldVerison.au3:873
local $tItemData = DllStructCreate("dword iQuality;dword pad1[5];dword iFlags;dword pad2[3];dword dwFileIndex;dword pad2[7];byte iEarLevel;")
```

**Значения iEarLevel:**
| Значение | Смысл |
|----------|-------|
| `0` | Предмет ещё не обработан (новый) |
| `1` | Показать предмет (show) |
| `2` | Скрыть предмет (hide) |

**Двойное использование iEarLevel:**
1. **Как маркер "уже видели"** - если `iEarLevel != 0`, предмет пропускается при сканировании (строка 911)
2. **Как флаг видимости** - DropFilter.dll читает это значение при рендеринге

```autoit
; D2StatsOldVerison.au3:911-913
; Проверка: если iEarLevel != 0, значит предмет уже обработан
if (not $g_bNotifierChanged and $iEarLevel <> 0) then continueloop
; По умолчанию показываем предмет
DisplayItemOnGround($pUnitData, true)
```

**Критически важно:** Сама по себе запись в `iEarLevel` **НЕ скрывает предмет**! Игра изначально не использует это поле для решения о рендеринге. Нужен хук (DropFilter.dll), который перехватывает рендеринг и читает это значение.

#### Механизм 2: DropFilter.dll (хук рендеринга)

DropFilter.dll - это внешняя DLL, которая хукает код рендеринга предметов в D2Client.dll:

```autoit
; D2StatsOldVerison.au3:2335-2340
#cs
D2Client.dll+5907E - 83 3E 04              - cmp dword ptr [esi],04 { 4 }
D2Client.dll+59081 - 0F85
-->
D2Client.dll+5907E - E9 *           - jmp DropFilter.dll+15D0 { PATCH_DropFilter }
#ce
```

**Как это работает:**
1. `InjectDropFilter()` загружает DropFilter.dll в процесс игры через `LoadLibraryA`
2. Находит экспортированную функцию `_PATCH_DropFilter@0`
3. Патчит код D2Client.dll+0x5907E, заменяя оригинальную проверку на JMP в DropFilter.dll
4. DropFilter.dll перехватывает рендеринг предметов и фильтрует их

**Код инжекта:**
```autoit
; D2StatsOldVerison.au3:2342-2373
func InjectDropFilter()
    local $sPath = FileGetLongName("DropFilter.dll", $FN_RELATIVEPATH)
    if (not FileExists($sPath)) then return _Debug("...")
    
    ; Загружаем DLL в процесс игры
    local $pLoadLibraryA = _WinAPI_GetProcAddress(_WinAPI_GetModuleHandle("kernel32.dll"), "LoadLibraryA")
    local $iRet = RemoteThread($pLoadLibraryA, $g_pD2InjectString)
    
    ; Патчим D2Client для перехода в DropFilter
    local $bInjected = 233 <> _MemoryRead($g_hD2Client + 0x5907E, $g_ahD2Handle, "byte")
    if ($iRet and $bInjected) then
        local $hDropFilter = _WinAPI_LoadLibrary("DropFilter.dll")
        local $pEntryAddress = _WinAPI_GetProcAddress($hDropFilter, "_PATCH_DropFilter@0")
        local $pJumpAddress = $pEntryAddress - 0x5 - ($g_hD2Client + 0x5907E)
        _MemoryWrite($g_hD2Client + 0x5907E, $g_ahD2Handle, "0xE9" & SwapEndian($pJumpAddress), "byte[5]")
    endif
endfunc
```

### Логика фильтрации предметов

```autoit
; D2StatsOldVerison.au3:1007-1064 (OnGroundFilterItems)
func OnGroundFilterItems(byref $aOnGroundDisplayPool, byref $bDelayedHideItem)
    ; ...
    for $i = 0 to UBound($aOnGroundDisplayPool) - 1
        if ($oFlags.item('$bShowItem')) then
            $bShowOnGround = True
        elseif ($oFlags.item('$bHideItem')) then
            $bHideCompletely = True
        endif
    next
    
    select
        case $bShowOnGround
            DisplayItemOnGround($pUnitData, true)
        case $bHideCompletely
            if ($bWithStatGroups) then
                $bDelayedHideItem = True  ; Отложить скрытие до проверки статов
            else
                DisplayItemOnGround($pUnitData, false)
            endif
    endselect
endfunc
```

---

## Что есть в D2MXLUtils (наша версия)

### Парсинг DSL (работает)

```rust
// src-tauri/src/rules/dsl.rs:164-170
NotifyColor::Hide => {
    rule.show_item = false;
    rule.color = Some("hide".to_string());
}
NotifyColor::Show => {
    rule.show_item = true;
    rule.color = Some("show".to_string());
}
```

### Структура Rule (есть поле show_item)

```rust
// src-tauri/src/rules/mod.rs:385-387
/// Show item notification (DSL: show/hide via color)
#[serde(default = "default_true")]
pub show_item: bool,
```

### Чего НЕТ

| Компонент | Статус | Файл в D2StatsOld |
|-----------|--------|-------------------|
| `DisplayItemOnGround()` | **ОТСУТСТВУЕТ** | строка 1004 |
| `InjectDropFilter()` | **ОТСУТСТВУЕТ** | строка 2342 |
| `EjectDropFilter()` | **ОТСУТСТВУЕТ** | строка 2377 |
| `GetDropFilterHandle()` | **ОТСУТСТВУЕТ** | строка 2326 |
| `DropFilter.dll` | **ОТСУТСТВУЕТ** | внешний файл |
| Хоткей для toggle фильтра | **ОТСУТСТВУЕТ** | строка 286 |
| Запись в pUnitData+0x48 | **ОТСУТСТВУЕТ** | - |

### Notifier (только сканирование)

```rust
// src-tauri/src/notifier.rs
// Только сканирует предметы и отправляет события на фронтенд
// НЕТ кода для скрытия/показа предметов в игре
pub fn tick(&mut self) -> Vec<ItemDropEvent> {
    // ... сканирование предметов ...
    // НЕТ: DisplayItemOnGround или аналога
}
```

---

## Почему не работает в D2StatsOld

Согласно твоему тестированию, в старой версии D2Stats флаги hide/show тоже не работают. Причина:

1. **Изменились адреса в D2Client.dll** - версия MedianXL обновилась
2. Адрес патча `D2Client.dll+0x5907E` больше не соответствует нужному коду
3. DropFilter.dll ищет экспорт `_PATCH_DropFilter@0` по старому адресу

---

## Что нужно сделать для восстановления работы

### Необходимые компоненты

Для работы hide/show нужны **оба компонента**:

| Компонент | Функция | Сложность |
|-----------|---------|-----------|
| DisplayItemOnGround (запись в iEarLevel) | Помечает предметы для скрытия | Легко |
| Хук рендеринга | Читает iEarLevel и блокирует отрисовку | Требует реверс-инжиниринга |

**Без хука рендеринга DisplayItemOnGround бесполезен** - игра просто проигнорирует записанное значение.

---

### Этап 1: Реверс-инжиниринг D2Client.dll

Прежде чем писать код, нужно найти место в D2Client.dll, где происходит отрисовка предметов на земле.

#### Что искать

В старой версии D2Stats патчился адрес `D2Client.dll+0x5907E`:
```asm
D2Client.dll+5907E - 83 3E 04    - cmp dword ptr [esi], 04  ; проверка: unitType == ITEM?
D2Client.dll+59081 - 0F85 ...    - jne ...
```

Нужно найти **аналогичный код в текущей версии D2Client.dll**:
- Проверка `cmp dword ptr [reg], 04` где 4 = ITEM
- Это место, где игра решает рендерить ли unit

#### Инструменты для реверса

| Инструмент | Применение |
|------------|------------|
| **Cheat Engine** | Поиск памяти, просмотр дизассемблера |
| **x64dbg / x32dbg** | Отладка, трассировка, установка breakpoints |
| **IDA Free / Ghidra** | Статический анализ D2Client.dll |

#### Методика поиска

1. **Найти функцию рендеринга предметов:**
   - Поставить breakpoint на чтение `pUnitData` при отрисовке предмета
   - Или искать строку `cmp dword ptr [*], 04` в дизассемблере

2. **Проверить что это нужное место:**
   - NOP'нуть инструкцию и убедиться что предметы перестают рисоваться
   - Или изменить условие jump и проверить эффект

3. **Документировать найденный адрес:**
   - Записать смещение относительно базы D2Client.dll
   - Записать оригинальные байты для восстановления

---

### Этап 2: Реализация DisplayItemOnGround

После нахождения адреса для хука, реализовать запись маркера:

**offsets.rs:**
```rust
pub mod item_data {
    // ...существующие офсеты...
    pub const EAR_LEVEL: usize = 0x48;  // 0=new, 1=show, 2=hide
}
```

**notifier.rs или injection.rs:**
```rust
pub fn set_item_visibility(&self, p_unit_data: u32, visible: bool) -> Result<(), String> {
    let value: u8 = if visible { 1 } else { 2 };
    self.process.write_memory(
        p_unit_data as usize + item_data::EAR_LEVEL,
        &[value]
    )
}
```

---

### Этап 3: Реализация хука рендеринга на Rust

После успешного реверса - написать inline hook:

```rust
// Псевдокод - конкретная реализация зависит от найденного адреса
pub fn inject_render_hook(&self, d2_client: usize) -> Result<(), String> {
    let patch_address = d2_client + НАЙДЕННЫЙ_OFFSET;

    // Хук должен:
    // 1. Прочитать pUnitData из регистра (зависит от контекста)
    // 2. Проверить [pUnitData + 0x48]
    // 3. Если == 2, пропустить отрисовку (jmp мимо)
    // 4. Иначе выполнить оригинальный код

    let hook_code: Vec<u8> = vec![
        // ... ассемблерный код хука ...
    ];

    self.process.write_buffer(patch_address, &hook_code)?;
    Ok(())
}
```

#### Варианты реализации хука

| Вариант | Описание | Плюсы | Минусы |
|---------|----------|-------|--------|
| **Inline patch** | Заменить инструкции на месте | Просто | Ограничен размером |
| **Trampoline** | JMP в наш код → проверка → JMP обратно | Гибко | Нужно выделить память |
| **VTable hook** | Подменить указатель на функцию | Чисто | Нужно найти vtable |

---

### Этап 4: Интеграция с фильтром

После реализации хука - связать с правилами фильтра:

```rust
// В notifier.rs, после матчинга правила
fn process_item(&mut self, item: &ScannedItem, rule: &Rule) {
    if !rule.show_item {
        // Пометить предмет как скрытый
        self.set_item_visibility(item.p_unit_data, false)?;
    } else {
        self.set_item_visibility(item.p_unit_data, true)?;
    }

    // ... остальная логика уведомлений ...
}
```

---

## Ключевые адреса и смещения

### Из D2StatsOldVersion.au3

| Смещение | Описание |
|----------|----------|
| `pUnitData + 0x48` | Visibility byte (1=show, 2=hide) |
| `D2Client.dll + 0x5907E` | Адрес для патча (устарел) |
| `DropFilter.dll + 0x15D0` | Entry point хука (_PATCH_DropFilter) |

### Проверка в Cheat Engine

Для поиска нового адреса патча:
1. Найти код `cmp dword ptr [esi], 04` в D2Client.dll
2. Это проверка типа юнита (4 = ITEM)
3. Патч должен перехватывать этот код

---

## Заключение

### Почему hide/show не работают

| Версия | Проблема |
|--------|----------|
| **D2MXLUtils (наша)** | Отсутствуют ОБА компонента: DisplayItemOnGround и хук рендеринга |
| **D2StatsOld** | Адрес хука `D2Client.dll+0x5907E` устарел для текущей версии MedianXL |

### План реализации

```
┌─────────────────────────────────────────────────────────────────┐
│  Этап 1: Реверс-инжиниринг D2Client.dll                         │
│          ↓                                                      │
│  Этап 2: Реализация DisplayItemOnGround (запись в iEarLevel)    │
│          ↓                                                      │
│  Этап 3: Реализация хука рендеринга на Rust                     │
│          ↓                                                      │
│  Этап 4: Интеграция с правилами фильтра                         │
└─────────────────────────────────────────────────────────────────┘
```

### Первый шаг

Начать с реверс-инжиниринга текущей версии D2Client.dll:
- Найти функцию рендеринга предметов
- Найти место проверки типа юнита (аналог `cmp dword ptr [esi], 04`)
- Задокументировать новый адрес для патча
