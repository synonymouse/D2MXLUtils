# Item Scanning Paths: DropScanner vs Marker BFS

Этот документ объясняет две разные системы, которые сейчас находят предметы на земле:

- `DropScanner::tick_items()` - основной item scanner, который создает notifications, loot history entries, hook-mask decisions и cache решений фильтра.
- `MarkerScanner::tick()` / `manager::bfs_item_positions()` внутри `map_markers` - marker scanner, который ищет позиции предметов через BFS по графу комнат, публикует raw BFS candidates для `DropScanner` и ставит automap markers только для уже известных filter decisions.

Цель документа - зафиксировать, чем эти системы похожи, где они расходятся, и почему возможен класс багов, где off-screen item виден одному пути, но не другому.

## Короткая версия

Оба пути стартуют от текущего игрока и его текущей runtime-комнаты (`Room1`), но дальше используют разные структуры игры.

```text
Common start:

PLAYER_UNIT
  -> player UnitAny
    -> player path
      -> current Room1
```

После этого пути расходятся:

```text
DropScanner:
current Room1
  -> готовый список nearby Room1 pointers (`pPaths`/`ppRoomsNear`, `iPaths`)
    -> unit chain от каждой nearby room entry
      -> full item enrichment + filter decision + notification

Marker BFS:
current Room1
  -> Room1 neighbours
    -> neighbours of neighbours, до depth limit
      -> per-room unit chain
        -> coordinates + unit_id only
        -> shared BFS candidate snapshot for DropScanner
        -> marker only if DropScanner already cached filter decision
```

Главное отличие: `DropScanner` принимает все смысловые решения о предмете, а marker BFS только находит координаты, `p_unit` и `unit_id`. Marker BFS не знает, что предмет важный, пока `DropScanner` не обработал этот `unit_id`, но теперь он передает BFS-only candidates обратно в item scanner.

## Термины

### `player path`

`player path` - это не маршрут, где игрок прошел. Это runtime-структура позиции/движения игрока в памяти D2.

Упрощенно она содержит или ведет к:

- текущей комнате игрока (`Room1`);
- текущим координатам/subtile position;
- данным движения;
- nearby структурам комнат, которые движок считает актуальными рядом с игроком.

В коде это начинается так:

```text
p_player = *(D2Client + PLAYER_UNIT)
p_path   = *(p_player + 0x2C)
p_room1  = *(p_path + 0x1C)
```

См. `src-tauri/src/notifier/discovery.rs` (`DropScanner::tick_items`), `src-tauri/src/map_markers/manager/mod.rs` (`bfs_item_positions`), `src-tauri/src/offsets.rs:127-134`.

### `Room1`

`Room1` - runtime-комната, которую игра держит в памяти. Это не обязательно квадрат матрицы. Практичнее думать о комнатах как о графе:

```text
        room C
          |
room A - room B - room D
          |
        room E
```

У комнаты есть ссылки на соседние комнаты (`ppRoomsNear`) и количество таких соседей (`dwRoomsNear`). Marker BFS ходит по этим ссылкам.

### BFS

BFS = breadth-first search, обход графа в ширину.

Для marker scanner это означает:

1. Начать с текущей `Room1` игрока.
2. Прочитать предметы в этой комнате.
3. Перейти к соседним комнатам.
4. Прочитать предметы там.
5. Перейти к соседям соседей.
6. Остановиться на depth limit или когда новых комнат нет.

Текущий marker BFS вызывает `manager::bfs_item_positions(&ctx, 10)` внутри feature `map_markers`, то есть лимит глубины равен 10 переходам по graph links, а не 10 экранным клеткам.

## `DropScanner::tick_items()` подробно

`DropScanner::tick_items()` - основной scanner loot-пайплайна. Он отвечает за то, что пользователь воспринимает как drop notification.

Файл: `src-tauri/src/notifier/discovery.rs`.

Подготовка payload (`to_event`) и on-demand stats (`enrich_event_stats`) —
`src-tauri/src/notifier/event.rs`; live matching caches, их disk persistence и
`items_dictionary_snapshot` — `src-tauri/src/notifier/catalog.rs`. Все методы
остаются на `DropScanner`; его state/lifecycle, DTOs и stable exports — в `mod.rs`.
`discovery.rs` сохраняет полный порядок tick: два nearby-room прохода,
проверенные BFS candidates, cleanup/pruning и pickup resolution.
`processing.rs` содержит `scan_unit` и `process_scanned_item`, `pickup.rs` —
inventory walk, `visibility/controls.rs` — no-pickup/always-show controls и mask retries;
`visibility/hotkey.rs` — полный reveal-hidden watcher с состоянием и командой настройки.
`visibility/hook/mod.rs` сохраняет native hook, trampoline/layout, reattach и mask operations;
`visibility/tracker/mod.rs` — bit accounting и retries. Их tests находятся рядом;
visibility container остаётся ungated, native gates и unsupported hook stubs сохранены.
Marker BFS по-прежнему публикует только raw candidates и читает cached decisions.

