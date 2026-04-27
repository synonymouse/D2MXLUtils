/**
 * CodeMirror 6 theme definitions for the D2 Rules DSL editor.
 */
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { d2rulesTags } from "./d2rules-language";

const darkPalette = {
  comment: "#753501",
  string: "#e09956",
  regex: "#7caa70",
  invalid: "#cc0000",
  unknown: "#888888",
  groupBracket: "#8899aa",
  directive: "#ffb86c",
  tier: "#bd93f9",
  quality: "#888888",
  ethereal: "#56d4b6",
  action: "#e53935",
  notification: "#c4b870",
};

const lightPalette = {
  comment: "#aa805d",
  string: "#b35900",
  regex: "#116611",
  invalid: "#cc0000",
  unknown: "#7a7a7a",
  groupBracket: "#5a6877",
  directive: "#9c5a00",
  tier: "#7b1fa2",
  quality: "#555555",
  ethereal: "#00838f",
  action: "#d32f2f",
  notification: "#ad1457",
};

/**
 * Dark theme for the D2 Rules editor
 *
 * Uses CSS variables from the app's design system with fallbacks
 */
export const darkTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--bg-secondary, #1a1a1f)",
      color: "var(--text-primary, #e6e6e6)",
      fontSize: "var(--text-sm, 13px)",
      fontFamily: "var(--font-mono, 'Fira Code', 'Consolas', monospace)",
    },
    ".cm-content": {
      caretColor: "var(--accent-primary, #c7b377)",
      padding: "12px",
      lineHeight: "1.6",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent-primary, #c7b377)",
      borderLeftWidth: "2px",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(255, 255, 255, 0.04)",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      backgroundColor: "rgba(199, 179, 119, 0.2)",
    },
    ".cm-gutters": {
      backgroundColor: "var(--bg-tertiary, #12121a)",
      color: "var(--text-muted, #666)",
      border: "none",
      borderRight: "1px solid var(--border-primary, #2a2a35)",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 12px 0 16px",
      minWidth: "3em",
    },
    ".cm-foldGutter": {
      width: "16px",
    },
    "&.cm-focused .cm-cursor": {
      borderLeftColor: "var(--accent-primary, #c7b377)",
    },
    "&.cm-focused": {
      outline: "none",
    },
    // Scrollbar styling
    ".cm-scroller": {
      scrollbarWidth: "thin",
      scrollbarColor: "var(--border-primary, #2a2a35) transparent",
    },
    ".cm-scroller::-webkit-scrollbar": {
      width: "8px",
      height: "8px",
    },
    ".cm-scroller::-webkit-scrollbar-thumb": {
      backgroundColor: "var(--border-primary, #2a2a35)",
      borderRadius: "4px",
    },
    ".cm-scroller::-webkit-scrollbar-track": {
      backgroundColor: "transparent",
    },
  },
  { dark: true }
);

/**
 * Light theme for the D2 Rules editor
 */
export const lightTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--bg-secondary, #ffffff)",
      color: "var(--text-primary, #1a1a1a)",
      fontSize: "var(--text-sm, 13px)",
      fontFamily: "var(--font-mono, 'Fira Code', 'Consolas', monospace)",
    },
    ".cm-content": {
      caretColor: "var(--accent-primary, #9a7b4f)",
      padding: "12px",
      lineHeight: "1.6",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent-primary, #9a7b4f)",
      borderLeftWidth: "2px",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(0, 0, 0, 0.04)",
    },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": {
      backgroundColor: "rgba(154, 123, 79, 0.2)",
    },
    ".cm-gutters": {
      backgroundColor: "var(--bg-tertiary, #f5f5f5)",
      color: "var(--text-muted, #999)",
      border: "none",
      borderRight: "1px solid var(--border-primary, #e0e0e0)",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 12px 0 16px",
      minWidth: "3em",
    },
    "&.cm-focused .cm-cursor": {
      borderLeftColor: "var(--accent-primary, #9a7b4f)",
    },
    "&.cm-focused": {
      outline: "none",
    },
    // Scrollbar styling — mirrors dark theme so both use theme vars.
    ".cm-scroller": {
      scrollbarWidth: "thin",
      scrollbarColor: "var(--border-primary, #e0e0e0) transparent",
    },
    ".cm-scroller::-webkit-scrollbar": {
      width: "8px",
      height: "8px",
    },
    ".cm-scroller::-webkit-scrollbar-thumb": {
      backgroundColor: "var(--border-primary, #e0e0e0)",
      borderRadius: "4px",
    },
    ".cm-scroller::-webkit-scrollbar-track": {
      backgroundColor: "transparent",
    },
  },
  { dark: false }
);

