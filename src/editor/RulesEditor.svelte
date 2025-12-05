<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, type Extension } from "@codemirror/state";
  import {
    EditorView,
    keymap,
    highlightActiveLine,
    highlightActiveLineGutter,
    drawSelection,
    dropCursor,
    rectangularSelection,
    crosshairCursor,
    highlightSpecialChars,
  } from "@codemirror/view";
  import {
    defaultKeymap,
    history,
    historyKeymap,
    indentWithTab,
  } from "@codemirror/commands";
  import { bracketMatching } from "@codemirror/language";
  import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
  import { lintGutter } from "@codemirror/lint";

  import { d2rules } from "./d2rules-language";
  import { getDarkThemeExtensions } from "./d2rules-theme";
  import { d2rulesLinter } from "./d2rules-linter";

  interface Props {
    /** Editor content (two-way bindable) */
    value?: string;
    /** Make editor read-only */
    readonly?: boolean;
    /** Additional CSS class */
    class?: string;
    /** Called when content changes */
    onchange?: (value: string) => void;
    /** Called when Ctrl+S is pressed */
    onsave?: (value: string) => void;
  }

  let {
    value = $bindable(""),
    readonly = false,
    class: className = "",
    onchange,
    onsave,
  }: Props = $props();

  let container: HTMLDivElement;
  let view: EditorView | null = null;

  // Track if we're updating from external value change
  let isExternalUpdate = false;

  /**
   * Build editor extensions
   */
  function buildExtensions(): Extension[] {
    const extensions: Extension[] = [
      // Basic editor features
      highlightSpecialChars(),
      history(),
      drawSelection(),
      dropCursor(),
      EditorState.allowMultipleSelections.of(true),
      rectangularSelection(),
      crosshairCursor(),
      highlightActiveLine(),
      highlightActiveLineGutter(),

      // Lint gutter (ошибки/предупреждения слева без номеров строк)
      lintGutter(),

      // Bracket handling
      bracketMatching(),
      closeBrackets(),

      // Keymaps
      keymap.of([
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),

      // D2 Rules DSL language
      d2rules(),

      // Theme (dark mode by default)
      ...getDarkThemeExtensions(),

      // Real-time linting via Tauri
      d2rulesLinter(),

      // Listen for document changes
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !isExternalUpdate) {
          const newValue = update.state.doc.toString();
          value = newValue;
          onchange?.(newValue);
        }
      }),
    ];

    // Ctrl+S / Cmd+S to save
    if (onsave) {
      extensions.push(
        keymap.of([
          {
            key: "Mod-s",
            run: () => {
              onsave(view?.state.doc.toString() ?? value);
              return true;
            },
            preventDefault: true,
          },
        ])
      );
    }

    // Read-only mode
    if (readonly) {
      extensions.push(EditorState.readOnly.of(true));
    }

    return extensions;
  }

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: buildExtensions(),
      }),
      parent: container,
    });
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });

  // Sync external value changes to editor
  $effect(() => {
    if (view && value !== view.state.doc.toString()) {
      isExternalUpdate = true;
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: value,
        },
      });
      isExternalUpdate = false;
    }
  });

  /**
   * Focus the editor
   */
  export function focus() {
    view?.focus();
  }

  /**
   * Get current content
   */
  export function getContent(): string {
    return view?.state.doc.toString() ?? value;
  }
</script>

<div bind:this={container} class="rules-editor {className}"></div>

<style>
  .rules-editor {
    height: 100%;
    overflow: hidden;
    border-radius: var(--radius-md, 8px);
    border: 1px solid var(--border, #2a2a35);
    background: var(--bg-secondary, #1a1a1f);
  }

  .rules-editor :global(.cm-editor) {
    height: 100%;
  }

  .rules-editor :global(.cm-scroller) {
    overflow: auto;
    font-family: var(--font-mono, "Fira Code", "Consolas", monospace);
  }

  /* Lint gutter icon styling */
  .rules-editor :global(.cm-lint-marker-error) {
    content: "●";
  }

  .rules-editor :global(.cm-lint-marker-warning) {
    content: "●";
  }

  /* Diagnostic tooltip styling */
  .rules-editor :global(.cm-tooltip-lint) {
    background: var(--bg-elevated, #252530);
    border: 1px solid var(--border, #2a2a35);
    border-radius: var(--radius-sm, 4px);
    padding: 4px 8px;
    font-size: var(--text-sm, 13px);
    color: var(--text-primary, #e8e6e3);
    font-family: var(--font-sans, system-ui);
  }

  /* Ensure inner text in tooltips остаётся читабельным в обеих темах */
  .rules-editor :global(.cm-tooltip-lint *) {
    font-family: inherit;
    color: inherit;
  }

  .rules-editor :global(.cm-diagnostic) {
    padding: 4px 8px;
    margin: 0;
  }

  .rules-editor :global(.cm-diagnostic-error) {
    border-left: 3px solid var(--stat-fire, #ff4444);
  }

  .rules-editor :global(.cm-diagnostic-warning) {
    border-left: 3px solid var(--quality-rare, #ffff00);
  }

  .rules-editor :global(.cm-diagnostic-info) {
    border-left: 3px solid var(--quality-magic, #6969ff);
  }
</style>


