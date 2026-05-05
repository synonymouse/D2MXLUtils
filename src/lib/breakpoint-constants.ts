export interface ClassInfo {
    id: number;
    name: string;
    token: string;
}

export interface MorphInfo {
    name: string;
    token: string;
    baseClass: string;
}

export interface MercInfo {
    id: number;
    name: string;
    token: string;
}

export interface DebuffInfo {
    name: string;
    value: number;
}

export interface WeaponType {
    token: string;
    name: string;
    primaryAnim: string;
    blockAnim: string;
}

export const CLASSES: ClassInfo[] = [
    { id: 0, name: "Amazon", token: "AM" },
    { id: 1, name: "Sorceress", token: "SO" },
    { id: 2, name: "Necromancer", token: "NE" },
    { id: 3, name: "Paladin", token: "PA" },
    { id: 4, name: "Barbarian", token: "BA" },
    { id: 5, name: "Druid", token: "DZ" },
    { id: 6, name: "Assassin", token: "AI" },
];

export const MORPHS: MorphInfo[] = [
    { name: "Werewolf", token: "40", baseClass: "DZ" },
    { name: "Werebear", token: "TG", baseClass: "DZ" },
    { name: "Wereowl", token: "OW", baseClass: "DZ" },
    { name: "Superbeast", token: "~Z", baseClass: "PA" },
    { name: "Deathlord", token: "0N", baseClass: "NE" },
    { name: "Treewarden", token: "TH", baseClass: "BA" },
];

export const MERCS: MercInfo[] = [
    { id: 0, name: "Rogue (Act 1)", token: "RG" },
    { id: 1, name: "Town Guard (Act 2)", token: "GU" },
    { id: 2, name: "Shapeshifter (Act 2)", token: "GU" },
    { id: 3, name: "Iron Wolf (Act 3)", token: "IW" },
    { id: 4, name: "Son of Harrogath (Act 5)", token: "0A" },
];

export const DEBUFFS: DebuffInfo[] = [
    { name: "None", value: 0 },
    { name: "Decrepify", value: -20 },
    { name: "Phobos", value: -20 },
    { name: "Uldyssian", value: -30 },
    { name: "Chill", value: -50 },
];

export const ANIM_TYPES = ["A1", "SC", "GH", "BL"] as const;
export type AnimType = (typeof ANIM_TYPES)[number];

export const ANIM_TYPE_LABELS: Record<AnimType, string> = {
    A1: "Attack",
    SC: "Cast",
    GH: "Hit Recovery",
    BL: "Block",
};

