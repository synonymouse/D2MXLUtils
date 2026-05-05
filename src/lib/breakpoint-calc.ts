import {
    CHAR_OVERRIDES,
    DW_WEAPON_ANIMS,
    STARTING_FRAME_CLASSES,
    STARTING_FRAME_ANIMS,
    type AnimType,
} from "./breakpoint-constants";

export interface AnimData {
    frames: number;
    animSpeed: number;
}

export interface BreakpointEntry {
    fpa: number;
    requiredStat: number;
}

export interface BreakpointTable {
    animType: AnimType;
    entries: BreakpointEntry[];
    currentFpa: number;
    currentStat: number;
    nextBreakpoint: BreakpointEntry | null;
    delta: number | null;
}

export type SpeedcalcTable = Record<string, { frames: number; anim_speed: number }>;

function lookupAnim(
    table: SpeedcalcTable,
    charToken: string,
    animPrefix: string,
    weaponAnim: string,
): AnimData | null {
    const key = `${charToken}${animPrefix}${weaponAnim}`;
    const entry = table[key];
    if (!entry) return null;
    return { frames: entry.frames, animSpeed: entry.anim_speed };
}

function resolveAnimLookup(
    table: SpeedcalcTable,
    charToken: string,
    animType: AnimType,
    primaryAnim: string,
    blockAnim: string,
    isDualWielding: boolean,
    isThrowing: boolean,
): { animPrefix: string; weaponAnim: string } | null {
    const override = CHAR_OVERRIDES[charToken];

    let weaponAnim: string;
    if (override) {
        weaponAnim = override.allAnims;
    } else if (animType === "BL") {
        weaponAnim = blockAnim;
    } else {
        weaponAnim = primaryAnim;
    }

    if (isDualWielding && !override) {
        const dwTable = DW_WEAPON_ANIMS[charToken];
        if (dwTable) {
            const dwAnim = dwTable[animType];
            if (dwAnim === null) return null;
            if (dwAnim) weaponAnim = dwAnim;
        }
    }

    let animPrefix: string = animType;
    if (override?.castPrefix && animType === "SC") {
        animPrefix = override.castPrefix;
    }

    if (isThrowing && animType === "A1" && !override) {
        if (table[`${charToken}TH${weaponAnim}`]) {
            animPrefix = "TH";
        }
    }

    return { animPrefix, weaponAnim };
}

function calcAttackFpa(
    frames: number,
    animSpeed: number,
    ias: number,
    wsm: number,
    skillSlow: number,
    hasStartingFrame: boolean,
    throwingPenalty: number,
): number {
    const effectiveFrames = hasStartingFrame ? frames - 2 : frames;
    const eIAS = Math.floor((120 * ias) / (120 + ias));
    const effective = Math.min(eIAS - wsm + skillSlow, 75) - throwingPenalty;
    const divisor = Math.floor((animSpeed * (100 + effective)) / 100);
    if (divisor <= 0) return effectiveFrames;
    return Math.ceil((256 * effectiveFrames) / divisor) - 1;
}

function calcCastFpa(
    frames: number,
    animSpeed: number,
    fcr: number,
    skillSlow: number,
): number {
    const eFCR = Math.min(Math.floor((120 * fcr) / (120 + fcr)) + skillSlow, 75);
    const divisor = Math.floor((animSpeed * (100 + eFCR)) / 100);
    if (divisor <= 0) return frames;
    return Math.ceil((256 * frames) / divisor) - 1;
}

function calcDefensiveFpa(
    frames: number,
    animSpeed: number,
    stat: number,
    skillSlow: number,
): number {
    const eStat = Math.floor((120 * stat) / (120 + stat));
    const divisor = Math.floor((animSpeed * (50 + eStat + skillSlow)) / 100);
    if (divisor <= 0) return frames;
    return Math.ceil((256 * frames) / divisor) - 1;
}

function calcWereformAttackFpa(
    table: SpeedcalcTable,
    charToken: string,
    ias: number,
    wsm: number,
    skillSlow: number,
): number | null {
    const nuEntry = lookupAnim(table, charToken, "NU", "HTH");
    const a1Entry = lookupAnim(table, charToken, "A1", "HTH");
    if (!nuEntry || !a1Entry) return null;

    const eIAS = Math.floor((120 * ias) / (120 + ias));
    const inner = Math.floor(((100 + eIAS - wsm) * a1Entry.animSpeed) / 100);
    if (inner <= 0) return null;
    const wAnimSpeed = Math.floor(
        (256 * nuEntry.frames) / Math.floor((256 * a1Entry.frames) / inner),
    );

    const effective = Math.min(eIAS - wsm + skillSlow, 75);
    const divisor = Math.floor((wAnimSpeed * (100 + effective)) / 100);
    if (divisor <= 0) return null;
    return Math.ceil((256 * a1Entry.frames) / divisor) - 1;
}

