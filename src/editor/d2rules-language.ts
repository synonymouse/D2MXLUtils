/**
 * CodeMirror 6 language definition for D2Stats-style loot filter DSL
 *
 * @module d2rules-language
 */
import { StreamLanguage, LanguageSupport } from "@codemirror/language";

// Quality keywords (item quality)
const QUALITY_KEYWORDS = [
  "unique",
  "set",
  "rare",
  "magic",
  "craft",
  "honor",
  "low",
  "normal",
  "superior",
];

// Tier keywords (item tier)
const TIER_KEYWORDS = [
  "sacred",
  "angelic",
  "master",
  "0",
  "1",
  "2",
  "3",
  "4",
];

// Color keywords
const COLOR_KEYWORDS = [
  "transparent",
  "white",
  "red",
  "lime",
  "blue",
  "gold",
  "grey",
  "gray",
  "black",
  "pink",
  "orange",
  "yellow",
  "green",
  "purple",
  "hide",
  "show",
];

// Sound keywords
const SOUND_KEYWORDS = [
  "sound_none",
  "sound1",
  "sound2",
  "sound3",
  "sound4",
  "sound5",
  "sound6",
];

// Display mode keywords
const DISPLAY_KEYWORDS = ["name", "stat"];

// Modifier keywords
const MODIFIER_KEYWORDS = ["eth"];

/**
 * StreamLanguage tokenizer for D2 Rules DSL
 *
 * Token types returned:
 * - comment: Lines starting with #
 * - string: Item name patterns in quotes ("...")
 * - regexp: Stat patterns in braces {...}
 * - keyword: Quality, tier, color, sound, display, modifier keywords
 * - invalid: Unknown tokens
 */
const d2rulesLanguage = StreamLanguage.define({
  name: "d2rules",

  token(stream) {
    // Skip whitespace
    if (stream.eatSpace()) return null;

    // Comments: # ...
    if (stream.match(/^#.*/)) {
      return "comment";
    }

    // Strings in quotes: "item pattern"
    if (stream.peek() === '"') {
      stream.next(); // consume opening quote
      let escaped = false;
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '"' && !escaped) {
          return "string";
        }
        escaped = ch === "\\";
      }
      // Unclosed quote - still return string but it will be marked as error by linter
      return "string";
    }

    // Stat patterns in braces: {stat pattern}
    if (stream.peek() === "{") {
      stream.next(); // consume opening brace
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === "}") {
          return "regexp";
        }
      }
      // Unclosed brace
      return "regexp";
    }

    // Words (keywords or unknown)
    if (stream.match(/^\w+/)) {
      const word = stream.current().toLowerCase();

      // Quality keywords with specific styling
      if (word === "unique") return "keyword qualityUnique";
      if (word === "set") return "keyword qualitySet";
      if (word === "rare") return "keyword qualityRare";
      if (word === "magic" || word === "craft") return "keyword qualityMagic";
      if (QUALITY_KEYWORDS.includes(word)) return "keyword quality";

      // Tier keywords
      if (TIER_KEYWORDS.includes(word)) return "keyword tier";

      // Color keywords
      if (COLOR_KEYWORDS.includes(word)) return "keyword color";

      // Sound keywords
      if (SOUND_KEYWORDS.includes(word)) return "keyword sound";

      // Display keywords
      if (DISPLAY_KEYWORDS.includes(word)) return "keyword display";

      // Modifier keywords
      if (MODIFIER_KEYWORDS.includes(word)) return "keyword modifier";

      // Unknown word - may be flagged by linter
      return "invalid";
    }

    // Consume any other character
    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "#" },
  },
});

/**
 * Create a LanguageSupport instance for D2 Rules DSL
 */
export function d2rules(): LanguageSupport {
  return new LanguageSupport(d2rulesLanguage);
}

export { d2rulesLanguage };


