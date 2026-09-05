/**
 * "Show matches" live highlight: briefly flashes the source line(s) of
 * whichever rule(s) just decided a dropped item's outcome. Backed by a
 * single `StateField` whose decoration set is entirely replaced on each
 * flash — callers own the fade-out timer (see `RulesEditor.flashLines`).
 */
import { StateEffect, StateField } from '@codemirror/state';
import { Decoration, EditorView, type DecorationSet } from '@codemirror/view';

export const setFlashLinesEffect = StateEffect.define<number[]>();

const flashLineDecoration = Decoration.line({ attributes: { class: 'cm-rule-flash' } });

export const matchHighlightField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none;
  },
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const effect of tr.effects) {
      if (effect.is(setFlashLinesEffect)) {
        const ranges = effect.value
          .filter((lineNo) => lineNo >= 1 && lineNo <= tr.state.doc.lines)
          .map((lineNo) => flashLineDecoration.range(tr.state.doc.line(lineNo).from))
          .sort((a, b) => a.from - b.from);
        deco = Decoration.set(ranges);
      }
    }
    return deco;
  },
  provide: (field) => EditorView.decorations.from(field),
});
