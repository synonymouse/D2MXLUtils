# CodeMirror 6 Integration Plan

## Цель

Создать полнофункциональный редактор правил лутфильтра на базе CodeMirror 6 с:

- Подсветкой синтаксиса DSL (как в D2Stats)
- Реал-тайм валидацией через Tauri команды
- Поддержкой тёмной/светлой темы
- Подчёркиванием ошибок и предупреждений

---

## Фаза 1: Установка и настройка

### 1.1 Установка зависимостей

```bash
pnpm add @codemirror/state @codemirror/view @codemirror/language @codemirror/commands @codemirror/autocomplete @codemirror/lint @lezer/highlight
```

**Пакеты:**

| Пакет | Назначение |

|-------|-----------|

| `@codemirror/state` | EditorState, transactions |

| `@codemirror/view` | EditorView, DOM рендеринг |

| `@codemirror/language` | Подсветка синтаксиса, фолдинг |

| `@codemirror/commands` | Базовые команды (undo, redo) |

| `@codemirror/autocomplete` | Автодополнение (будущее) |

| `@codemirror/lint` | Линтинг и маркеры ошибок |

| `@lezer/highlight` | Стилизация токенов |

---

## Фаза 2: Грамматика DSL

### 2.1 DSL Синтаксис (для подсветки)

```text
# Комментарий                     → comment
"Ring$"                           → string (item pattern)
unique rare magic set low normal  → keyword.quality
sacred angelic master 0 1 2 3 4   → keyword.tier
eth                               → keyword.modifier
{[3-5] to All Skills}             → regex (stat pattern)
gold lime red blue white ...      → keyword.color
hide show                         → keyword.action
sound1 sound2 ... sound6          → keyword.sound
name stat                         → keyword.display
```

### 2.2 Структура файлов

```
src/editor/
├── d2rules-language.ts     # Определение языка и подсветка
├── d2rules-theme.ts        # Темы редактора (dark/light)
├── d2rules-linter.ts       # Интеграция с validate_filter_dsl
└── RulesEditor.svelte      # Svelte-обёртка
```

### 2.3 Реализация языка (d2rules-language.ts)

Используем **StreamLanguage** для простого токенизатора (без полного Lezer):

```typescript
import { StreamLanguage, LanguageSupport } from "@codemirror/language";
import { tags, Tag } from "@lezer/highlight";

// Кастомные теги для DSL
export const d2ruleTags = {
  qualityUnique: Tag.define(),
  qualitySet: Tag.define(),
  qualityRare: Tag.define(),
  qualityMagic: Tag.define(),
  qualityNormal: Tag.define(),
  tier: Tag.define(),
  color: Tag.define(),
  sound: Tag.define(),
  statPattern: Tag.define(),
};

const QUALITY_KEYWORDS = ["unique", "set", "rare", "magic", "craft", "honor", "low", "normal", "superior"];
const TIER_KEYWORDS = ["sacred", "angelic", "master", "0", "1", "2", "3", "4"];
const COLOR_KEYWORDS = ["transparent", "white", "red", "lime", "blue", "gold", "grey", "gray", "black", "pink", "orange", "yellow", "green", "purple", "hide", "show"];
const SOUND_KEYWORDS = ["sound_none", "sound1", "sound2", "sound3", "sound4", "sound5", "sound6"];
const DISPLAY_KEYWORDS = ["name", "stat"];
const MODIFIER_KEYWORDS = ["eth"];

const d2rulesLanguage = StreamLanguage.define({
  token(stream, state) {
    // Комментарии
    if (stream.match(/^#.*/)) return "comment";
    
    // Строки в кавычках (item pattern)
    if (stream.match(/^"[^"]*"/)) return "string";
    if (stream.match(/^"[^"]*/)) return "string invalid"; // незакрытая кавычка
    
    // Stat pattern в фигурных скобках
    if (stream.match(/^\{[^}]*\}/)) return "regexp";
    if (stream.match(/^\{[^}]*/)) return "regexp invalid";
    
    // Ключевые слова
    if (stream.match(/^\w+/)) {
      const word = stream.current().toLowerCase();
      
      if (word === "unique") return "keyword qualityUnique";
      if (word === "set") return "keyword qualitySet";
      if (word === "rare") return "keyword qualityRare";
      if (word === "magic" || word === "craft") return "keyword qualityMagic";
      if (QUALITY_KEYWORDS.includes(word)) return "keyword quality";
      if (TIER_KEYWORDS.includes(word)) return "keyword tier";
      if (COLOR_KEYWORDS.includes(word)) return "keyword color";
      if (SOUND_KEYWORDS.includes(word)) return "keyword sound";
      if (DISPLAY_KEYWORDS.includes(word)) return "keyword display";
      if (MODIFIER_KEYWORDS.includes(word)) return "keyword modifier";
      
      return "invalid"; // неизвестное слово
    }
    
    stream.next();
    return null;
  },
});

export function d2rules(): LanguageSupport {
  return new LanguageSupport(d2rulesLanguage);
}
```

