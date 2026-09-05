<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Compartment, EditorState, type Extension } from '@codemirror/state';
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightActiveLineGutter,
    drawSelection,
    dropCursor,
    rectangularSelection,
    crosshairCursor,
    highlightSpecialChars,
  } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import {
    bracketMatching,
    codeFolding,
    foldable,
    foldedRanges,
    foldEffect,
    unfoldEffect,
    foldGutter,
    foldKeymap,
  } from '@codemirror/language';
  import { acceptCompletion, closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
  import { lintGutter, setDiagnostics } from '@codemirror/lint';

  import { d2rules } from './d2rules-language';
  import { d2rulesFolding } from './d2rules-folding';
  import { matchHighlightField, setFlashLinesEffect } from './d2rules-match-highlight';
  import { getDarkThemeExtensions, getLightThemeExtensions } from './d2rules-theme';
  import { d2rulesLinter, type ValidationResult } from './d2rules-linter';
  import { d2rulesAutocomplete } from './d2rules-autocomplete';
  import { d2rulesHover } from './d2rules-hover';
  import { itemsDictionaryStore } from '../stores';

  /** Pick the editor theme that matches the active app theme. Reads the
   *  `data-theme` attribute on `<html>`, which `settingsStore.applyTheme`
   *  keeps in sync. */
  function themeExtensionsForCurrentMode(): Extension[] {
    const mode = document.documentElement.getAttribute('data-theme');
    return mode === 'light' ? getLightThemeExtensions() : getDarkThemeExtensions();
  }

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
    /** Called after validation completes with results */
    onvalidate?: (result: ValidationResult) => void;
    /** Group-rule line numbers to fold once real content loads (the caller
     *  typically mounts with `value` still empty and fills it in moments
     *  later via an async profile load). */
    initialFoldedLines?: number[];
    /** Called whenever the folded set changes, so the caller can persist it
     *  (e.g. per profile, across tab switches and app restarts). */
    onFoldsChange?: (lines: number[]) => void;
  }

  let {
    value = $bindable(''),
    readonly = false,
    class: className = '',
    onchange,
    onsave,
    onvalidate,
    initialFoldedLines,
    onFoldsChange,
  }: Props = $props();

  let container: HTMLDivElement;
  let view: EditorView | null = null;
  const themeCompartment = new Compartment();
  let themeObserver: MutationObserver | null = null;
  let flashClearTimer: ReturnType<typeof setTimeout> | null = null;

  // Track if we're updating from external value change
  let isExternalUpdate = false;
  // Suppress onFoldsChange while we're the ones applying initialFoldedLines,
  // so restoring folds doesn't immediately report the same state back.
  let isRestoringFolds = false;

  function currentFoldedLines(state: EditorState): number[] {
    const lines: number[] = [];
    foldedRanges(state).between(0, state.doc.length, (from) => {
      lines.push(state.doc.lineAt(from).number);
    });
    return lines;
  }

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
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),

      // Soft word wrap
      EditorView.lineWrapping,

      lintGutter(),

      // Bracket handling
      bracketMatching(),
      closeBrackets(),

      // Collapse group rule bodies (`[...] { ... }`) to a single line
      codeFolding(),
      foldGutter(),
      d2rulesFolding,

      // "Show matches" live highlight (see flashLines() below)
      matchHighlightField,

      // Keymaps
      keymap.of([
        ...closeBracketsKeymap,
        { key: 'Tab', run: acceptCompletion },
        ...defaultKeymap,
        ...historyKeymap,
        ...foldKeymap,
        indentWithTab,
      ]),

      // D2 Rules DSL language
      d2rules(),

      // Theme (swapped dynamically via compartment on app theme change)
      themeCompartment.of(themeExtensionsForCurrentMode()),

      d2rulesAutocomplete(() => itemsDictionaryStore.options),

      d2rulesLinter(500, onvalidate),

      d2rulesHover(),

      // Listen for document changes
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !isExternalUpdate) {
          const newValue = update.state.doc.toString();
          value = newValue;
          onchange?.(newValue);

          // Clear diagnostics immediately when user starts typing
          // They will reappear after the debounced linter runs
          update.view.dispatch(setDiagnostics(update.state, []));
        }

        if (
          !isRestoringFolds &&
          update.transactions.some((tr) =>
            tr.effects.some((e) => e.is(foldEffect) || e.is(unfoldEffect)),
          )
        ) {
          onFoldsChange?.(currentFoldedLines(update.state));
        }
      }),
    ];

    // Ctrl+S / Cmd+S to save
    if (onsave) {
      extensions.push(
        keymap.of([
          {
            key: 'Mod-s',
            run: () => {
              onsave(view?.state.doc.toString() ?? value);
              return true;
            },
            preventDefault: true,
          },
        ]),
      );
    }

    // Read-only mode
    if (readonly) {
      extensions.push(EditorState.readOnly.of(true));
    }

    return extensions;
  }

  // Callers (LootFilterTab) mount this with `value` still empty and fill it
  // in moments later via an async profile load, which lands as an external
  // value-sync transaction below. Applying initialFoldedLines only at mount
  // would fold an empty document and then have that full-document
  // replacement wipe the result. `foldsRestored` lets both call sites
  // attempt the restore and it takes effect whichever one first sees real
  // content.
  let foldsRestored = false;

  function restoreFolds() {
    if (!view || foldsRestored || !initialFoldedLines?.length) return;
    const effects = [];
    for (const lineNo of initialFoldedLines) {
      if (lineNo < 1 || lineNo > view.state.doc.lines) continue;
      const line = view.state.doc.line(lineNo);
      const range = foldable(view.state, line.from, line.to);
      if (range) effects.push(foldEffect.of(range));
    }
    if (effects.length) {
      isRestoringFolds = true;
      view.dispatch({ effects });
      isRestoringFolds = false;
      foldsRestored = true;
    }
  }

  onMount(() => {
    view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: buildExtensions(),
      }),
      parent: container,
    });

    // Watch <html data-theme> for changes and reconfigure the editor's theme
    // compartment so the editor tracks the app-wide theme toggle.
    themeObserver = new MutationObserver(() => {
      view?.dispatch({
        effects: themeCompartment.reconfigure(themeExtensionsForCurrentMode()),
      });
    });
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme'],
    });

    restoreFolds();
  });

  onDestroy(() => {
    themeObserver?.disconnect();
    themeObserver = null;
    if (flashClearTimer) clearTimeout(flashClearTimer);
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
      restoreFolds();
    }
  });

  /**
   * Focus the editor
   */
  export function focus() {
    view?.focus();
  }

  /**
   * Briefly highlight the given 1-based source lines ("show matches" mode).
   * Replaces any lines still flashing from a previous call and clears after
   * `holdMs` of inactivity.
   */
  export function flashLines(lines: number[], holdMs = 900) {
    if (!view) return;
    view.dispatch({ effects: setFlashLinesEffect.of(lines) });
    if (flashClearTimer) clearTimeout(flashClearTimer);
    flashClearTimer = setTimeout(() => {
      flashClearTimer = null;
      view?.dispatch({ effects: setFlashLinesEffect.of([]) });
    }, holdMs);
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
    max-height: 100%;
    overflow: hidden;
    border-radius: var(--radius-md, 8px);
    border: 1px solid var(--border-primary, #2a2a35);
    background: var(--bg-secondary, #1a1a1f);
  }

  .rules-editor :global(.cm-editor) {
    height: 100%;
    max-height: 100%;
  }

  .rules-editor :global(.cm-scroller) {
    overflow: auto;
    max-height: 100%;
    font-family: var(--font-mono, 'Fira Code', 'Consolas', monospace);
  }

  /* Lint gutter icon styling */
  .rules-editor :global(.cm-lint-marker-error) {
    content: '●';
  }

  .rules-editor :global(.cm-lint-marker-warning) {
    content: '●';
  }

  /* Diagnostic tooltip styling */
  .rules-editor :global(.cm-tooltip-lint) {
    background: var(--bg-elevated, #252530);
    border: 1px solid var(--border-primary, #2a2a35);
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

  /* "Show matches" live highlight — the rule line that just decided a
     drop's outcome. */
  .rules-editor :global(.cm-rule-flash) {
    background: color-mix(in srgb, var(--accent-primary, #c7b377) 28%, transparent);
    border-left: 3px solid var(--accent-primary, #c7b377);
  }

  .rules-editor :global(.cm-tooltip-hover-explain) {
    background: var(--bg-elevated, #252530);
    border: 1px solid var(--border-primary, #2a2a35);
    border-radius: var(--radius-sm, 4px);
    padding: 8px 12px;
    font-size: var(--text-sm, 13px);
    line-height: 1.5;
    color: var(--text-primary, #e8e6e3);
    font-family: var(--font-sans, system-ui);
    max-width: 480px;
    white-space: pre-line;
  }
</style>
