import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from '@codemirror/autocomplete';
import type { AutocompleteOption } from '../stores/items-dictionary.svelte';
import { SYNTAX_KEYWORDS } from './d2rules-language';
import { settingsStore } from '../stores';

type LineContext = 'string' | 'stat' | 'comment' | 'code';

/** Classify the cursor position within a line: inside a quoted item-name
 *  pattern, inside a `{stat pattern}`, past a `#` comment, or plain code
 *  (keywords, group brackets). Each region gets its own completion source. */
function classifyPosition(line: string, offset: number): LineContext {
  let inString = false;
  let inBrace = false;
  let escaped = false;

  for (let i = 0; i < offset; i++) {
    const c = line[i];
    if (inString) {
      if (escaped) {
        escaped = false;
        continue;
      }
      if (c === '\\') {
        escaped = true;
        continue;
      }
      if (c === '"') {
        inString = false;
      }
      continue;
    }
    if (inBrace) {
      if (c === '}') {
        inBrace = false;
      }
      continue;
    }
    if (c === '#') {
      return 'comment';
    }
    if (c === '"') {
      inString = true;
      continue;
    }
    if (c === '{') {
      inBrace = true;
      continue;
    }
  }

  if (inString) return 'string';
  if (inBrace) return 'stat';
  return 'code';
}

function itemNameCompletion(getOptions: () => AutocompleteOption[]) {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const lineOffset = context.pos - line.from;

    if (classifyPosition(line.text, lineOffset) !== 'string') {
      return null;
    }

    const wordBefore = context.matchBefore(/[A-Za-z0-9 \-']*/);
    if (!wordBefore) return null;
    if (wordBefore.from === wordBefore.to && !context.explicit) {
      return null;
    }

    const opts = getOptions();
    if (opts.length === 0) return null;

    return {
      from: wordBefore.from,
      options: opts.map((o) => ({
        label: o.label,
        type: o.kind,
      })),
      validFor: /^[A-Za-z0-9 \-']*$/,
    };
  };
}

function syntaxKeywordCompletion(context: CompletionContext): CompletionResult | null {
  const line = context.state.doc.lineAt(context.pos);
  const lineOffset = context.pos - line.from;

  if (classifyPosition(line.text, lineOffset) !== 'code') {
    return null;
  }

  const wordBefore = context.matchBefore(/[A-Za-z0-9_]+/);
  if (!wordBefore) return null;
  if (wordBefore.from === wordBefore.to && !context.explicit) {
    return null;
  }

  const soundOptions = settingsStore.settings.sounds.map((_, i) => ({
    label: `sound${i + 1}`,
    category: 'notify' as const,
  }));

  return {
    from: wordBefore.from,
    options: [
      ...SYNTAX_KEYWORDS,
      { label: 'sound_none', category: 'notify' as const },
      ...soundOptions,
    ].map((k) => ({
      label: k.label,
      type: 'keyword',
    })),
    validFor: /^[A-Za-z0-9_]*$/,
  };
}

export function d2rulesAutocomplete(getOptions: () => AutocompleteOption[]) {
  return autocompletion({
    activateOnTyping: true,
    closeOnBlur: true,
    override: [itemNameCompletion(getOptions), syntaxKeywordCompletion],
  });
}
