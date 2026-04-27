import { hoverTooltip, type Tooltip } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";

const cache = new Map<string, string | null>();

async function explain(text: string): Promise<string | null> {
  if (cache.has(text)) {
    return cache.get(text) ?? null;
  }
  let result: string | null = null;
  try {
    result = (await invoke<string | null>("explain_filter_line", {
      line: text,
    })) ?? null;
  } catch (e) {
    console.error("[d2rules-hover] explain_filter_line failed:", e);
    result = null;
  }
  cache.set(text, result);
  return result;
}

export function d2rulesHover() {
  return hoverTooltip(
    async (view, pos): Promise<Tooltip | null> => {
      const line = view.state.doc.lineAt(pos);
      const text = line.text;
      if (!text.trim()) return null;
      const explanation = await explain(text);
      if (!explanation) return null;
      return {
        pos: line.from,
        end: line.to,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "cm-tooltip-hover-explain";
          dom.textContent = explanation;
          return { dom };
        },
      };
    },
    { hideOnChange: true }
  );
}
