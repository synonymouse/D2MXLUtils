# Autocomplete для DSL редактора

## Обзор

Добавление автодополнения (autocomplete) в CodeMirror 6 редактор правил лутфильтра. Предлагать:

- Базовые типы предметов (Ring, Amulet, Armor)
- Уникальные предметы (Stone of Jordan, Shako)
- Сетовые предметы
- Ключевые слова DSL (unique, rare, eth, gold, sound1)

**Источник данных**: Извлечение из MPQ архивов MedianXL → статический JSON в приложении.

---

## Извлечение данных из MPQ

### Структура MedianXL

```
C:\d2\median-xl\
├── medianxl-bG9jYWw.mpq        ← local = локализация (вероятно текстовые данные)
├── medianxl-aXRlbXNnZng.mpq   ← itemsgfx = графика предметов
└── medianxl-*.mpq              ← другие ресурсы
```

### Целевые файлы внутри MPQ

После извлечения нужны:

```
data/global/excel/
├── items.txt          ← Базовые типы (Ring, Amulet, etc.)
├── uniqueitems.txt    ← Уникальные предметы
├── setitems.txt       ← Сетовые предметы
└── runes.txt          ← Руны (опционально)

data/local/lng/
├── item-names.txt     ← Локализованные названия
└── item-namestr.txt   ← ID строк
```

### Способы извлечения

#### Вариант A: Ручное извлечение (MPQ Editor)

