/**
 * CodeMirror 6 theme definitions for D2 Rules DSL editor
 *
 * Colors are inspired by Diablo 2 item quality palette
 *
 * @module d2rules-theme
 */
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";

// Diablo 2 palette colors
const colors = {
  // Item quality colors (from game)
  unique: "#c7b377", // Gold
  set: "#00ff00", // Bright green
  rare: "#ffff00", // Yellow
  magic: "#6969ff", // Blue
  crafted: "#ffa500", // Orange
  normal: "#888888", // Gray

  // Syntax colors
  comment: "#6a737d", // Dim gray (italic)
  string: "#e09956", // Orange (item patterns)
  regex: "#56d364", // Green (stat patterns)
  color: "#ff79c6", // Pink (color keywords)
  visibility: "#ff6b6b", // Red-ish (show/hide)
  directive: "#ffb86c", // Warm orange (hide default / show default — file-scope)
  notify: "#f1fa8c", // Yellow (notify — spec-critical keyword)
  sound: "#8be9fd", // Cyan (sound keywords)
  tier: "#bd93f9", // Purple (tier keywords)
  modifier: "#c7b377", // Gold italic (eth)
  display: "#aaaaaa", // Light gray (name/stat)
  map: "#ff4d4f", // Red (map — matches the in-game red-cross marker)
  groupBracket: "#8899aa", // Muted blue-grey for [] {} in groups
  invalid: "#cc0000",
  unknown: "#888888", // Unknown tokens (gray)
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
      caretColor: "var(--accent, #c7b377)",
      padding: "12px",
      lineHeight: "1.6",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent, #c7b377)",
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
      color: "var(--text-tertiary, #666)",
      border: "none",
      borderRight: "1px solid var(--border, #2a2a35)",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 12px 0 16px",
      minWidth: "3em",
    },
    ".cm-foldGutter": {
      width: "16px",
    },
    "&.cm-focused .cm-cursor": {
      borderLeftColor: "var(--accent, #c7b377)",
    },
    "&.cm-focused": {
      outline: "none",
    },
    // Scrollbar styling
    ".cm-scroller": {
      scrollbarWidth: "thin",
      scrollbarColor: "var(--border, #2a2a35) transparent",
    },
    ".cm-scroller::-webkit-scrollbar": {
      width: "8px",
      height: "8px",
    },
    ".cm-scroller::-webkit-scrollbar-thumb": {
      backgroundColor: "var(--border, #2a2a35)",
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
      caretColor: "var(--accent, #9a7b4f)",
      padding: "12px",
      lineHeight: "1.6",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--accent, #9a7b4f)",
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
      color: "var(--text-tertiary, #999)",
      border: "none",
      borderRight: "1px solid var(--border, #e0e0e0)",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 12px 0 16px",
      minWidth: "3em",
    },
    "&.cm-focused": {
      outline: "none",
    },
  },
  { dark: false }
);

/**
 * Syntax highlighting for dark theme
 */
export const darkHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.comment, color: colors.comment, fontStyle: "italic" },
    { tag: tags.string, color: colors.string },
    { tag: tags.regexp, color: colors.regex },
    { tag: tags.keyword, color: colors.normal },
    { tag: tags.invalid, color: colors.invalid, textDecoration: "underline wavy" },
  ])
);

/**
 * Syntax highlighting for light theme (adjusted colors for visibility)
 */
export const lightHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.comment, color: "#6a737d", fontStyle: "italic" },
    { tag: tags.string, color: "#b35900" },
    { tag: tags.regexp, color: "#116611" },
    { tag: tags.keyword, color: "#555555" },
    { tag: tags.invalid, color: "#cc0000", textDecoration: "underline wavy" },
  ])
);

/**
 * Custom class-based styling for quality-specific tokens
 *
 * These are applied via `.tok-{className}` classes in the editor
 */
export const qualityHighlighting = EditorView.baseTheme({
  // Quality colors (Diablo 2 palette)
  ".tok-qualityUnique": { color: colors.unique, fontWeight: "600" },
  ".tok-qualitySet": { color: colors.set, fontWeight: "600" },
  ".tok-qualityRare": { color: colors.rare },
  ".tok-qualityMagic": { color: colors.magic },
  ".tok-quality": { color: colors.normal },

  // Other keyword types
  ".tok-tier": { color: colors.tier },
  ".tok-color": { color: colors.color },
  ".tok-visibility": { color: colors.visibility, fontWeight: "600" },
  ".tok-directive": {
    color: colors.directive,
    fontWeight: "700",
    textTransform: "uppercase",
    letterSpacing: "0.5px",
  },
  ".tok-notify": { color: colors.notify, fontWeight: "600" },
  ".tok-sound": { color: colors.sound },
  ".tok-modifier": { color: colors.modifier, fontStyle: "italic" },
  ".tok-display": { color: colors.display },
  ".tok-map": { color: colors.map, fontWeight: "600" },
  ".tok-groupBracket": { color: colors.groupBracket, fontWeight: "700" },

  // Invalid tokens
  ".tok-invalid": {
    color: colors.invalid,
    textDecoration: "underline wavy",
    textDecorationColor: colors.invalid,
  },
});

export const autocompleteTheme = EditorView.baseTheme({
  ".cm-tooltip.cm-tooltip-autocomplete": {
    backgroundColor: "var(--bg-elevated, #252530)",
    border: "1px solid var(--border, #2a2a35)",
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
    backgroundColor: "var(--accent, #c7b377)",
    color: "var(--bg-primary, #0a0a0f)",
  },
  ".cm-completionLabel": {
    fontFamily: "inherit",
  },
  ".cm-completionMatchedText": {
    textDecoration: "none",
    fontWeight: "700",
    color: "var(--accent, #c7b377)",
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
  return [darkTheme, darkHighlighting, qualityHighlighting, autocompleteTheme];
}

/**
 * Get theme extensions for light mode
 */
export function getLightThemeExtensions() {
  return [lightTheme, lightHighlighting, qualityHighlighting, autocompleteTheme];
}