// Mirrors https://dev.median-xl.com/speedcalc/. primaryAnim is the COF
// weapon token used for A1/SC/GH; blockAnim for BL.
export const WEAPON_TYPES: WeaponType[] = [
    { token: "swor", name: "One-Handed Swords", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "crsd", name: "Crystal Swords", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "2hsd", name: "Two-Handed Swords", primaryAnim: "2HS", blockAnim: "1HS" },
    { token: "axe", name: "One-Handed Axes", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "2hax", name: "Two-Handed Axes", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "mace", name: "Maces", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "hamm", name: "Hammers", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "scep", name: "Sceptres", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "jave", name: "Javelins", primaryAnim: "1HT", blockAnim: "1HT" },
    { token: "spea", name: "Spears", primaryAnim: "2HT", blockAnim: "1HS" },
    { token: "scyh", name: "Scythes", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "knif", name: "Daggers", primaryAnim: "1HT", blockAnim: "1HT" },
    { token: "tkni", name: "Throwing Knives", primaryAnim: "1HT", blockAnim: "1HT" },
    { token: "taxe", name: "Throwing Axes", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "staf", name: "Staves", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "bow", name: "Bows", primaryAnim: "BOW", blockAnim: "1HS" },
    { token: "xbow", name: "Crossbows", primaryAnim: "XBW", blockAnim: "1HS" },
    { token: "abow", name: "Amazon Bows", primaryAnim: "BOW", blockAnim: "1HS" },
    { token: "aspe", name: "Amazon Spears", primaryAnim: "2HT", blockAnim: "1HS" },
    { token: "ajav", name: "Amazon Javelins", primaryAnim: "1HT", blockAnim: "1HT" },
    { token: "h2h", name: "Assassin Claws", primaryAnim: "HT1", blockAnim: "HT1" },
    { token: "nagi", name: "Assassin Naginatas", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "bswd", name: "Barbarian Swords", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "baxe", name: "Barbarian One-Handed Axes", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "2hbx", name: "Barbarian Two-Handed Axes", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "dbow", name: "Druid Bows", primaryAnim: "BOW", blockAnim: "1HS" },
    { token: "dstf", name: "Druid Staves", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "nscy", name: "Necromancer Scythes", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "nstf", name: "Necromancer Staves", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "nknf", name: "Necromancer Daggers", primaryAnim: "1HT", blockAnim: "1HT" },
    { token: "nxbw", name: "Necromancer Crossbows", primaryAnim: "XBW", blockAnim: "1HS" },
    { token: "wand", name: "Necromancer Wands", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "pclb", name: "Paladin Clubs", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "pmac", name: "Paladin Maces", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "pham", name: "Paladin Hammers", primaryAnim: "STF", blockAnim: "1HS" },
    { token: "pspe", name: "Paladin Spears", primaryAnim: "2HT", blockAnim: "1HS" },
    { token: "orb", name: "Sorceress Orbs", primaryAnim: "1HS", blockAnim: "1HS" },
    { token: "scrd", name: "Sorceress Crystal Swords", primaryAnim: "1HS", blockAnim: "1HS" },
];

// Wereform morphs and Rogue merc force HTH weaponAnim for every anim type.
// `castPrefix` overrides the COF prefix used for cast (normally "SC").
export interface CharOverride {
    allAnims: string;
    isWereform: boolean;
    castPrefix?: string;
}

export const CHAR_OVERRIDES: Record<string, CharOverride> = {
    "40": { allAnims: "HTH", isWereform: true },
    "TG": { allAnims: "HTH", isWereform: true },
    "OW": { allAnims: "HTH", isWereform: true },
    "~Z": { allAnims: "HTH", isWereform: true },
    "0N": { allAnims: "HTH", isWereform: true, castPrefix: "A1" },
    "TH": { allAnims: "HTH", isWereform: true, castPrefix: "A1" },
    "RG": { allAnims: "HTH", isWereform: false },
};

const CLASS_SPECIFIC_TOKENS: Record<string, string[]> = {
    AM: ["abow", "aspe", "ajav"],
    AI: ["h2h", "nagi"],
    BA: ["bswd", "baxe", "2hbx"],
    DZ: ["dbow", "dstf"],
    NE: ["nscy", "nstf", "nknf", "nxbw", "wand"],
    PA: ["pclb", "pmac", "pham", "pspe"],
    SO: ["orb", "scrd"],
};

// MXL mercs can wield class-specific weapons (Rogue ↔ Amazon Bows etc.).
// Town Guard and Shapeshifter share token "GU" and the same allowed list.
const MERC_WEAPON_TOKENS: Record<string, string[]> = {
    RG: ["bow", "abow"],
    GU: ["jave", "spea", "scyh", "pspe"],
    IW: ["swor", "crsd"],
    "0A": ["swor", "crsd", "2hsd", "bswd"],
};

const ALL_CLASS_SPECIFIC = new Set(Object.values(CLASS_SPECIFIC_TOKENS).flat());

const GENERIC_WEAPON_TOKENS = WEAPON_TYPES
    .filter((wt) => !ALL_CLASS_SPECIFIC.has(wt.token))
    .map((wt) => wt.token);

export const WEAPON_TYPE_MAP = new Map(WEAPON_TYPES.map((wt) => [wt.token, wt]));

