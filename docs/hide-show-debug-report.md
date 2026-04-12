# Отчёт об отладке hide/show флагов лутфильтра

## Резюме проблемы

**Цель:** Скрывать/показывать тултипы предметов на земле через хук функции `D2Sigma.dll.text+CBCD0`.

**Статус:** Хук работает, запись в память проходит успешно (verified), но предметы НЕ скрываются.

---

## Архитектура решения

### Хук
- Точка хука: `D2Sigma.dll.text+CBCD0` (функция `LootFilter_ShouldShowItem`)
- Сигнатура: `bool __thiscall LootFilter_ShouldShowItem(Unit* pUnit)`
- ECX = pUnit, возврат AL = 0/1

### Логика хука (трамплин)
```asm
; Проверка глобальных флагов
cmp byte ptr [g_show_all], 0
je Return_Hide              ; если 0 - скрыть всё

cmp byte ptr [g_filter_en], 0
je Original_Code            ; если 0 - использовать оригинальный фильтр

; Проверка pUnit
test ecx, ecx
jz Original_Code

; Получить pUnitData = [pUnit + 0x14]
mov eax, [ecx+14h]
test eax, eax
jz Original_Code

; Прочитать iEarLevel = [pUnitData + 0x48]
movzx eax, byte ptr [eax+48h]

cmp al, 2                   ; HIDE?
je Return_Hide              ; -> return 1 (скрыть)

cmp al, 1                   ; SHOW?
je Return_Show              ; -> return 0 (показать)

jmp Original_Code           ; iEarLevel=0 -> оригинальный фильтр

Return_Hide:
    xor al, al              ; AL = 0 (HIDE)
    ret                     ; return 0 = скрыть

Return_Show:
    mov al, 1               ; AL = 1 (SHOW)
    ret                     ; return 1 = показать

Original_Code:
    ; восстановленные инструкции...
    jmp D2Sigma.dll.text+CBCD9
```

### Запись visibility
- Адрес: `pUnitData + 0x48` (поле iEarLevel в структуре ItemData)
- Значения: 0 = не обработан, 1 = show, 2 = hide

---

## Что работает

1. **Хук установлен корректно**
   - JMP на трамплин по адресу CBCD0
   - PATCH_SIZE = 9 байт (исправлено с 10)
   - Возврат на CBCD9 (валидная инструкция `test ebx,ebx`)

2. **Глобальные флаги работают**
   - g_show_all и g_filter_en выделяются и инициализируются
   - Синхронизация с AppState работает

3. **Запись в память проходит успешно**
   ```
   set_item_visibility: pUnitData=0x17E71C00, addr=0x17E71C48, value=2 (HIDE)
     -> Write verified OK: read back 2
   ```

4. **Фильтр правильно определяет действие**
   ```
   Filter action for 'Minor Healing Potion': show_item=false
   ```

---

## Что НЕ работает

**Основная проблема:** При вызове функции фильтрации игрой, по адресу `[pUnitData + 0x48]` находится **0**, а не **2**.

### Отладка через CE

1. Поставили breakpoint на `movzx eax, byte ptr [eax+48]`
2. При срабатывании:
   - EAX = pUnitData (например 0x17E71B00)
   - После инструкции: EAX = 0x00000000
3. В hex dump по адресу `pUnitData + 0x48` видны нули

### Противоречие

| Источник | Адрес | Значение |
|----------|-------|----------|
| Лог после записи | 0x17E71C48 | 2 (verified OK) |
| CE в момент чтения хуком | 0x17E71B48 | 0 |

**Заметка:** Адреса разные! 0x17E71C00 vs 0x17E71B00 - разница 0x100.

---

## Гипотезы

### 1. Разные экземпляры предмета
Возможно игра создаёт несколько структур ItemData для одного визуального предмета, или пересоздаёт их.

### 2. Timing проблема
- Игра вызывает функцию фильтрации СРАЗУ при создании предмета
- Наш scanner находит предмет через ~200ms
- К этому моменту результат уже закэширован

### 3. Игра обнуляет iEarLevel
Возможно поле iEarLevel используется игрой для чего-то другого и перезаписывается.

### 4. Неправильное смещение
Может смещение 0x48 неверное для текущей версии MedianXL.

### 5. Copy-on-write или защита памяти
Запись проходит в одну область, а чтение идёт из другой.

---

## Что пробовали

1. **Инвертировали логику возврата** (0=show, 1=hide) - не помогло
2. **Исправили PATCH_SIZE** с 10 на 9 байт - исправило краш, но не проблему
3. **Добавили верификацию записи** - запись проходит успешно
4. **Добавили apply_filter_to_all_items** - записываем каждый тик, но всё равно 0

---

## Следующие шаги для отладки

### 1. Проверить смещение iEarLevel
Нужно найти где в структуре ItemData реально хранятся данные которые можно модифицировать.

### 2. Поставить breakpoint на запись
В CE: "Find out what writes to this address" на адрес iEarLevel. Посмотреть кто его обнуляет.

### 3. Проверить pUnitData
При срабатывании хука посмотреть значение [pUnit + 0x14] - может там другой указатель.

### 4. Альтернативный подход
Вместо записи в iEarLevel, передать список скрытых unit_id через shared memory в хук.

### 5. Проверить версию MedianXL
Смещения могли измениться в новой версии. Нужно перепроверить структуру ItemData.

---

## Ключевые файлы

- `src-tauri/src/loot_filter_hook.rs` - генерация хука и трамплина
- `src-tauri/src/notifier.rs` - запись visibility и сканирование предметов
- `src-tauri/src/offsets.rs` - смещения (EAR_LEVEL = 0x48)
- `docs/loot-filter-hook-specification.md` - спецификация хука

---

## Код хука (текущее состояние)

```rust
// loot_filter_hook.rs

const PATCH_SIZE: usize = 9;  // Было 10, исправлено

// Логика возврата (инвертированная):
// Return_Hide: mov al, 1; ret  (iEarLevel == 2 -> return 1 -> hide)
// Return_Show: xor al, al; ret (iEarLevel == 1 -> return 0 -> show)
```

---

## Контакты и ресурсы

- Спецификация: `docs/loot-filter-hook-specification.md`
- UI интеграция: `docs/loot-filter-ui-integration-report.md`
- Анализ hide/show: `docs/hide-show-analysis.md`
