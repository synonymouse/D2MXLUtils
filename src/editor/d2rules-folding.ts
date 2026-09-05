/**
 * Code folding for the D2 Rules DSL: lets group bodies (`[...] { ... }`)
 * collapse to a single line. Groups cannot nest (see loot-filter-dsl.md),
 * so matching an opener to its closer is a simple forward scan.
 */
import { foldService } from '@codemirror/language';
import type { EditorState } from '@codemirror/state';

function stripComment(text: string): string {
  const idx = text.indexOf('#');
  return idx === -1 ? text : text.slice(0, idx);
}

/** A line opens a group when its last non-comment, non-space character is a
 *  bare `{` — mirrors the tokenizer's groupBracket detection in
 *  d2rules-language.ts (a stat pattern like `{All Skills}` always closes on
 *  the same line, so it can never be mistaken for a group opener here). */
export function isGroupOpenLine(text: string): boolean {
  return stripComment(text).trimEnd().endsWith('{');
}

/** A line closes a group when it consists of nothing but `}` (plus
 *  whitespace/comment). */
export function isGroupCloseLine(text: string): boolean {
  return stripComment(text).trim() === '}';
}

export const d2rulesFolding = foldService.of((state: EditorState, lineStart: number) => {
  const openLine = state.doc.lineAt(lineStart);
  if (!isGroupOpenLine(openLine.text)) return null;

  for (let lineNo = openLine.number + 1; lineNo <= state.doc.lines; lineNo++) {
    const line = state.doc.line(lineNo);
    if (isGroupCloseLine(line.text)) {
      if (line.from <= openLine.to) return null;
      return { from: openLine.to, to: line.from };
    }
  }
  return null;
});