export function getWeaponTypesForCharacter(charToken: string): WeaponType[] {
    const mercTokens = MERC_WEAPON_TOKENS[charToken];
    if (mercTokens) {
        const allowed = new Set(mercTokens);
        return WEAPON_TYPES.filter((wt) => allowed.has(wt.token));
    }
    const classSpecific = CLASS_SPECIFIC_TOKENS[charToken] ?? [];
    const allowed = new Set([...GENERIC_WEAPON_TOKENS, ...classSpecific]);
    return WEAPON_TYPES.filter((wt) => allowed.has(wt.token));
}

// Fallback for findWeaponTypeByWclass when the live family chain misses
// every WEAPON_TYPES token: pick the class-preferred weapon for the
// (class, primaryAnim) pair.
const CLASS_PREFERRED_ANIM: Record<string, Record<string, string>> = {
    AM: { BOW: "abow", "2HT": "aspe", "1HT": "ajav" },
    AI: { HT1: "h2h", STF: "nagi" },
    BA: { "1HS": "bswd" },
    DZ: { BOW: "dbow", STF: "dstf" },
    NE: { "1HT": "nknf", STF: "nscy", XBW: "nxbw" },
    PA: { "1HS": "pclb", "2HT": "pspe", STF: "pham" },
    SO: { "1HS": "scrd" },
};

export function findWeaponTypeByWclass(
    charToken: string,
    wclass: string,
    familyCodes?: string[],
): WeaponType | null {
    if (familyCodes && familyCodes.length > 0) {
        for (const code of familyCodes) {
            const wt = WEAPON_TYPE_MAP.get(code.toLowerCase());
            if (wt) return wt;
        }
    }
    const anim = wclass.toUpperCase();
    const preferred = CLASS_PREFERRED_ANIM[charToken]?.[anim];
    if (preferred) {
        const wt = WEAPON_TYPE_MAP.get(preferred);
        if (wt) return wt;
    }
    const available = getWeaponTypesForCharacter(charToken);
    return available.find((wt) => wt.primaryAnim === anim) ?? null;
}

export const STARTING_FRAME_CLASSES = new Set(["AM", "SO"]);
export const STARTING_FRAME_ANIMS = new Set(["1HS", "1HT", "2HS", "2HT", "STF"]);

export const THROWING_FAMILIES = new Set(["tkni", "jave", "ajav", "taxe"]);

// Mercs and wereforms can't throw.
export const THROWING_DISALLOWED_TOKENS = new Set([
    "RG", "GU", "IW", "0A",
    "40", "TG", "OW", "~Z", "0N", "TH",
]);

// Families a Barb can't dual-wield (2H grip / ranged / caster off-hand).
// Everything else — including normally-2H melee like Two-Handed Swords or
// Throwing Axes — is fair game.
export const BARB_DW_EXCLUDED_FAMILIES = new Set([
    "spea", "aspe", "pspe",
    "scyh", "nscy",
    "staf", "dstf", "nstf",
    "bow", "abow", "dbow",
    "xbow", "nxbw",
    "nagi",
    "wand", "orb",
]);

// Per-class weaponAnim overrides for SC/GH/BL when DW is active. Looked up
// via `{charToken}{animType}{weaponAnim}` (e.g. BAGH1SS, AISCHT2, AIBLHT2).
// `null` = suppress that anim type (Barb BL — can't block while DW). Missing
// key = anim unchanged; A1 always stays on the base primaryAnim because the
// speedcalc JS comments "Double attack animations are currently not in use".
export const DW_WEAPON_ANIMS: Record<string, Partial<Record<AnimType, string | null>>> = {
    BA: { SC: "1SS", GH: "1SS", BL: null },
    AI: { SC: "HT2", GH: "HT2", BL: "HT2" },
};

// Maul-class hammers (WSM == 10) swing as 1HS, not STF, for A1/SC/GH.
export function effectiveWeaponAnimForBase(
    weapon: WeaponType,
    wsm: number,
    animType: AnimType,
): string {
    if (weapon.token === "hamm" && wsm === 10) {
        if (animType === "BL") return weapon.blockAnim;
        return "1HS";
    }
    if (animType === "BL") return weapon.blockAnim;
    return weapon.primaryAnim;
}
