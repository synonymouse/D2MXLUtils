# Таблицы UniqueItems.txt / SetItems.txt / Sets.txt в памяти D2

Справочник по memory layout трёх таблиц D2Common, которые нужны для
расширения автокомплита (см. `docs/autocomplete.md` → раздел "Как
расширять → Добавить уникалки и сеты").

Статус: **верифицировано на живой памяти MedianXL 1.13c** — одноразовый
debug-дамп (`DropScanner::verify_item_tables_debug`) пройден и удалён.
Константы зафиксированы в `src-tauri/src/offsets.rs` (`d2common::SGPT_DATA_TABLES`,
модули `data_tables`, `unique_items_txt`, `set_items_txt`).

---

## TL;DR

Всё адресуется через единую структуру `D2DataTablesStrc` (в исходниках
D2MOO — `sgptDataTables`). Указатель на неё уже используется в проекте:

```
sgptDataTables = *(u32*)(D2Common + 0x99E1C)
```

Это тот же `$g_pD2sgpt` из легаси `D2Stats.au3` (строка 259).

Внутри `D2DataTablesStrc` нас интересуют шесть полей:

| Offset | Тип | Что это |
|---|---|---|
| `+0xC0C` | `D2SetsTxt*` | `pSetsTxt` — бонусы за полный сет |
| `+0xC10` | `int` | `nSetsTxtRecordCount` |
| `+0xC18` | `D2SetItemsTxt*` | `pSetItemsTxt` — отдельные предметы сета |
| `+0xC1C` | `int` | `nSetItemsTxtRecordCount` |
| `+0xC24` | `D2UniqueItemsTxt*` | `pUniqueItemsTxt` — уникалки |
| `+0xC28` | `int` | `nUniqueItemsTxtRecordCount` |

Оффсет `+0xC24` независимо подтверждён `D2Stats.au3:928` (`$pUniqueItemsTxt
= _MemoryRead($g_pD2sgpt + 0xC24, ...)`), что подтверждает согласованность
layout-а D2MOO (таргет 1.10) с 1.13c-кодом MedianXL.

---

## Запись `D2UniqueItemsTxt`

Размер: `0x14C` байт. Подтверждён AutoIt-кодом:
`$pUniqueItemsTxt + ($iFileIndex * 0x14c)` (`D2Stats.au3:976`).

| Offset | Размер | Поле | Назначение |
|---|---|---|---|
| `0x00` | u16 | `wId` | record ID |
| `0x02` | char[32] | `szName` | внутреннее имя (ключ из `.txt`) |
| **`0x22`** | **u16** | **`wTblIndex`** | **NAME_ID для `GetStringById`** |
| `0x24` | u16 | `wVersion` | 0 = classic, 100 = expansion |
| `0x26` | u16 | — | padding |
| `0x28` | u32 | `dwBaseItemCode` | FourCC базового предмета |
| `0x2C` | u32 | `dwUniqueItemFlags` | |
| `0x30` | u16 | `wRarity` | drop-rate вес |
| `0x32` | u16 | — | padding |
| `0x34` | u16 | `wLvl` | ← уже в `offsets.rs::unique_items_txt::LEVEL` |
| `0x36` | u16 | `wLvlReq` | required level |
| `0x38` | i8 | `nChrTransform` | палитра персонажа |
| `0x39` | i8 | `nInvTransform` | палитра инвентаря |
| `0x3A` | char[32] | `szFlippyFile` | |
| `0x5A` | char[32] | `szInvFile` | |
| `0x7C` | u32 | `dwCostMult` | |
| `0x80` | u32 | `dwCostAdd` | |
| `0x84` | u16 | `wDropSound` | |
| `0x86` | u16 | `wUseSound` | |
| `0x88` | u8 | `nDropSfxFrame` | |
| `0x8C` | 12×0x10 | `pProperties[12]` | аффиксы (конец записи: 0x8C + 0xC0 = 0x14C) |

**Подтверждение что `wTblIndex` — это именно string-table ID:**
`D2MOO/source/D2Common/src/DataTbls/ItemsTbls.cpp`, функция загрузки
(`DATATBLS_LoadUniqueItemsTxt`):

```cpp
sgptDataTables->pUniqueItemsTxt[i].wTblIndex = D2LANG_GetTblIndex(
    sgptDataTables->pUniqueItemsTxt[i].szName, &pUnicode);
if (sgptDataTables->pUniqueItemsTxt[i].wTblIndex == 0) {
    sgptDataTables->pUniqueItemsTxt[i].wTblIndex = 5383;
}
```

Игра сама при загрузке резолвит `szName` → `wTblIndex` через локализацию.
Если запись в string-table не найдена — записывается sentinel `5383`.
В рантайме мы просто передаём `wTblIndex` в `D2Injector::get_string`
(тот же путь, что для `items.txt.NAME_ID`).

---

## Запись `D2SetItemsTxt` (отдельные предметы сета — "Sigon's Gage")

Размер: `0x1B8` байт (`0x118 + 10 × sizeof(D2PropertyStrc)`, где
`sizeof(D2PropertyStrc) = 0x10`).

| Offset | Размер | Поле | Назначение |
|---|---|---|---|
| `0x00` | u16 | `wSetItemId` | |
| `0x02` | char[32] | `szName` | внутреннее имя |
| `0x22` | u16 | `wVersion` | |
| **`0x24`** | **u16** | **`wStringId`** | **NAME_ID для `GetStringById`** |
| `0x26` | u16 | — | padding |
| `0x28` | u32 | `szItemCode` | FourCC базы |
| `0x2C` | i16 | `nSetId` | индекс в `Sets.txt` |
| `0x2E` | i16 | `nSetItems` | |
| `0x30` | u16 | `wLvl` | |
| `0x32` | u16 | `wLvlReq` | |
| `0x34` | u32 | `dwRarity` | |
| `0x38` | u32 | `dwCostMult` | |
| `0x3C` | u32 | `dwCostAdd` | |
| `0x40` | i8 | `nChrTransform` | |
| `0x41` | i8 | `nInvTransform` | |
| `0x42` | char[32] | `szFlippyFile` | |
| `0x62` | char[32] | `szInvFile` | |
| `0x82` | u16 | `wDropSound` | |
| `0x84` | u16 | `wUseSound` | |
| `0x86` | u8 | `nDropSfxFrame` | |
| `0x87` | u8 | `nAddFunc` | |
| `0x88` | 9×0x10 | `pProperties[9]` | full-set аффиксы (0x88 + 0x90 = 0x118) |
| `0x118` | 10×0x10 | `pPartialBoni[10]` | partial-set бонусы (конец: 0x1B8) |

У сетов имя-идентификатор именуется `wStringId` напрямую — никаких
догадок не нужно, это буквально string-table index.

---

## Запись `D2SetsTxt` (полноценные сет-бонусы — "Sigon's Complete Steel")

Размер: `0x128` байт. Для автокомплита _базовых предметов_ не критично,
но понадобится если делать подсказки имён сет-групп.

| Offset | Размер | Поле |
|---|---|---|
| `0x00` | u16 | `wSetId` |
| `0x02` | u16 | `wStringId` — NAME_ID |
| `0x04` | u16 | `wVersion` |
| `0x08` | u32 | `unk0x08` |
| `0x0C` | i32 | `nSetItems` |
| `0x10` | 2×0x10 | `pBoni2[2]` |
| `0x30` | 2×0x10 | `pBoni3[2]` |
| `0x50` | 2×0x10 | `pBoni4[2]` |
| `0x70` | 2×0x10 | `pBoni5[2]` |
| `0x90` | 8×0x10 | `pFBoni[8]` |
| `0x110` | 6×4 | `pSetItem[6]` — указатели на `D2SetItemsTxt` |

---

## Оговорка про версию D2

D2MOO — проект реверса для D2 1.10. MedianXL работает на D2 1.13c.
Различается только **абсолютный адрес `sgptDataTables` в D2Common.dll**:
D2MOO указывает `D2Common.dll + 0x96A20` (relative:
`0x6FDD6A20 − 0x6FD40000`), наш 1.13c-путь — `D2Common + 0x99E1C`.

**Layout полей _внутри_ структур одинаков** для 1.10 и 1.13c — это
подтверждено live-дампом MedianXL (см. ниже). Движок D2 хардкодит
оффсеты полей в ассемблере, так что MedianXL мог расширять только
_количество_ записей, но не их структуру.

---

## Результаты верификации

Дамп в `d2mxlutils.log` при атташе к MedianXL 1.13c:

```
[item-tables verify] sgptDataTables = 0x6FDEFED8
[item-tables verify] UniqueItems: count=1822, ptr=0x28EEF028
[item-tables verify] UniqueItems[0]: name_id=2697 -> "Amulet of the Viper"
[item-tables verify] UniqueItems[1]: name_id=2698 -> "Staff of Kings"
[item-tables verify] UniqueItems[2]: name_id=2699 -> "Horadric Staff"
[item-tables verify] UniqueItems[3]: name_id=2700 -> "Hell Forge Hammer"
[item-tables verify] UniqueItems[4]: name_id=1062 -> "Khalim's Flail"
...
[item-tables verify] UniqueItems[1821]: name_id=28560 -> "Auriel's Satchel"
[item-tables verify] SetItems: count=330, ptr=0x32E88D58
[item-tables verify] SetItems[0]: name_id=10122 -> "Civerb's Ward"
[item-tables verify] SetItems[1]: name_id=10123 -> "Civerb's Icon"
...
[item-tables verify] SetItems[329]: name_id=26376 -> "Branches"
```

Подтвердилось:

- **Pointer-механика D2SGPT работает** для обеих таблиц — `+0xC24`/`+0xC28`
  для уников, `+0xC18`/`+0xC1C` для сет-предметов.
- **NAME_ID-оффсеты корректны:** `wTblIndex @ 0x22` в уникалках и
  `wStringId @ 0x24` в сетах резолвятся через `D2Lang::GetStringById` в
  осмысленные локализованные имена.
- **Record sizes корректны:** `0x14C` (уники) и `0x1B8` (сеты) — итерация
  по 1822 и 330 записям без смещения/мусора.
- **MedianXL сильно расширяет объём** относительно ванильной 1.13c
  (1822 уникалки, 330 сет-предметов).
- **Для автокомплита:** имена "чистые" — без тир-суффиксов вроде
  `(Sacred)`/`(Angelic)`/`(Mastercrafted)`, которые встречаются в
  `items.txt` у base-types. Достаточно пропустить через существующий
  `strip_color_codes` (для `ÿc`-escape-ов) и брать как есть.

Debug-метод `DropScanner::verify_item_tables_debug` после верификации
удалён — константы живут в `offsets.rs` и доступны любому модулю.

---

## Сорсы

- [ThePhrozenKeep/D2MOO — `source/D2Common/include/D2DataTbls.h`](https://github.com/ThePhrozenKeep/D2MOO/blob/master/source/D2Common/include/D2DataTbls.h) — полный `D2DataTablesStrc` с оффсетами всех полей.
- [ThePhrozenKeep/D2MOO — `source/D2Common/include/DataTbls/ItemsTbls.h`](https://github.com/ThePhrozenKeep/D2MOO/blob/master/source/D2Common/include/DataTbls/ItemsTbls.h) — структуры `D2UniqueItemsTxt`, `D2SetItemsTxt`, `D2SetsTxt`.
- [ThePhrozenKeep/D2MOO — `source/D2Common/src/DataTbls/ItemsTbls.cpp`](https://github.com/ThePhrozenKeep/D2MOO/blob/master/source/D2Common/src/DataTbls/ItemsTbls.cpp) — подтверждение что `wTblIndex` — это string-table index, выставляемый через `D2LANG_GetTblIndex(szName)`.
- Локально: `D2Stats.au3:259, 928, 976-977` — 1.13c-специфичные значения `sgptDataTables = [D2Common + 0x99E1C]`, `pUniqueItemsTxt = [sgpt + 0xC24]`, record size `0x14C`, `wLvl` на оффсете 52.
