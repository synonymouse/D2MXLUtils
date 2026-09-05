/**
 * CodeMirror 6 language definition for D2Stats-style loot filter DSL.
 */
import { StreamLanguage, LanguageSupport } from '@codemirror/language';
import { Tag } from '@lezer/highlight';

export const d2rulesTags = {
  tier: Tag.define(),
  quality: Tag.define(),
  ethereal: Tag.define(),
  socket: Tag.define(),
  action: Tag.define(),
  notification: Tag.define(),
  directive: Tag.define(),
  groupBracket: Tag.define(),
  unknown: Tag.define(),
  class: Tag.define(),
  level: Tag.define(),
};

const QUALITY_KEYWORDS = [
  'unique',
  'set',
  'rare',
  'magic',
  'craft',
  'crafted',
  'honor',
  'honorific',
  'low',
  'inferior',
  'normal',
  'superior',
];

const TIER_KEYWORDS = ['sacred', 'angelic', 'master', '0', '1', '2', '3', '4'];

const SOCKET_KEYWORDS = [
  'sockets0',
  'sockets1',
  'sockets2',
  'sockets3',
  'sockets4',
  'sockets5',
  'sockets6',
];

const COLOR_KEYWORDS = [
  'white',
  'red',
  'lime',
  'blue',
  'gold',
  'grey',
  'gray',
  'black',
  'pink',
  'orange',
  'yellow',
  'green',
  'purple',
];

const VISIBILITY_KEYWORDS = ['show', 'hide'];
const NOTIFY_KEYWORDS = ['notify'];
const SOUND_KEYWORD_REGEX = /^sound(_none|\d+)$/;

function isSoundKeyword(word: string): boolean {
  return SOUND_KEYWORD_REGEX.test(word);
}
const DISPLAY_KEYWORDS = ['stat'];
const MODIFIER_KEYWORDS = ['eth', 'quest'];
const MAP_KEYWORDS = ['map'];

/** Character class keywords + their short aliases (docs/filter_spec/loot-filter-dsl.md). */
const CLASS_KEYWORDS = [
  'amazon',
  'zon',
  'sorceress',
  'sorc',
  'necromancer',
  'necro',
  'paladin',
  'pal',
  'pally',
  'barbarian',
  'barb',
  'druid',
  'dru',
  'assassin',
  'sin',
];

/** Bare prefixes for `min_clvl<N>` / `max_clvl<N>` / `min_ilvl<N>` /
 *  `max_ilvl<N>` — numeric-suffix keywords, same shape as `sound<N>`. */
const LEVEL_KEYWORD_PREFIXES = ['min_clvl', 'max_clvl', 'min_ilvl', 'max_ilvl'];
const LEVEL_KEYWORD_REGEX = /^(min|max)_(clvl|ilvl)\d+$/;

function isLevelKeyword(word: string): boolean {
  return LEVEL_KEYWORD_REGEX.test(word);
}