**Инструмент**: [MPQ Editor by Ladik](http://www.zezula.net/en/mpq/download.html)

**Шаги**:

1. Скачать и установить MPQ Editor
2. Открыть все `medianxl-*.mpq` архивы по очереди
3. Найти `data/global/excel/*.txt` файлы
4. Извлечь в папку проекта `scripts/extracted/`
5. Запустить скрипт обработки (см. ниже)

**Плюсы**: Не требует установки библиотек

**Минусы**: Нужно повторять вручную при обновлениях

#### Вариант B: Автоматизация (Python + mpyq)

**Установка**:

```bash
pip install mpyq
```

**Скрипт**: `scripts/extract-items.py`

```python
#!/usr/bin/env python3
import mpyq
import csv
import json
from pathlib import Path

MPQ_DIR = Path('C:/d2/median-xl')
OUTPUT_FILE = Path('src/assets/items-data.json')

def extract_from_mpq(mpq_path, file_path):
    """Извлечь файл из MPQ архива"""
    try:
        archive = mpyq.MPQArchive(str(mpq_path))
        return archive.read_file(file_path).decode('latin-1')
    except:
        return None

def parse_tsv(content):
    """Парсинг TSV файла"""
    lines = content.strip().split('\n')
    reader = csv.DictReader(lines, delimiter='\t')
    return list(reader)

def extract_items():
    """Извлечь названия всех предметов"""
    result = {
        'base_items': [],
        'unique_items': [],
        'set_items': [],
        'version': 'MedianXL Sigma',
        'extracted_at': None
    }
    
    # Пробуем разные MPQ файлы
    mpq_files = list(MPQ_DIR.glob('medianxl-*.mpq'))
    
    for mpq_file in mpq_files:
        print(f"Checking {mpq_file.name}...")
        
        # Пытаемся извлечь items.txt
        content = extract_from_mpq(mpq_file, 'data/global/excel/items.txt')
        if content and not result['base_items']:
            rows = parse_tsv(content)
            result['base_items'] = [row.get('name', '') for row in rows if row.get('name')]
            print(f"  ✓ Found {len(result['base_items'])} base items")
        
        # Пытаемся извлечь uniqueitems.txt
        content = extract_from_mpq(mpq_file, 'data/global/excel/uniqueitems.txt')
        if content and not result['unique_items']:
            rows = parse_tsv(content)
            result['unique_items'] = [row.get('index', '') for row in rows if row.get('index')]
            print(f"  ✓ Found {len(result['unique_items'])} unique items")
        
        # Пытаемся извлечь setitems.txt
        content = extract_from_mpq(mpq_file, 'data/global/excel/setitems.txt')
        if content and not result['set_items']:
            rows = parse_tsv(content)
            result['set_items'] = [row.get('index', '') for row in rows if row.get('index')]
            print(f"  ✓ Found {len(result['set_items'])} set items")
    
    # Сохраняем результат
    import datetime
    result['extracted_at'] = datetime.datetime.now().isoformat()
    
    OUTPUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_FILE, 'w', encoding='utf-8') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    
    print(f"\n✓ Saved to {OUTPUT_FILE}")
    return result

if __name__ == '__main__':
    data = extract_items()
    print(f"\nTotal items extracted:")
    print(f"  Base: {len(data['base_items'])}")
    print(f"  Unique: {len(data['unique_items'])}")
    print(f"  Set: {len(data['set_items'])}")
```

**Использование**:

```bash
python scripts/extract-items.py
```

**Плюсы**: Автоматизация, легко обновлять

**Минусы**: Требует Python и mpyq

---

## Архитектура

```
┌─────────────────────────────────────────────────────────┐
│                   AUTOCOMPLETE FLOW                     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  [MPQ архивы MedianXL]                                  │
│         │                                               │
│         ▼                                               │
│  ┌──────────────────┐                                   │
│  │ Скрипт извлечения│  ← Ручной (MPQ Editor)            │
│  │ (один раз)       │    или Python/Node.js             │
│  └────────┬─────────┘                                   │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────┐                           │
│  │ items-data.json          │  ← Статический файл       │
│  │ {                        │    в assets/              │
│  │   base_items: [...],     │                           │
│  │   unique_items: [...],   │                           │
│  │   set_items: [...]       │                           │
│  │ }                        │                           │
│  └────────┬─────────────────┘                           │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────┐                           │
│  │ Frontend: импорт JSON    │  ← import itemsData       │
│  │ (compile-time)           │                           │
│  └────────┬─────────────────┘                           │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────┐                           │
│  │ CodeMirror Extension     │  ← d2rules-autocomplete.ts│
│  │ autocompletion()         │                           │
│  └────────┬─────────────────┘                           │
│           │                                             │
│           ▼                                             │
│  [Пользователь печатает]                                │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────┐                           │
│  │ Контекстный анализ       │  ← Курсор внутри "..."?   │
│  │ позиции курсора          │    После качества?        │
│  └────────┬─────────────────┘                           │
│           │                                             │
│           ▼                                             │
│  ┌──────────────────────────┐                           │
│  │ Фильтрация вариантов     │  ← По введенному тексту   │
│  └────────┬─────────────────┘                           │
│           │                                             │
│           ▼                                             │
│  [Показ выпадающего списка]                             │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## Реализация

### Этап 1: Извлечение данных

**Создать файлы**:

- `scripts/extract-items.py` - скрипт Python (см. выше)
- `scripts/README.md` - инструкции по обновлению данных

**Запустить**:

```bash
python scripts/extract-items.py
```

**Результат**: `src/assets/items-data.json`

### Этап 2: Ключевые слова DSL

**Создать**: `src/assets/keywords-data.json`

```json
{
  "quality": [
    "unique", "set", "rare", "magic", "craft", "honor",
    "low", "normal", "superior"
  ],
  "tier": [
    "sacred", "angelic", "master",
    "0", "1", "2", "3", "4"
  ],
  "color": [
    "white", "red", "lime", "blue", "gold", "grey", "gray",
    "black", "pink", "orange", "yellow", "green", "purple",
    "hide", "show", "transparent"
  ],
  "sound": [
    "sound_none",
    "sound1", "sound2", "sound3", "sound4", "sound5", "sound6"
  ],
  "display": ["name", "stat"],
  "modifier": ["eth"]
}
```

### Этап 3: CodeMirror Extension

**Создать**: `src/editor/d2rules-autocomplete.ts`

```typescript
import { autocompletion, type CompletionContext, type CompletionResult, type Completion } from '@codemirror/autocomplete';

// Импортируем статические данные
import itemsData from '../assets/items-data.json';
import keywordsData from '../assets/keywords-data.json';

interface AutocompleteData {
  base_items: string[];
  unique_items: string[];
  set_items: string[];
}

const items: AutocompleteData = itemsData as AutocompleteData;

// Собираем все ключевые слова в один массив
const allKeywords = [
  ...keywordsData.quality,
  ...keywordsData.tier,
  ...keywordsData.color,
  ...keywordsData.sound,
  ...keywordsData.display,
  ...keywordsData.modifier,
];

/**
 * Создать extension автодополнения для DSL редактора
 */
export function d2rulesAutocomplete() {
  return autocompletion({
    override: [
      (context: CompletionContext): CompletionResult | null => {
        return getCompletions(context);
      }
    ],
    activateOnTyping: true,
    closeOnBlur: true,
    maxRenderedOptions: 100,
  });
}

/**
 * Получить список вариантов автодополнения
 */
function getCompletions(context: CompletionContext): CompletionResult | null {
  const { state, pos } = context;
  const line = state.doc.lineAt(pos);
  const lineText = line.text;
  const lineOffset = pos - line.from;
  
  // Определяем контекст курсора
  const isInsideQuotes = isInQuotedString(lineText, lineOffset);
  
  // Получаем слово перед курсором
  const wordBefore = context.matchBefore(/[\w$^.*+?[\]{}()|\\-]*/);
  
  if (!wordBefore || (wordBefore.from === wordBefore.to && !context.explicit)) {
    return null;
  }
  
  const options: Completion[] = [];
  
  if (isInsideQuotes) {
    // Внутри кавычек - предлагаем названия предметов
    options.push(
      ...items.base_items.map(name => ({
        label: name,
        type: 'variable',
        detail: 'base type',
      })),
      ...items.unique_items.map(name => ({
        label: name,
        type: 'constant',
        detail: 'unique',
        boost: 2, // Приоритет уникальным
      })),
      ...items.set_items.map(name => ({
        label: name,
        type: 'constant',
        detail: 'set',
        boost: 1,
      }))
    );
  } else {
    // Вне кавычек - ключевые слова
    options.push(
      ...allKeywords.map(kw => ({
        label: kw,
        type: 'keyword',
      }))
    );
  }
  
  return {
    from: wordBefore.from,
    options,
    filter: true, // CodeMirror сам отфильтрует по введенному тексту
  };
}

/**
 * Проверить, находится ли курсор внутри кавычек
 */
function isInQuotedString(text: string, offset: number): boolean {
  let inQuotes = false;
  let escaped = false;
  
  for (let i = 0; i < offset; i++) {
    if (escaped) {
      escaped = false;
      continue;
    }
    
    if (text[i] === '\\') {
      escaped = true;
      continue;
    }
    
    if (text[i] === '"') {
      inQuotes = !inQuotes;
    }
  }
  
  return inQuotes;
}
```

### Этап 4: Интеграция в редактор

**Изменить**: `src/editor/RulesEditor.svelte`

```svelte
<script lang="ts">
  // ... existing imports ...
  import { d2rulesAutocomplete } from './d2rules-autocomplete';
  
  function buildExtensions(): Extension[] {
    const extensions: Extension[] = [
      // ... existing extensions ...
      
      // D2 Rules DSL language
      d2rules(),
      
      // Theme
      ...getDarkThemeExtensions(),
      
      // Autocomplete ← ДОБАВИТЬ
      d2rulesAutocomplete(),
      
      // Linter
      d2rulesLinter(500, onvalidate),
      
      // ... rest of extensions ...
    ];
    
    return extensions;
  }
</script>
```

### Этап 5: Стилизация

**Изменить**: `src/editor/d2rules-theme.ts`

Добавить стили для `.cm-tooltip-autocomplete`:

```typescript
export function getDarkThemeExtensions(): Extension[] {
  return [
    EditorView.theme({
      // ... existing styles ...
      
      // Autocomplete dropdown
      '.cm-tooltip-autocomplete': {
        background: 'var(--bg-elevated, #252530)',
        border: '1px solid var(--border, #2a2a35)',
        borderRadius: 'var(--radius-sm, 4px)',
        fontFamily: 'var(--font-mono)',
      },
      '.cm-tooltip-autocomplete ul': {
        maxHeight: '300px',
        overflowY: 'auto',
      },
      '.cm-tooltip-autocomplete li': {
        padding: '4px 8px',
        cursor: 'pointer',
      },
      '.cm-tooltip-autocomplete li[aria-selected]': {
        background: 'var(--accent, #c7b377)',
        color: 'var(--bg-primary, #0a0a0f)',
      },
      '.cm-completionIcon': {
        marginRight: '6px',
        opacity: 0.7,
      },
      '.cm-completionLabel': {
        fontFamily: 'var(--font-mono)',
      },
      '.cm-completionDetail': {
        marginLeft: '8px',
        fontSize: '0.85em',
        opacity: 0.6,
      },
    }, { dark: true }),
  ];
}
```

### Этап 6: Экспорт из модуля

**Изменить**: `src/editor/index.ts`

```typescript
export { d2rules, d2rulesLanguage } from './d2rules-language';
export { getDarkThemeExtensions, getLightThemeExtensions } from './d2rules-theme';
export { d2rulesLinter } from './d2rules-linter';
export { d2rulesAutocomplete } from './d2rules-autocomplete'; // ← ДОБАВИТЬ
```

---

## Обновление данных

### При выходе новой версии MedianXL:

1. Запустить скрипт извлечения:
   ```bash
   python scripts/extract-items.py
   # или ручное извлечение через MPQ Editor
   ```

2. Проверить обновленный файл:
   ```bash
   cat src/assets/items-data.json | head -20
   ```

3. Закоммитить изменения:
   ```bash
   git add src/assets/items-data.json
   git commit -m "Update items data for MedianXL vX.X"
   ```

4. Пересобрать приложение:
   ```bash
   pnpm build
   pnpm tauri build
   ```


---

## Fallback данные

На случай, если извлечение не удалось, создать минимальный набор:

**Создать**: `src/assets/items-data.json` (начальный)

```json
{
  "base_items": [
    "Ring", "Amulet",
    "Body Armor", "Belt", "Boots", "Gloves", "Helm", "Shield",
    "Sword", "Axe", "Mace", "Dagger", "Spear", "Polearm",
    "Bow", "Crossbow", "Staff", "Wand", "Scepter"
  ],
  "unique_items": [
    "Shako", "Stone of Jordan", "Harlequin Crest"
  ],
  "set_items": [],
  "version": "Fallback",
  "extracted_at": null
}
```

---

## Тестирование

1. Открыть редактор правил
2. Начать вводить внутри кавычек: `"Rin` → должен показать Ring
3. Начать вводить вне кавычек: `uni` → должен показать unique
4. Проверить фильтрацию по вводу
5. Проверить навигацию стрелками и выбор Enter

---

## Примеры использования

**Автодополнение внутри кавычек**:

```
"Rin|"       →  Ring
"Amu|"       →  Amulet
"Shak|"      →  Shako (unique)
```

**Автодополнение вне кавычек**:

```
"Ring" uni|  →  unique
"Ring" eth|  →  eth
"Ring" sou|  →  sound1, sound2, sound3...
```

**Regex паттерны**:

```
"Ring$|"     →  Ring$ (точное совпадение)
"^Amu|"      →  ^Amulet (начало строки)
```

---

## TODO

- [ ] Извлечь данные из MPQ (скрипт или MPQ Editor) → items-data.json
- [ ] Создать keywords-data.json с ключевыми словами DSL
- [ ] Реализовать d2rules-autocomplete.ts extension
- [ ] Интегрировать автодополнение в RulesEditor.svelte
- [ ] Стилизация выпадающего списка в d2rules-theme.ts
- [ ] Документация по обновлению данных в scripts/README.md