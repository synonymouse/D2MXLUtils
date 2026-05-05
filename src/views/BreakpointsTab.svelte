<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import { listen, type UnlistenFn } from "@tauri-apps/api/event";
    import { onMount } from "svelte";
    import {
        CLASSES,
        MORPHS,
        MERCS,
        DEBUFFS,
        ANIM_TYPES,
        ANIM_TYPE_LABELS,
        WEAPON_TYPE_MAP,
        BARB_DW_EXCLUDED_FAMILIES,
        THROWING_FAMILIES,
        THROWING_DISALLOWED_TOKENS,
        effectiveWeaponAnimForBase,
        getWeaponTypesForCharacter,
        findWeaponTypeByWclass,
        type AnimType,
        type WeaponType,
    } from "../lib/breakpoint-constants";
    import {
        computeBreakpointTable,
        type BreakpointTable,
        type CalcParams,
        type SpeedcalcTable,
    } from "../lib/breakpoint-calc";

    interface BreakpointData {
        class: number;
        wclass: string;
        wsm: number;
        file_index: number;
        family_codes: string[];
        ias: number;
        fcr: number;
        fhr: number;
        fbr: number;
        skill_ias: number;
        skill_fhr: number;
        merc_type: number | null;
    }

    interface BreakpointsPayload {
        player: BreakpointData | null;
        merc: BreakpointData | null;
    }

    interface WeaponBase {
        file_index: number;
        name: string;
        wclass: string;
        wsm: number;
        family_codes: string[];
    }

    let speedcalcTable = $state<SpeedcalcTable | null>(null);
    let weaponBaseCatalog = $state<WeaponBase[]>([]);
    let livePlayer = $state<BreakpointData | null>(null);
    let liveMerc = $state<BreakpointData | null>(null);
    let loadError = $state<string | null>(null);
    let activeEntity = $state<"player" | "merc">("player");

    // Player overrides
    let overrideClass = $state<number | null>(null);
    let overrideMorph = $state<string | null>(null);
    let overrideWeaponType = $state<string | null>(null);
    let overrideBaseFileIndex = $state<number | null>(null);
    let overrideIas = $state<string>("");
    let overrideFcr = $state<string>("");
    let overrideFhr = $state<string>("");
    let overrideFbr = $state<string>("");
    let debuffIndex = $state(0);
    let isDualWielding = $state(false);
    let isThrowing = $state(false);

    // Merc overrides
    let overrideMercType = $state<number | null>(null);
    let overrideMercWeaponType = $state<string | null>(null);
    let overrideMercBaseFileIndex = $state<number | null>(null);
    let overrideMercIas = $state<string>("");
    let overrideMercFcr = $state<string>("");
    let overrideMercFhr = $state<string>("");
    let overrideMercFbr = $state<string>("");
    let mercDebuffIndex = $state(0);

    // MXL tier suffixes — tiered copies of a base share WSM, collapse them.
    const TIER_SUFFIX_RE = /\s*\((?:[1-4]|Sacred|Angelic|Mastercrafted)\)\s*$/i;
    function stripTier(name: string): string {
        return name.replace(TIER_SUFFIX_RE, "").trim();
    }

    let basesByFamily = $derived.by((): Map<string, WeaponBase[]> => {
        const map = new Map<string, WeaponBase[]>();
        if (weaponBaseCatalog.length === 0) return map;
        for (const base of weaponBaseCatalog) {
            let familyToken: string | null = null;
            for (const code of base.family_codes) {
                const wt = WEAPON_TYPE_MAP.get(code.toLowerCase());
                if (wt) { familyToken = wt.token; break; }
            }
            if (!familyToken) continue;
            let bucket = map.get(familyToken);
            if (!bucket) { bucket = []; map.set(familyToken, bucket); }
            bucket.push(base);
        }
        for (const [token, bucket] of map.entries()) {
            const seen = new Map<string, WeaponBase>();
            for (const b of bucket) {
                const display = stripTier(b.name);
                if (!display) continue;
                if (!seen.has(display)) {
                    seen.set(display, { ...b, name: display });
                }
            }
            const dedup = Array.from(seen.values());
            dedup.sort((a, b) => a.wsm - b.wsm);
            map.set(token, dedup);
        }
        return map;
    });

    let charToken = $derived.by((): string => {
        if (activeEntity === "merc") {
            const mercId = overrideMercType ?? liveMerc?.merc_type ?? 0;
            return MERCS[mercId]?.token ?? "RG";
        }
        if (overrideMorph) return overrideMorph;
        const classId = overrideClass ?? livePlayer?.class ?? 0;
        return CLASSES[classId]?.token ?? "AM";
    });

    let availableWeapons = $derived(getWeaponTypesForCharacter(charToken));

    // Priority: manual override > auto-detected from live > first available.
    let effectiveWeaponToken = $derived.by((): string => {
        const manual = activeEntity === "player" ? overrideWeaponType : overrideMercWeaponType;
        if (manual) return manual;
        const live = activeEntity === "player" ? livePlayer : liveMerc;
        if (live && (live.wclass || (live.family_codes && live.family_codes.length > 0))) {
            const detected = findWeaponTypeByWclass(charToken, live.wclass ?? "", live.family_codes);
            if (detected) return detected.token;
        }
        return availableWeapons[0]?.token ?? "";
    });

    let effectiveWeapon = $derived.by((): WeaponType | null => {
        return availableWeapons.find((w) => w.token === effectiveWeaponToken) ?? null;
    });

    let availableBases = $derived.by((): WeaponBase[] => {
        if (!effectiveWeapon) return [];
        return basesByFamily.get(effectiveWeapon.token) ?? [];
    });

    // Live `file_index` may point at any tier variant; availableBases keeps
    // one stripped representative per family, so we match through the
    // catalog by stripped name.
    let effectiveBase = $derived.by((): WeaponBase | null => {
        if (availableBases.length === 0) return null;
        const manual = activeEntity === "player" ? overrideBaseFileIndex : overrideMercBaseFileIndex;
        if (manual !== null) {
            const found = availableBases.find((b) => b.file_index === manual);
            if (found) return found;
        }
        const live = activeEntity === "player" ? livePlayer : liveMerc;
        if (live?.file_index) {
            const liveBase = weaponBaseCatalog.find((b) => b.file_index === live.file_index);
            if (liveBase) {
                const display = stripTier(liveBase.name);
                const found = availableBases.find((b) => b.name === display);
                if (found) return found;
            }
        }
        return availableBases[0];
    });

    let canDualWield = $derived.by((): boolean => {
        if (activeEntity !== "player") return false;
        if (!effectiveWeapon) return false;
        if (charToken === "AI") return effectiveWeapon.token === "h2h";
        if (charToken === "BA") {
            return !BARB_DW_EXCLUDED_FAMILIES.has(effectiveWeapon.token);
        }
        return false;
    });

    let canThrow = $derived.by((): boolean => {
        if (activeEntity !== "player") return false;
        if (!effectiveWeapon) return false;
        if (!THROWING_FAMILIES.has(effectiveWeapon.token)) return false;
        if (THROWING_DISALLOWED_TOKENS.has(charToken)) return false;
        return true;
    });

    // Drop the flag if the user switches weapon family / class so a hidden
    // checkbox can't keep its old state alive in the calc.
    $effect(() => {
        if (!canDualWield && isDualWielding) isDualWielding = false;
    });
    $effect(() => {
        if (!canThrow && isThrowing) isThrowing = false;
    });

    let calcParams = $derived.by((): CalcParams | null => {
        if (!speedcalcTable) return null;
        const weapon = effectiveWeapon;
        if (!weapon) return null;

        const live = activeEntity === "player" ? livePlayer : liveMerc;
        const oIas = activeEntity === "player" ? overrideIas : overrideMercIas;
        const oFcr = activeEntity === "player" ? overrideFcr : overrideMercFcr;
        const oFhr = activeEntity === "player" ? overrideFhr : overrideMercFhr;
        const oFbr = activeEntity === "player" ? overrideFbr : overrideMercFbr;
        const dIdx = activeEntity === "player" ? debuffIndex : mercDebuffIndex;

        const wsm = effectiveBase?.wsm ?? live?.wsm ?? 0;
        const primaryAnim = effectiveWeaponAnimForBase(weapon, wsm, "A1");

        return {
            charToken,
            primaryAnim,
            blockAnim: weapon.blockAnim,
            wsm,
            ias: oIas !== "" ? parseInt(oIas) || 0 : live?.ias ?? 0,
            fcr: oFcr !== "" ? parseInt(oFcr) || 0 : live?.fcr ?? 0,
            fhr: oFhr !== "" ? parseInt(oFhr) || 0 : live?.fhr ?? 0,
            fbr: oFbr !== "" ? parseInt(oFbr) || 0 : live?.fbr ?? 0,
            skillIas: live?.skill_ias ?? 0,
            skillFhr: live?.skill_fhr ?? 0,
            debuff: DEBUFFS[dIdx]?.value ?? 0,
            isDualWielding: activeEntity === "player" ? isDualWielding : false,
            isThrowing: activeEntity === "player" ? isThrowing : false,
        };
    });

    let tables = $derived.by((): BreakpointTable[] => {
        if (!speedcalcTable || !calcParams) return [];
        const result: BreakpointTable[] = [];
        for (const animType of ANIM_TYPES) {
            const table = computeBreakpointTable(
                speedcalcTable,
                calcParams,
                animType as AnimType,
            );
            if (table && table.entries.length > 0) {
                result.push(table);
            }
        }
        return result;
    });

    let displayClass = $derived.by(() => {
        if (activeEntity === "merc") {
            const mercId = overrideMercType ?? liveMerc?.merc_type ?? 0;
            return MERCS[mercId]?.name ?? "Unknown";
        }
        if (overrideMorph) {
            return MORPHS.find((m) => m.token === overrideMorph)?.name ?? "Unknown";
        }
        const classId = overrideClass ?? livePlayer?.class ?? 0;
        return CLASSES[classId]?.name ?? "Unknown";
    });

    function setWeaponTypeOverride(token: string) {
        if (activeEntity === "player") {
            overrideWeaponType = token;
            overrideBaseFileIndex = null;
        } else {
            overrideMercWeaponType = token;
            overrideMercBaseFileIndex = null;
        }
    }

    function setBaseOverride(fileIndex: number) {
        if (activeEntity === "player") overrideBaseFileIndex = fileIndex;
        else overrideMercBaseFileIndex = fileIndex;
    }

    onMount(() => {
        const unlisteners: UnlistenFn[] = [];

        invoke("set_breakpoints_polling", { enabled: true });

        invoke<SpeedcalcTable | null>("get_speedcalc_data").then((data) => {
            if (data && Object.keys(data).length > 0) {
                speedcalcTable = data;
            } else {
                invoke("refresh_speedcalc_data")
                    .then(() => invoke<SpeedcalcTable | null>("get_speedcalc_data"))
                    .then((freshData) => {
                        if (freshData && Object.keys(freshData).length > 0) {
                            speedcalcTable = freshData;
                        } else {
                            loadError = "Failed to load breakpoint data";
                        }
                    })
                    .catch((e) => {
                        loadError = `Failed to fetch breakpoint data: ${e}`;
                    });
            }
        });

        invoke<WeaponBase[] | null>("get_weapon_base_catalog").then((data) => {
            if (data && data.length > 0) weaponBaseCatalog = data;
        });

        listen<BreakpointsPayload>("breakpoints-update", (event) => {
            livePlayer = event.payload.player;
            liveMerc = event.payload.merc;
        }).then((u) => unlisteners.push(u));

        listen<WeaponBase[]>("weapon-base-catalog-updated", (event) => {
            if (event.payload && event.payload.length > 0) weaponBaseCatalog = event.payload;
        }).then((u) => unlisteners.push(u));

        return () => {
            invoke("set_breakpoints_polling", { enabled: false });
            unlisteners.forEach((u) => u());
        };
    });
