import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";

function isInsideQuotedString(line: string, offset: number): boolean {
  let inString = false;
  let inComment = false;
  let escaped = false;

  for (let i = 0; i < offset; i++) {
    if (inComment) return inString;
    const c = line[i];
    if (inString) {
      if (escaped) {
        escaped = false;
        continue;
      }
      if (c === "\\") {
        escaped = true;
        continue;
      }
      if (c === '"') {
        inString = false;
      }
    } else {
      if (c === "#") {
        inComment = true;
      } else if (c === '"') {
        inString = true;
      }
    }
  }

  return inString;
}

export function d2rulesAutocomplete(getItems: () => string[]) {
  return autocompletion({
    activateOnTyping: true,
    closeOnBlur: true,
    override: [
      (context: CompletionContext): CompletionResult | null => {
        const line = context.state.doc.lineAt(context.pos);
        const lineOffset = context.pos - line.from;

        if (!isInsideQuotedString(line.text, lineOffset)) {
          return null;
        }

        const wordBefore = context.matchBefore(/[A-Za-z0-9 \-']*/);
        if (!wordBefore) return null;
        if (wordBefore.from === wordBefore.to && !context.explicit) {
          return null;
        }

        const items = getItems();
        if (items.length === 0) return null;

        return {
          from: wordBefore.from,
          options: items.map((name) => ({
            label: name,
            type: "variable",
          })),
          validFor: /^[A-Za-z0-9 \-']*$/,
        };
      },
    ],
  });
}
