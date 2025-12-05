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
  sound: "#8be9fd", // Cyan (sound keywords)
  tier: "#bd93f9", // Purple (tier keywords)
  modifier: "#c7b377", // Gold italic (eth)
  display: "#aaaaaa", // Light gray (name/stat)
  invalid: "#ff5555", // Red (errors)
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
  ".tok-sound": { color: colors.sound },
  ".tok-modifier": { color: colors.modifier, fontStyle: "italic" },
  ".tok-display": { color: colors.display },

  // Invalid tokens
  ".tok-invalid": {
    color: colors.invalid,
    textDecoration: "underline wavy",
    textDecorationColor: colors.invalid,
  },
});

/**
 * Get theme extensions for dark mode
 */
export function getDarkThemeExtensions() {
  return [darkTheme, darkHighlighting, qualityHighlighting];
}

/**
 * Get theme extensions for light mode
 */
export function getLightThemeExtensions() {
  return [lightTheme, lightHighlighting, qualityHighlighting];
}