---

## Фаза 3: Темизация

### 3.1 Тема редактора (d2rules-theme.ts)

```typescript
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";

// Цвета из игры (Diablo 2 palette)
const colors = {
  unique: "#c7b377",    // Gold
  set: "#00ff00",       // Bright green
  rare: "#ffff00",      // Yellow
  magic: "#6969ff",     // Blue
  crafted: "#ffa500",   // Orange
  normal: "#888888",    // Gray
  comment: "#6a737d",   // Dim gray
  string: "#e09956",    // Orange (item patterns)
  regex: "#56d364",     // Green (stat patterns)
  color: "#ff79c6",     // Pink (color keywords)
  sound: "#8be9fd",     // Cyan (sound keywords)
  tier: "#bd93f9",      // Purple (tier keywords)
  invalid: "#ff5555",   // Red (errors)
};

export const darkTheme = EditorView.theme({
  "&": {
    backgroundColor: "var(--bg-secondary, #1a1a1f)",
    color: "var(--text-primary, #e6e6e6)",
    fontSize: "var(--text-sm, 13px)",
    fontFamily: "var(--font-mono)",
  },
  ".cm-content": {
    caretColor: "var(--accent, #c7b377)",
    padding: "var(--space-3, 12px)",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--accent, #c7b377)",
  },
  ".cm-activeLine": {
    backgroundColor: "rgba(255,255,255,0.05)",
  },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
    backgroundColor: "rgba(199, 179, 119, 0.2)",
  },
  ".cm-gutters": {
    backgroundColor: "var(--bg-tertiary, #12121a)",
    color: "var(--text-tertiary, #666)",
    border: "none",
    borderRight: "1px solid var(--border, #2a2a35)",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 8px 0 16px",
  },
}, { dark: true });

export const darkHighlighting = syntaxHighlighting(HighlightStyle.define([
  { tag: tags.comment, color: colors.comment, fontStyle: "italic" },
  { tag: tags.string, color: colors.string },
  { tag: tags.regexp, color: colors.regex },
  { tag: tags.invalid, color: colors.invalid, textDecoration: "underline wavy" },
  // Quality-specific colors
  { tag: tags.keyword, color: colors.normal },
]));

// Кастомные классы для качества предметов
export const qualityHighlighting = EditorView.baseTheme({
  ".tok-qualityUnique": { color: colors.unique, fontWeight: "bold" },
  ".tok-qualitySet": { color: colors.set, fontWeight: "bold" },
  ".tok-qualityRare": { color: colors.rare },
  ".tok-qualityMagic": { color: colors.magic },
  ".tok-tier": { color: colors.tier },
  ".tok-color": { color: colors.color },
  ".tok-sound": { color: colors.sound },
  ".tok-modifier": { color: colors.unique, fontStyle: "italic" },
  ".tok-display": { color: "#aaa" },
  ".tok-invalid": { color: colors.invalid, textDecoration: "underline wavy red" },
});
```

---

## Фаза 4: Svelte-обёртка

### 4.1 RulesEditor.svelte