### Что он делает

В одном tick он:

1. Проверяет, что игрок в игре (`is_ingame()`).
2. Лениво строит caches для item class names, unique names, set names.
3. Находит `pPaths` и `iPaths` через текущего игрока.
4. Делает первый проход по units, чтобы собрать `current_item_ids` и goblin events.
5. Делает второй проход по units, чтобы обработать каждый item.
6. Для нового item вызывает `scan_unit()`.
7. `scan_unit()` читает `ItemData`, получает имя/stats/sockets через injector и создает `ScannedItem`.
8. `to_event()` превращает `ScannedItem` в `ItemDropEvent`.
9. Если есть loot filter, вызывает `filter.decide(&MatchContext::new(&event))`.
10. Записывает hook-mask bits для show/hide/default поведения in-game labels.
11. Кладет `ItemDropEvent` в `recent_events`.
12. Кладет `CachedFilterDecision` в `recent_filter_decisions`.
13. Если filter decision содержит notification, возвращает event в `Vec<ItemDropEvent>`.
14. В конце prunes локальные и shared caches по `current_item_ids`.

Снаружи `app/scanner_runtime/worker.rs` берет возвращенные events и делает `app_handle.emit("item-drop", &item)`. Здесь же остаются attach/bootstrap, item/marker coordination и ordered shutdown; `mod.rs` содержит discovery/auto-start, а `readouts.rs` — readout/DPS sampling с заимствованными worker-owned counters/last-good values. Root `main.rs` собирает приложение и регистрирует close/join watchdog.

### Как он находит units

Текущий код идет таким путем:

```text
D2Client + PLAYER_UNIT
  -> p_player
    -> p_path / player path struct
      -> current Room1
        -> pPaths pointer (по offsets это `Room1.ppRoomsNear`)
        -> iPaths count
          -> for each nearby_room in pPaths:
               first unit = *(nearby_room + PATH_TO_UNIT / UNIT_FIRST)
               walk UnitAny.p_next_unit chain
```

Relevant code:

- `src-tauri/src/notifier/discovery.rs` (`DropScanner::tick_items`) - получает `pPaths` и `iPaths`.
- `src-tauri/src/notifier/discovery.rs` (`DropScanner::tick_items`) - первый проход собирает `current_item_ids`.
- `src-tauri/src/notifier/discovery.rs` (`DropScanner::tick_items`) - второй проход обрабатывает items.
- `src-tauri/src/notifier/processing.rs` (`DropScanner::scan_unit`) - обогащает новый item.

### Что важно про `pPaths`

Название `pPaths` может сбивать с толку. Это не история движения игрока. По текущим offsets это фактически `ppRoomsNear` текущей `Room1`: готовый массив nearby `Room1` pointers, который игра держит рядом с текущей позицией игрока. Соответственно, `iPaths` в этом пути фактически является count для этого массива (`dwRoomsNear`).

`DropScanner` доверяет этому готовому массиву. Он сам не строит graph traversal на несколько уровней. Если item не попал в эти nearby room entries или в unit chain, который scanner проходит от них, `DropScanner` этот item не обработает в данном tick.

### Какие данные получает `DropScanner`

Это полноценный путь обработки item:

```text
UnitAny
  -> ItemData
  -> ScannedItem
  -> ItemDropEvent
  -> MatchContext
  -> FilterDecision
  -> notification / hook bits / recent_filter_decisions / loot history
```

Именно здесь появляются:

- display name;
- base name;
- quality;
- stats;
- sockets;
- unique tier;
- filter notification color/sound settings;
- `place_on_map` decision;
- visibility decision (`Show`, `Hide`, `Default`).

Marker scanner этих данных сам не создает.

## Marker BFS подробно

Marker scanner сейчас вынесен в отдельный thread.

Files:

- `src-tauri/src/map_markers/mod.rs` — единая feature entry, экспортирует `MarkerScanner`.
- `src-tauri/src/map_markers/scanner.rs` — private coordinator; `scanner_tests.rs` объявлен под scanner.
- `src-tauri/src/map_markers/manager/mod.rs` — private `MapMarkerManager`, существующие методы и соседние children/tests. Shared Windows fixtures доступны через `map_markers::test_support::{Fixture,calls}`.

### Что он делает

В одном tick он:

1. Проверяет, что есть player unit.
2. Проверяет, есть ли loaded filter config.
3. Проверяет, есть ли в filter config `map` rules.
4. Если map rules нет, очищает app markers один раз и выходит.
5. Запускает `manager::bfs_item_positions(&ctx, 10)`.
6. Для каждого найденного `p_unit` читает `unit_id`.
7. Делает snapshot `recent_filter_decisions`.
8. Для каждого BFS item проверяет, есть ли current-generation cached decision.
9. Если decision говорит `place_on_map = true` и visibility не `Hide`, добавляет `MarkerItem`.
10. Передает markers в `MapMarkerManager::tick()`.
11. `MapMarkerManager` reconcile'ит persistent markers и пишет automap cells.

Relevant code:

- `src-tauri/src/map_markers/scanner.rs` (`MarkerScanner::tick`) - setup, early returns, BFS, чтение `unit_id`, snapshot filter decisions и сбор `MarkerItem`.
- `src-tauri/src/map_markers/manager/mod.rs` (`bfs_item_positions`) - сам BFS по `Room1` graph.

### Как BFS находит units

Текущий код идет таким путем:

```text
D2Client + PLAYER_UNIT
  -> p_player
    -> p_path
      -> current Room1
        -> Room1.UNIT_FIRST
          -> walk UnitAny.ROOM_NEXT chain for this room
        -> Room1.ppRoomsNear
          -> neighbour Room1
            -> neighbour.UNIT_FIRST
            -> neighbour.ROOM_NEXT chain
          -> next depth...
```

BFS не использует готовый `pPaths`/`iPaths` массив как основной источник области обхода. Он сам расширяет область поиска по соседним комнатам.

### Какие данные получает BFS

`bfs_item_positions()` возвращает только:

```rust
Vec<(p_unit, sub_x, sub_y)>
```

После этого `MarkerScanner` дополнительно читает `unit_id` из `p_unit`.

То есть marker BFS знает:

- pointer на unit (`p_unit`);
- `unit_id`;
- world/subtile координаты (`sub_x`, `sub_y`);
- automap cell coordinates после `sub_to_cell()`.

Marker BFS сам не знает:

- имя item;
- quality;
- stats;
- sockets;
- unique tier;
- совпадает ли item с loot filter rule;
- надо ли проигрывать notification sound;
- надо ли писать loot history entry.

Из-за этого marker scanner обязан читать `recent_filter_decisions`, которые пишет `DropScanner`.

## Как scanner threads связаны сейчас

Сейчас связь двусторонняя, но ответственность остается разделенной:

```text
MarkerScanner thread
  -> recent_bfs_items
       -> DropScanner thread

DropScanner thread
  -> recent_events
  -> recent_filter_decisions
       -> MarkerScanner thread
```

`MarkerScanner` пишет:

- `recent_bfs_items: HashMap<unit_id, BfsItemCandidate>`.

`DropScanner` пишет:

- `recent_events: HashMap<unit_id, ItemDropEvent>`;
- `recent_filter_decisions: HashMap<unit_id, CachedFilterDecision>`.

`DropScanner` читает `recent_bfs_items`, проверяет, что candidate pointer все еще указывает на live item с тем же `unit_id`, и для неизвестных/stale candidates запускает обычный `scan_unit()` / filter path.

`MarkerScanner` читает `recent_filter_decisions` и решает, можно ли ставить marker для BFS-found `unit_id`.

## Принципиальные отличия

| Вопрос                           | DropScanner `pPaths` path                                                 | Marker BFS path                                  |
| -------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------ |
| Где стартует?                    | От player unit и player path                                              | От player unit и player path                     |
| Какая стартовая комната?         | Current `Room1` игрока                                                    | Current `Room1` игрока                           |
| Как расширяет область?           | Берет готовый `pPaths`/`iPaths` список (`ppRoomsNear` текущей комнаты)    | Сам обходит `Room1.ppRoomsNear` graph            |
| Как идет по units?               | От `nearby_room + PATH_TO_UNIT`/`UNIT_FIRST`, затем `UnitAny.p_next_unit` | От `Room1.UNIT_FIRST`, затем `UnitAny.ROOM_NEXT` |
| Получает имя/stats?              | Да                                                                        | Нет                                              |
| Прогоняет loot filter?           | Да                                                                        | Нет                                              |
| Эмитит notification?             | Да, через `app/scanner_runtime/worker.rs`                                 | Нет                                              |
| Пишет `recent_filter_decisions`? | Да                                                                        | Нет                                              |
| Ставит automap marker?           | Нет напрямую                                                              | Да, но только по cached decision от DropScanner  |
| Публикует BFS candidates?        | Нет                                                                       | Да, через `recent_bfs_items`                     |

