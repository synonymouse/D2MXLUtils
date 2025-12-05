/**
 * D2 Rules Editor module
 *
 * CodeMirror 6 based editor for D2Stats-style loot filter DSL
 *
 * @module editor
 */

// Main editor component
export { default as RulesEditor } from "./RulesEditor.svelte";

// Language support
export { d2rules, d2rulesLanguage } from "./d2rules-language";

// Themes
export {
  darkTheme,
  lightTheme,
  darkHighlighting,
  lightHighlighting,
  qualityHighlighting,
  getDarkThemeExtensions,
  getLightThemeExtensions,
} from "./d2rules-theme";

// Linting
export { d2rulesLinter, validateDsl } from "./d2rules-linter";