</script>

<div class="breakpoints-tab">
    {#if loadError}
        <div class="error-banner">{loadError}</div>
    {/if}

    <div class="entity-toggle">
        <button
            class="entity-btn"
            class:active={activeEntity === "player"}
            onclick={() => { activeEntity = "player"; }}
        >
            Player
        </button>
        <button
            class="entity-btn"
            class:active={activeEntity === "merc"}
            onclick={() => { activeEntity = "merc"; }}
        >
            Mercenary
        </button>
    </div>

    <div class="controls">
        <div class="control-row">
            {#if activeEntity === "player"}
                <label>
                    <span class="label">Class</span>
                    <select
                        value={overrideClass ?? livePlayer?.class ?? 0}
                        onchange={(e) => { overrideClass = parseInt(e.currentTarget.value); }}
                    >
                        {#each CLASSES as cls, i}
                            <option value={i}>{cls.name}</option>
                        {/each}
                    </select>
                </label>

                <label>
                    <span class="label">Morph</span>
                    <select
                        value={overrideMorph ?? ""}
                        onchange={(e) => { overrideMorph = e.currentTarget.value || null; }}
                    >
                        <option value="">None</option>
                        {#each MORPHS as morph}
                            <option value={morph.token}>{morph.name}</option>
                        {/each}
                    </select>
                </label>
            {:else}
                <label>
                    <span class="label">Mercenary</span>
                    <select
                        value={overrideMercType ?? liveMerc?.merc_type ?? 0}
                        onchange={(e) => { overrideMercType = parseInt(e.currentTarget.value); }}
                    >
                        {#each MERCS as merc}
                            <option value={merc.id}>{merc.name}</option>
                        {/each}
                    </select>
                </label>
            {/if}

            <label>
                <span class="label">Weapon Type</span>
                <select
                    value={effectiveWeaponToken}
                    onchange={(e) => setWeaponTypeOverride(e.currentTarget.value)}
                >
                    {#each availableWeapons as wt}
                        <option value={wt.token}>{wt.name}</option>
                    {/each}
                </select>
            </label>

            {#if availableBases.length > 0}
                <label>
                    <span class="label">Weapon Base</span>
                    <select
                        value={effectiveBase?.file_index ?? -1}
                        onchange={(e) => setBaseOverride(parseInt(e.currentTarget.value))}
                    >
                        {#each availableBases as base (base.file_index)}
                            <option value={base.file_index}>{base.name}</option>
                        {/each}
                    </select>
                </label>
            {/if}

            <label>
                <span class="label">Debuff</span>
                <select
                    value={activeEntity === "player" ? debuffIndex : mercDebuffIndex}
                    onchange={(e) => {
                        const val = parseInt(e.currentTarget.value);
                        if (activeEntity === "player") {
                            debuffIndex = val;
                        } else {
                            mercDebuffIndex = val;
                        }
                    }}
                >
                    {#each DEBUFFS as debuff, i}
                        <option value={i}>{debuff.name} ({debuff.value})</option>
                    {/each}
                </select>
            </label>
        </div>

        <div class="control-row">
            <label>
                <span class="label">IAS</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.ias : liveMerc?.ias)?.toString() ?? "0"}
                    value={activeEntity === "player" ? overrideIas : overrideMercIas}
                    oninput={(e) => {
                        if (activeEntity === "player") overrideIas = e.currentTarget.value;
                        else overrideMercIas = e.currentTarget.value;
                    }}
                />
            </label>
            <label>
                <span class="label">FCR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fcr : liveMerc?.fcr)?.toString() ?? "0"}
                    value={activeEntity === "player" ? overrideFcr : overrideMercFcr}
                    oninput={(e) => {
                        if (activeEntity === "player") overrideFcr = e.currentTarget.value;
                        else overrideMercFcr = e.currentTarget.value;
                    }}
                />
            </label>
            <label>
                <span class="label">FHR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fhr : liveMerc?.fhr)?.toString() ?? "0"}
                    value={activeEntity === "player" ? overrideFhr : overrideMercFhr}
                    oninput={(e) => {
                        if (activeEntity === "player") overrideFhr = e.currentTarget.value;
                        else overrideMercFhr = e.currentTarget.value;
                    }}
                />
            </label>
            <label>
                <span class="label">FBR</span>
                <input
                    type="number"
                    placeholder={(activeEntity === "player" ? livePlayer?.fbr : liveMerc?.fbr)?.toString() ?? "0"}
                    value={activeEntity === "player" ? overrideFbr : overrideMercFbr}
                    oninput={(e) => {
                        if (activeEntity === "player") overrideFbr = e.currentTarget.value;
                        else overrideMercFbr = e.currentTarget.value;
                    }}
                />
            </label>

            {#if canDualWield}
                <label class="checkbox-label">
                    <input
                        type="checkbox"
                        checked={isDualWielding}
                        onchange={(e) => { isDualWielding = e.currentTarget.checked; }}
                    />
                    <span class="label">Dual Wielding</span>
                </label>
            {/if}

            {#if canThrow}
                <label class="checkbox-label">
                    <input
                        type="checkbox"
                        checked={isThrowing}
                        onchange={(e) => { isThrowing = e.currentTarget.checked; }}
                    />
                    <span class="label">Throwing</span>
                </label>
            {/if}
        </div>
    </div>

    <div class="current-status">
        <span class="status-class">{displayClass}</span>
        {#if effectiveWeapon}
            <span class="status-detail">
                {effectiveWeapon.name}{#if effectiveBase} — {effectiveBase.name}{/if}
            </span>
        {/if}
    </div>

    <div class="tables-container">
        {#each tables as bpTable (bpTable.animType)}
            <div class="bp-table">
                <h3 class="bp-title">
                    {ANIM_TYPE_LABELS[bpTable.animType]}
                    <span class="bp-current">Current: {bpTable.currentFpa} FPA</span>
                    {#if bpTable.delta !== null}
                        <span class="bp-delta">+{bpTable.delta} to next</span>
                    {/if}
                </h3>
                <table>
                    <thead>
                        <tr>
                            <th>FPA</th>
                            <th>Required</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each bpTable.entries as entry (entry.fpa)}
                            <tr class:current={entry.fpa === bpTable.currentFpa} class:next={entry === bpTable.nextBreakpoint}>
                                <td>{entry.fpa}</td>
                                <td>{entry.requiredStat}</td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {:else}
            {#if speedcalcTable}
                <p class="no-data">No breakpoint data available for this combination.</p>
            {:else if !loadError}
                <p class="no-data">Loading breakpoint data...</p>
            {/if}
        {/each}
    </div>
</div>

<style>
    .breakpoints-tab {
        display: flex;
        flex-direction: column;
        gap: var(--space-3);
        height: 100%;
        overflow-y: auto;
    }

    .error-banner {
        padding: var(--space-2) var(--space-3);
        background: var(--status-error-bg);
        color: var(--status-error-text);
        border-radius: var(--radius-sm);
        font-size: var(--text-sm);
    }

    .entity-toggle {
        display: flex;
        gap: var(--space-1);
    }

    .entity-btn {
        padding: var(--space-1) var(--space-3);
        border: 1px solid var(--border-primary);
        background: var(--bg-secondary);
        color: var(--text-secondary);
        border-radius: var(--radius-sm);
        cursor: pointer;
        font-size: var(--text-sm);
    }

    .entity-btn.active {
        background: var(--accent-primary);
        color: var(--accent-text);
        border-color: var(--accent-primary);
    }

    .controls {
        display: flex;
        flex-direction: column;
        gap: var(--space-2);
        padding: var(--space-2) var(--space-3);
        background: var(--bg-secondary);
        border-radius: var(--radius-md);
        border: 1px solid var(--border-primary);
    }

    .control-row {
        display: flex;
        gap: var(--space-3);
        flex-wrap: wrap;
        align-items: end;
    }

    .control-row label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        font-size: var(--text-sm);
    }

    .control-row .label {
        font-size: var(--text-xs);
        color: var(--text-muted);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }

    .control-row select,
    .control-row input[type="number"] {
        padding: var(--space-1) var(--space-2);
        background: var(--bg-primary);
        border: 1px solid var(--border-primary);
        border-radius: var(--radius-sm);
        color: var(--text-primary);
        font-size: var(--text-sm);
        font-family: var(--font-mono);
        min-width: 80px;
    }

    .control-row input[type="number"] {
        width: 70px;
    }

    .checkbox-label {
        flex-direction: row !important;
        align-items: center;
        gap: var(--space-1) !important;
        padding-bottom: 6px;
    }

    .checkbox-label input[type="checkbox"] {
        margin: 0;
        cursor: pointer;
    }

    .checkbox-label .label {
        text-transform: none;
        letter-spacing: normal;
        font-size: var(--text-sm);
        color: var(--text-primary);
    }

    .current-status {
        display: flex;
        gap: var(--space-3);
        align-items: center;
        font-size: var(--text-sm);
        color: var(--text-muted);
    }

    .status-class {
        font-weight: 600;
        color: var(--text-primary);
    }

    .status-detail {
        font-family: var(--font-mono);
    }

    .tables-container {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
        gap: var(--space-3);
        flex: 1;
        min-height: 0;
        overflow-y: auto;
    }

    .bp-table {
        background: var(--bg-secondary);
        border: 1px solid var(--border-primary);
        border-radius: var(--radius-md);
        padding: var(--space-2);
    }

    .bp-title {
        font-size: var(--text-sm);
        font-weight: 600;
        color: var(--text-primary);
        margin: 0 0 var(--space-2) 0;
        display: flex;
        align-items: center;
        gap: var(--space-2);
        flex-wrap: wrap;
    }

    .bp-current {
        font-size: var(--text-xs);
        color: var(--accent-primary);
        font-weight: 500;
    }

    .bp-delta {
        font-size: var(--text-xs);
        color: var(--status-warning-text);
        font-weight: 500;
    }

    table {
        width: 100%;
        border-collapse: collapse;
        font-size: var(--text-xs);
        font-family: var(--font-mono);
    }

    th {
        text-align: left;
        padding: var(--space-1);
        color: var(--text-muted);
        border-bottom: 1px solid var(--border-primary);
        font-weight: 500;
    }

    td {
        padding: var(--space-1);
        color: var(--text-secondary);
    }

    tr.current {
        background: var(--accent-primary-subtle, rgba(99, 102, 241, 0.1));
    }

    tr.current td {
        color: var(--accent-primary);
        font-weight: 600;
    }

    tr.next td {
        color: var(--status-warning-text);
    }

    .no-data {
        color: var(--text-muted);
        font-size: var(--text-sm);
        text-align: center;
        padding: var(--space-4);
    }
</style>