Особенно важны разные unit-chain поля:

- `unit::NEXT_UNIT` / `0xE4` - `pListNext`, game-wide/list chain.
- `unit::ROOM_NEXT` / `0xE8` - `pRoomNext`, per-room chain.

См. `src-tauri/src/offsets.rs:137-153`.

## Почему множества найденных items могут отличаться

Это не утверждение, что BFS всегда лучше `DropScanner`. Корректнее: они читают разные структуры, поэтому могут иметь разные множества `unit_id` в конкретный tick.

Возможны три случая:

```text
1. DropScanner видит item, BFS видит item
   -> нормальный общий случай.

2. DropScanner видит item, BFS не видит item
   -> notification появится, marker может не появиться или появиться позже.

3. BFS видит item, DropScanner не видит item через pPaths
   -> DropScanner может обработать verified BFS candidate на следующем item tick,
      создать notification и записать cached filter decision для marker scanner.
```

Причины потенциального расхождения:

- `pPaths`/`iPaths` может представлять не тот же набор комнат, который достижим через BFS depth 10.
- Готовый nearby-list игры может обновляться в другой момент, чем `Room1` graph.
- `DropScanner` и BFS идут по разным unit chains внутри/около rooms.
- В момент движения, загрузки/выгрузки rooms или смерти монстра структуры могут быть кратковременно несинхронны.
- Read failures в одном пути могут остановить конкретный chain walk, а другой путь все еще сможет дойти до item через другую структуру.

## Как это связано с off-screen drops

Игровой сценарий:

1. Игрок стоит в комнате A.
2. Summon, DoT, AoE, projectile или другой delayed damage убивает монстра в соседней комнате B.
3. Item появляется в loaded runtime structures игры.
4. Marker BFS, обходя `Room1` graph, может найти `p_unit`, `unit_id` и coordinates item в комнате B.
5. Marker scanner публикует candidate в `recent_bfs_items`.
6. `DropScanner` в тот же период может не увидеть item через свой `pPaths` path, но читает BFS candidate snapshot.
7. `DropScanner` проверяет `p_unit`, подтверждает тот же live `unit_id`, обогащает item и пишет `recent_filter_decisions[unit_id]`.
8. Frontend получает notification из обычного `DropScanner::tick_items()` path.
9. Marker scanner на следующем pass видит current-generation filter decision и может поставить marker.

Важно: до появления marker BFS такой drop тоже мог быть пропущен `DropScanner`, если он действительно не попадал в `pPaths` path. BFS не создал эту слепую зону. BFS просто дал второй канал обнаружения, который теперь подключен обратно к notification/filter pipeline.

## Что нужно измерить, чтобы не гадать

Чтение кода показывает архитектурную возможность расхождения, но не доказывает частоту в игре. Для подтверждения нужны aggregated diagnostics.

Минимальные счетчики:

```text
bfs_items_count
ppaths_items_count
bfs_only_count = bfs_unit_ids - ppaths_current_item_ids
ppaths_only_count = ppaths_current_item_ids - bfs_unit_ids
bfs_only_without_filter_decision_count
bfs_only_enriched_later_count
```

Полезные дополнительные поля:

- BFS depth, на котором найден item.
- Distance/subtile distance от игрока.
- Был ли item позже увиден `DropScanner`.
- Был ли для item создан `recent_filter_decisions`.
- Был ли notification emitted.
- Был ли marker placed.

Если `bfs_only_count` всегда равен 0 на реальных сценариях, то P1 off-screen hypothesis почти неактуальна. Если `bfs_only_without_filter_decision_count` ненулевой, это подтверждает, что текущая архитектура теряет часть найденных BFS items.

## Практический вывод

Не стоит считать один путь строго правильным, а другой неправильным. Их роли разные:

- `DropScanner` - authoritative pipeline для понимания item и пользовательских действий.
- Marker BFS - spatial discovery path для координат и automap markers.

Spatial discovery теперь передает неизвестные items обратно в authoritative pipeline. Фикс не заменяет `DropScanner` на BFS; он добавляет обратную связь:

```text
Marker BFS found candidates
  -> shared BFS candidates snapshot
    -> DropScanner enriches unknown candidates
      -> recent_filter_decisions
        -> MarkerScanner places markers
        -> app/scanner_runtime/worker.rs emits notifications
```

Такой подход сохраняет разделение ответственности и использует BFS как дополнительный источник кандидатов, а не как второй независимый loot scanner. Ограничение остается намеренным: если фильтр не содержит `map` rules, marker BFS не запускается, чтобы не возвращать прежнюю цену BFS для notification-only конфигураций.