const d2rulesLanguage = StreamLanguage.define({
  name: 'd2rules',

  tokenTable: {
    tier: d2rulesTags.tier,
    quality: d2rulesTags.quality,
    modifier: d2rulesTags.ethereal,
    socket: d2rulesTags.socket,
    visibility: d2rulesTags.action,
    notify: d2rulesTags.notification,
    color: d2rulesTags.notification,
    sound: d2rulesTags.notification,
    display: d2rulesTags.notification,
    map: d2rulesTags.notification,
    directive: d2rulesTags.directive,
    unknown: d2rulesTags.unknown,
    groupBracket: d2rulesTags.groupBracket,
    class: d2rulesTags.class,
    level: d2rulesTags.level,
  },

  token(stream) {
    // Skip whitespace
    if (stream.eatSpace()) return null;

    // Comments: # ...
    if (stream.match(/^#.*/)) {
      return 'comment';
    }

    // Strings in quotes: "item pattern"
    if (stream.peek() === '"') {
      stream.next(); // consume opening quote
      let escaped = false;
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '"' && !escaped) {
          return 'string';
        }
        escaped = ch === '\\';
      }
      // Unclosed quote - still return string but it will be marked as error by linter
      return 'string';
    }

    // Stat patterns in braces: {stat pattern}. Group body { ... } is handled
    // separately: a lone `{` or `}` on a line is recognised as a groupBracket.
    if (stream.peek() === '{') {
      // A bare `{` at end-of-visible-content is a group opener — don't consume
      // what follows as a stat pattern.
      const pos = stream.pos;
      stream.next();
      const rest = stream.string.slice(stream.pos).trim();
      if (rest === '' || rest.startsWith('#')) {
        return 'bracket groupBracket';
      }
      // Otherwise treat as a stat pattern up to the matching `}`.
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === '}') {
          return 'regexp';
        }
      }
      // Unclosed — fall through as regexp so the linter can flag it.
      void pos;
      return 'regexp';
    }

    if (stream.peek() === '}') {
      stream.next();
      return 'bracket groupBracket';
    }

    // Group header bracket: `[` ... `]` { ... }
    if (stream.peek() === '[') {
      stream.next();
      return 'bracket groupBracket';
    }
    if (stream.peek() === ']') {
      stream.next();
      return 'bracket groupBracket';
    }

    // Words (keywords or unknown)
    if (stream.match(/^\w+/)) {
      const word = stream.current().toLowerCase();

      // File-scope directive `hide default` / `show default`: both tokens get
      // the `directive` style when the line consists of exactly those two
      // words (ignoring trailing comments).
      if (word === 'default') {
        const prefix = stream.string.slice(0, stream.start).trim().toLowerCase();
        if (prefix === 'hide' || prefix === 'show') {
          return 'keyword directive';
        }
        return 'keyword unknown';
      }

      if (QUALITY_KEYWORDS.includes(word)) return 'keyword quality';

      // Tier keywords
      if (TIER_KEYWORDS.includes(word)) return 'keyword tier';

      // Socket-count keywords (sockets0..sockets6)
      if (SOCKET_KEYWORDS.includes(word)) return 'keyword socket';

      // Visibility (show / hide) — distinct from color. When the line is the
      // `hide default` / `show default` file-scope directive, emit the
      // `directive` style instead.
      if (VISIBILITY_KEYWORDS.includes(word)) {
        const prefix = stream.string.slice(0, stream.start).trim();
        const rest = stream.string.slice(stream.pos).replace(/#.*/, '').trim().toLowerCase();
        if (prefix === '' && rest === 'default') {
          return 'keyword directive';
        }
        return 'keyword visibility';
      }

      // Notify — the only thing that gates a notification.
      if (NOTIFY_KEYWORDS.includes(word)) return 'keyword notify';

      // Color keywords
      if (COLOR_KEYWORDS.includes(word)) return 'keyword color';

      // Sound keywords
      if (isSoundKeyword(word)) return 'keyword sound';

      // Character class keywords (and short aliases)
      if (CLASS_KEYWORDS.includes(word)) return 'keyword class';

      // Character/item level keywords (min_clvl<N>, max_clvl<N>, min_ilvl<N>, max_ilvl<N>)
      if (isLevelKeyword(word)) return 'keyword level';

      // Display keywords
      if (DISPLAY_KEYWORDS.includes(word)) return 'keyword display';

      // Modifier keywords
      if (MODIFIER_KEYWORDS.includes(word)) return 'keyword modifier';

      // Map-marker keyword
      if (MAP_KEYWORDS.includes(word)) return 'keyword map';

      // Unknown word - may be flagged by linter
      return 'keyword unknown';
    }

    // Consume any other character
    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: '#' },
  },
});

/**
 * Create a LanguageSupport instance for D2 Rules DSL
 */
export function d2rules(): LanguageSupport {
  return new LanguageSupport(d2rulesLanguage);
}

export { d2rulesLanguage };

export type SyntaxKeywordCategory =
  | 'tier'
  | 'quality'
  | 'socket'
  | 'color'
  | 'visibility'
  | 'notify'
  | 'display'
  | 'modifier'
  | 'map'
  | 'directive'
  | 'class'
  | 'level';

export interface SyntaxKeywordEntry {
  label: string;
  category: SyntaxKeywordCategory;
}

/** Bare DSL keywords (outside quoted item-name patterns) offered by autocomplete. */
export const SYNTAX_KEYWORDS: SyntaxKeywordEntry[] = [
  ...QUALITY_KEYWORDS.map((label) => ({ label, category: 'quality' as const })),
  ...TIER_KEYWORDS.filter((label) => !/^\d+$/.test(label)).map((label) => ({
    label,
    category: 'tier' as const,
  })),
  ...SOCKET_KEYWORDS.map((label) => ({ label, category: 'socket' as const })),
  ...COLOR_KEYWORDS.map((label) => ({ label, category: 'color' as const })),
  ...VISIBILITY_KEYWORDS.map((label) => ({ label, category: 'visibility' as const })),
  ...NOTIFY_KEYWORDS.map((label) => ({ label, category: 'notify' as const })),
  ...DISPLAY_KEYWORDS.map((label) => ({ label, category: 'display' as const })),
  ...MODIFIER_KEYWORDS.map((label) => ({ label, category: 'modifier' as const })),
  ...MAP_KEYWORDS.map((label) => ({ label, category: 'map' as const })),
  ...CLASS_KEYWORDS.map((label) => ({ label, category: 'class' as const })),
  ...LEVEL_KEYWORD_PREFIXES.map((label) => ({ label, category: 'level' as const })),
  { label: 'default', category: 'directive' as const },
];