```svelte
<script lang="ts">
  import { onMount, onDestroy, createEventDispatcher } from 'svelte';
  import { EditorState, type Extension } from '@codemirror/state';
  import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
  import { d2rules } from './d2rules-language';
  import { darkTheme, darkHighlighting, qualityHighlighting } from './d2rules-theme';
  import { d2rulesLinter } from './d2rules-linter';

  interface Props {
    value?: string;
    readonly?: boolean;
    class?: string;
  }
  
  let { value = $bindable(''), readonly = false, class: className = '' }: Props = $props();
  
  const dispatch = createEventDispatcher<{
    change: { value: string };
    save: { value: string };
  }>();
  
  let container: HTMLDivElement;
  let view: EditorView | null = null;
  
  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    history(),
    keymap.of([...defaultKeymap, ...historyKeymap]),
    d2rules(),
    darkTheme,
    darkHighlighting,
    qualityHighlighting,
    d2rulesLinter(),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        value = update.state.doc.toString();
        dispatch('change', { value });
      }
    }),
    // Ctrl+S для сохранения
    keymap.of([{
      key: "Mod-s",
      run: () => {
        dispatch('save', { value });
        return true;
      }
    }]),
  ];
  
  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: readonly 
          ? [...extensions, EditorState.readOnly.of(true)]
          : extensions,
      }),
      parent: container,
    });
  });
  
  onDestroy(() => {
    view?.destroy();
  });
  
  // Синхронизация внешних изменений value
  $effect(() => {
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: value,
        },
      });
    }
  });
</script>

<div bind:this={container} class="rules-editor {className}"></div>

<style>
  .rules-editor {
    height: 100%;
    overflow: auto;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
  }
  
  .rules-editor :global(.cm-editor) {
    height: 100%;
  }
  
  .rules-editor :global(.cm-scroller) {
    overflow: auto;
  }
</style>
```

---

## Фаза 5: Линтинг

### 5.1 Интеграция с validate_filter_dsl (d2rules-linter.ts)

```typescript
import { linter, type Diagnostic } from "@codemirror/lint";
import { invoke } from "@tauri-apps/api/core";

interface ValidationError {
  line: number;
  column: number;
  message: string;
  severity: "error" | "warning" | "info";
}

export function d2rulesLinter() {
  return linter(async (view) => {
    const doc = view.state.doc;
    const text = doc.toString();
    
    if (!text.trim()) return [];
    
    try {
      const errors: ValidationError[] = await invoke("validate_filter_dsl", { text });
      
      return errors.map((err): Diagnostic => {
        const line = doc.line(Math.min(err.line, doc.lines));
        return {
          from: line.from,
          to: line.to,
          severity: err.severity,
          message: err.message,
        };
      });
    } catch (e) {
      console.error("Linter error:", e);
      return [];
    }
  }, {
    delay: 300, // debounce 300ms
  });
}
```

---

## Фаза 6: LootFilterTab Integration

### 6.1 Обновление LootFilterTab.svelte

```svelte
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import RulesEditor from '../editor/RulesEditor.svelte';
  import Button from '../components/Button.svelte';
  
  let dslText = $state(`# My Loot Filter
# Notify on unique items
"." unique gold sound1

# Hide normal items  
"." normal hide

# Rings with +skills
"Ring$" rare {Skills} lime sound2 stat
`);
  
  let parseStatus = $state<'idle' | 'parsing' | 'valid' | 'error'>('idle');
  let parseErrors = $state<string[]>([]);
  let ruleCount = $state(0);
  
  async function parseFilter() {
    parseStatus = 'parsing';
    parseErrors = [];
    
    try {
      const config = await invoke('parse_filter_dsl', { text: dslText });
      ruleCount = config.rules?.length ?? 0;
      parseStatus = 'valid';
    } catch (e: any) {
      parseErrors = Array.isArray(e) ? e.map((err: any) => err.message) : [String(e)];
      parseStatus = 'error';
    }
  }
  
  function handleChange(e: CustomEvent<{ value: string }>) {
    dslText = e.detail.value;
    parseStatus = 'idle';
  }
  
  async function handleSave(e: CustomEvent<{ value: string }>) {
    await parseFilter();
    if (parseStatus === 'valid') {
      // TODO: Save to profile
      console.log('Filter saved!');
    }
  }
</script>

<section class="loot-filter-tab">
  <header class="tab-header">
    <h2>Loot Filter Editor</h2>
    <div class="actions">
      <Button onclick={parseFilter} disabled={parseStatus === 'parsing'}>
        {parseStatus === 'parsing' ? 'Parsing...' : 'Parse'}
      </Button>
      <span class="status" class:valid={parseStatus === 'valid'} class:error={parseStatus === 'error'}>
        {#if parseStatus === 'valid'}
          ✓ {ruleCount} rules
        {:else if parseStatus === 'error'}
          ✗ {parseErrors.length} errors
        {/if}
      </span>
    </div>
  </header>
  
  <div class="editor-container">
    <RulesEditor 
      bind:value={dslText}
      onchange={handleChange}
      onsave={handleSave}
    />
  </div>
  
  {#if parseErrors.length > 0}
    <div class="error-panel">
      {#each parseErrors as error}
        <div class="error-line">{error}</div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .loot-filter-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: var(--space-3);
    padding: var(--space-4);
  }
  
  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  
  .tab-header h2 {
    margin: 0;
    font-size: var(--text-lg);
    color: var(--text-primary);
  }
  
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  
  .status {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }
  
  .status.valid {
    color: var(--quality-set);
  }
  
  .status.error {
    color: var(--stat-fire);
  }
  
  .editor-container {
    flex: 1;
    min-height: 200px;
    overflow: hidden;
  }
  
  .error-panel {
    background: rgba(255, 68, 68, 0.1);
    border: 1px solid var(--stat-fire);
    border-radius: var(--radius-md);
    padding: var(--space-3);
    max-height: 100px;
    overflow-y: auto;
  }
  
  .error-line {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--stat-fire);
    padding: var(--space-1) 0;
  }
</style>
```