function buildHighlighting(p: typeof darkPalette) {
  return syntaxHighlighting(
    HighlightStyle.define([
      { tag: tags.comment, color: p.comment, fontStyle: "italic" },
      { tag: tags.string, color: p.string },
      { tag: tags.regexp, color: p.regex },
      {
        tag: tags.invalid,
        color: p.invalid,
        textDecoration: "underline wavy",
      },
      { tag: d2rulesTags.tier, color: p.tier, fontWeight: "600" },
      { tag: d2rulesTags.quality, color: p.quality, fontWeight: "600" },
      {
        tag: d2rulesTags.ethereal,
        color: p.ethereal,
        fontStyle: "italic",
        fontWeight: "600",
      },
      { tag: d2rulesTags.action, color: p.action, fontWeight: "600" },
      {
        tag: d2rulesTags.notification,
        color: p.notification,
        fontWeight: "600",
      },
      {
        tag: d2rulesTags.directive,
        color: p.directive,
        fontWeight: "700",
        textTransform: "uppercase",
        letterSpacing: "0.5px",
      },
      {
        tag: d2rulesTags.groupBracket,
        color: p.groupBracket,
        fontWeight: "700",
      },
      { tag: d2rulesTags.unknown, color: p.unknown },
    ])
  );
}

export const darkHighlighting = buildHighlighting(darkPalette);
export const lightHighlighting = buildHighlighting(lightPalette);

export const autocompleteTheme = EditorView.baseTheme({
  ".cm-tooltip.cm-tooltip-autocomplete": {
    backgroundColor: "var(--bg-elevated, #252530)",
    border: "1px solid var(--border-primary, #2a2a35)",
    borderRadius: "var(--radius-sm, 4px)",
    fontFamily: "var(--font-mono, 'Fira Code', 'Consolas', monospace)",
    fontSize: "var(--text-sm, 13px)",
    color: "var(--text-primary, #e8e6e3)",
    boxShadow: "0 4px 12px rgba(0, 0, 0, 0.35)",
    overflow: "hidden",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul": {
    maxHeight: "260px",
    padding: "2px 0",
    fontFamily: "inherit",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li": {
    padding: "3px 10px 3px 0",
    lineHeight: "1.5",
    cursor: "pointer",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent-primary, #c7b377)",
    color: "var(--bg-primary, #0a0a0f)",
  },
  ".cm-completionLabel": {
    fontFamily: "inherit",
  },
  ".cm-completionMatchedText": {
    textDecoration: "none",
    fontWeight: "700",
    color: "var(--accent-primary, #c7b377)",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected] .cm-completionMatchedText":
    {
      color: "var(--bg-primary, #0a0a0f)",
    },
  ".cm-completionDetail": {
    marginLeft: "10px",
    fontSize: "0.85em",
    opacity: 0.65,
    fontStyle: "italic",
  },
  ".cm-completionIcon": {
    width: "2.8em",
    paddingRight: "0.3em",
    fontSize: "0.78em",
    fontStyle: "italic",
    opacity: 0.55,
    textAlign: "right",
    fontFamily: "var(--font-mono, inherit)",
  },
  ".cm-completionIcon-base::after": { content: '""' },
  ".cm-completionIcon-set::after": { content: '"set"' },
  ".cm-completionIcon-tu::after": { content: '"TU"' },
  ".cm-completionIcon-su::after": { content: '"SU"' },
  ".cm-completionIcon-ssu::after": { content: '"SSU"' },
  ".cm-completionIcon-sssu::after": { content: '"SSSU"' },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected] .cm-completionIcon":
    {
      opacity: 0.8,
      color: "var(--bg-primary, #0a0a0f)",
    },
});

/**
 * Get theme extensions for dark mode
 */
export function getDarkThemeExtensions() {
  return [darkTheme, darkHighlighting, autocompleteTheme];
}

/**
 * Get theme extensions for light mode
 */
export function getLightThemeExtensions() {
  return [lightTheme, lightHighlighting, autocompleteTheme];
}