function findRequiredStat(
    targetFpa: number,
    calcFn: (stat: number) => number,
): number {
    let lo = 0;
    let hi = 500;
    const baseFpa = calcFn(0);
    if (baseFpa <= targetFpa) return 0;
    if (calcFn(hi) > targetFpa) return -1;

    while (lo < hi) {
        const mid = Math.floor((lo + hi) / 2);
        if (calcFn(mid) <= targetFpa) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    return lo;
}

export interface CalcParams {
    charToken: string;
    primaryAnim: string;
    blockAnim: string;
    wsm: number;
    ias: number;
    fcr: number;
    fhr: number;
    fbr: number;
    skillIas: number;
    skillFhr: number;
    debuff: number;
    isDualWielding: boolean;
    isThrowing: boolean;
}

export function computeBreakpointTable(
    table: SpeedcalcTable,
    params: CalcParams,
    animType: AnimType,
): BreakpointTable | null {
    const resolved = resolveAnimLookup(
        table,
        params.charToken,
        animType,
        params.primaryAnim,
        params.blockAnim,
        params.isDualWielding,
        params.isThrowing,
    );
    if (!resolved) return null;
    const { animPrefix, weaponAnim } = resolved;

    const anim = lookupAnim(table, params.charToken, animPrefix, weaponAnim);
    if (!anim) return null;

    const override = CHAR_OVERRIDES[params.charToken];
    const skillSlow = params.debuff;
    const hasStartingFrame =
        animType === "A1" &&
        !override &&
        !params.isThrowing &&
        STARTING_FRAME_CLASSES.has(params.charToken) &&
        STARTING_FRAME_ANIMS.has(params.primaryAnim);
    // -30 penalty when throwing falls back to A1 because no TH<anim> entry
    // exists for this char/weapon combo (matches speedcalc JS).
    const throwingPenalty =
        animType === "A1" && params.isThrowing && animPrefix !== "TH" ? 30 : 0;

    let currentStat: number;
    let calcFn: (stat: number) => number;

    switch (animType) {
        case "A1":
            currentStat = params.ias;
            if (override?.isWereform) {
                const charToken = params.charToken;
                calcFn = (s) =>
                    calcWereformAttackFpa(
                        table,
                        charToken,
                        s,
                        params.wsm,
                        skillSlow,
                    ) ?? anim.frames;
            } else {
                calcFn = (s) =>
                    calcAttackFpa(
                        anim.frames,
                        anim.animSpeed,
                        s,
                        params.wsm,
                        skillSlow,
                        hasStartingFrame,
                        throwingPenalty,
                    );
            }
            break;
        case "SC":
            currentStat = params.fcr;
            calcFn = (s) => calcCastFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
        case "GH":
            currentStat = params.fhr;
            calcFn = (s) => calcDefensiveFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
        case "BL":
            currentStat = params.fbr;
            calcFn = (s) => calcDefensiveFpa(anim.frames, anim.animSpeed, s, skillSlow);
            break;
    }

    const entries: BreakpointEntry[] = [];
    const maxFpa = calcFn(0);
    const minFpa = calcFn(500);
    let prevFpa = -1;

    for (let fpa = maxFpa; fpa >= minFpa; fpa--) {
        const required = findRequiredStat(fpa, calcFn);
        if (required < 0) continue;
        if (fpa === prevFpa) continue;
        const actualFpa = calcFn(required);
        if (actualFpa !== fpa) continue;
        if (entries.length > 0 && entries[entries.length - 1].requiredStat === required) continue;
        entries.push({ fpa, requiredStat: required });
        prevFpa = fpa;
    }

    const currentFpa = calcFn(currentStat);
    const nextBreakpoint =
        entries.find((e) => e.fpa < currentFpa && e.requiredStat > currentStat) ?? null;
    const delta = nextBreakpoint ? nextBreakpoint.requiredStat - currentStat : null;

    return {
        animType,
        entries,
        currentFpa,
        currentStat,
        nextBreakpoint,
        delta,
    };
}