---

## Фаза 7: Дополнительные улучшения

### 7.1 Автосохранение профиля

**Архитектура хранения:**

- **settings** (`settings.svelte.ts`) — хранит только `activeProfile: string` (имя активного профиля)
- **профили** (`%APPDATA%/D2MXLUtils/profiles/*.json`) — хранят сам текст фильтра

Добавить в `settings.svelte.ts`:

```typescript
// Имя активного профиля (сам текст фильтра хранится в профиле)
let activeProfile = $state('default');

export function getActiveProfile() { return activeProfile; }
export function setActiveProfile(name: string) { 
  activeProfile = name;
  saveSettings();
}
```

**Tauri команды для профилей** (реализация в пункте 3.5 основного плана):

```rust
// src-tauri/src/profiles.rs (будущий модуль)
#[tauri::command]
pub fn list_profiles() -> Vec<String>;

#[tauri::command]
pub fn load_profile(name: &str) -> Result<String, String>; // возвращает DSL текст

#[tauri::command]
pub fn save_profile(name: &str, dsl_text: &str) -> Result<(), String>;

#[tauri::command]
pub fn delete_profile(name: &str) -> Result<(), String>;
```

**Debounced автосохранение в LootFilterTab:**

```typescript
// В LootFilterTab.svelte
let saveTimeout: ReturnType<typeof setTimeout> | null = null;

function debouncedSave(dslText: string, delay = 1000) {
  if (saveTimeout) clearTimeout(saveTimeout);
  saveTimeout = setTimeout(async () => {
    const profileName = getActiveProfile();
    await invoke('save_profile', { name: profileName, dslText });
  }, delay);
}

function handleChange(e: CustomEvent<{ value: string }>) {
  dslText = e.detail.value;
  parseStatus = 'idle';
  debouncedSave(dslText); // автосохранение с debounce
}
```

### 7.2 Поддержка светлой темы

Добавить `lightTheme` в `d2rules-theme.ts` с инвертированными цветами и переключением по `data-theme`.

---

## Порядок реализации

| # | Задача | Файлы | Зависимости |

|---|--------|-------|-------------|

| 1 | Установить пакеты CodeMirror 6 | `package.json` | — |

| 2 | Создать `d2rules-language.ts` | `src/editor/` | #1 |

| 3 | Создать `d2rules-theme.ts` | `src/editor/` | #1 |

| 4 | Создать `d2rules-linter.ts` | `src/editor/` | #1 |

| 5 | Создать `RulesEditor.svelte` | `src/editor/` | #2, #3, #4 |

| 6 | Обновить `LootFilterTab.svelte` | `src/views/` | #5 |

| 7 | Добавить `activeProfile` в settings | `src/stores/settings.svelte.ts` | #6 |

---

## Ключевые соображения

### Почему StreamLanguage вместо полного Lezer?

- DSL простой (построчный, без вложенности)
- StreamLanguage проще в реализации и отладке
- Достаточен для подсветки и базовой навигации
- Полный Lezer можно добавить позже для autocomplete (пункт 3.4)

### Интеграция с темами приложения

- Редактор использует CSS-переменные из `variables.css`
- При смене темы (`data-theme`) редактор автоматически адаптируется
- Цвета качества предметов соответствуют игровым (Diablo 2 palette)

### Производительность линтинга

- Debounce 300ms предотвращает спам запросов
- `validate_filter_dsl` — легковесная операция на Rust
- Ошибки подсвечиваются без блокировки ввода

### Разделение ответственности: settings vs профили

- **settings** — глобальные настройки приложения (тема, хоткеи, `activeProfile`)
- **профили** — отдельные JSON файлы с текстом фильтра (пункт 3.5 основного плана)
- Tauri команды для профилей (`save_profile`, `load_profile`) реализуются отдельно