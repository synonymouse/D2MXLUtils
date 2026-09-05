<script lang="ts">
  import { onMount } from 'svelte';
  import { CLASSES } from '../lib/breakpoint-constants';
  import { statsStore, type UnitStats } from '../stores';

  let activeEntity = $state<'player' | 'merc'>('player');

  let active = $derived(activeEntity === 'player' ? statsStore.player : statsStore.merc);
  // The first `stats-update` payload can lag behind the tab opening (or the
  // player loading into game) by a poll cycle or more — each tick reads ~100
  // stat ids one-by-one via the injector, twice (player + merc). Rather than
  // show a blank pane until that first payload lands, render the section/row
  // *shells* as soon as we know a game is running and fill them with a
  // skeleton placeholder instead of leaving the whole tab empty. `statsStore`
  // lives outside this component, so switching tabs away and back re-shows
  // its cached last-known data instantly instead of hitting this state again.
  let awaitingFirstData = $derived(
    statsStore.gameStatus === 'ingame' && !active && !statsStore.receivedFirstPayload,
  );

  // Life-per-Vitality / Mana-per-Energy growth factors, indexed by class id
  // (0=Amazon..6=Assassin, matches `CLASSES` in breakpoint-constants.ts).
  // These differ from vanilla D2's per-class values, so don't fall back to
  // vanilla numbers here. Both arrays confirmed against the official Median
  // XL class docs (docs.median-xl.com/doc/class/<class>).
  //
  // Known limitation: wielding Azurewrath multiplies Life-per-Vitality by
  // 0.9 — not modeled here (would need to detect the specific equipped
  // unique), so the "from Vitality" estimate reads ~11% high while it's
  // equipped. Mentally scale the figure by 0.9 in that case.
  const LIFE_PER_VIT = [2.25, 2.25, 1.5, 2.75, 2.75, 2.25, 2.25];
  const MANA_PER_ENE = [2.25, 2.5, 3, 1.5, 1.5, 3, 2.25];

  // Elemental resist cap is 75% by default, extendable via `+max resist`
  // item stats. Physical resist cap is a fixed 50% with no known
  // cap-increasing stat — its value can still overcap above 50 through
  // other means, it just isn't shown with a computed max here.
  const BASE_RESIST_CAP = 75;
  const PHYSICAL_RESIST_CAP = 50;
  // Elemental/poison resists have a hard absolute ceiling of 90% regardless
  // of how much +max-resist bonus is stacked — the displayed max is capped
  // here, with any excess bonus surfaced as "(+X%)" so it's visible without
  // implying it actually raises the real cap.
  const ABSOLUTE_RESIST_CAP = 90;

  function num(stats: Record<string, number>, id: number): number {
    return stats[String(id)] ?? 0;
  }

  /** Splits an aggregate stat into base / flat item-and-skill bonus / percent
   *  item-and-skill bonus, expressed as three terms that sum exactly to the
   *  total (mirrors D2Stats.au3:651's `iTotal/(1+iPercent/100) - iBase` for
   *  the flat portion, then folds any rounding into the percent term's
   *  absolute point value so the displayed numbers always add up). */
  function splitBonus(
    total: number,
    pct: number,
    base: number,
  ): { flat: number; pctPoints: number } {
    const denom = 1 + pct / 100;
    const flat = denom > 0 ? Math.ceil(total / denom - base) : 0;
    const pctPoints = total - base - flat;
    return { flat, pctPoints };
  }

  function attributeBreakdown(u: UnitStats, statId: number, pctId: number): string {
    const base = u.baseStats[String(statId)] ?? 0;
    const total = num(u.stats, statId);
    const pct = num(u.stats, pctId);
    const { flat, pctPoints } = splitBonus(total, pct, base);
    return `${base} + ${flat} flat + ${pctPoints} (${pct}%) = ${total}`;
  }

  /** Unlike attributes (where D2Stats.au3 itself validates that the unit's
   *  own no-item StatList value is a meaningful "base"), Life/Mana are
   *  *derived* stats — the engine most likely keeps them in sync using your
   *  CURRENT total Vitality/Energy (points + skills + items all included)
   *  whenever it changes, so there's no reliable way to isolate a "no-item"
   *  base or a clean flat/percent split for them from what we can read. We
   *  only show what's actually known: the real total (read from the
   *  engine), your %Life/%Mana bonus (which applies to the whole pool,
   *  vitality-derived portion included), and a best-effort estimate of how
   *  much of the total is attributable to Vitality/Energy. */
  function lifeManaBreakdown(
    u: UnitStats,
    statId: number,
    pctId: number,
    vitalId: number,
    perVital: number,
  ): string {
    const total = num(u.stats, statId);
    const pct = num(u.stats, pctId);
    const vitalTotal = num(u.stats, vitalId);
    const fromVital = Math.floor(vitalTotal * perVital);
    const vitalLabel = vitalId === 3 ? 'Vit' : 'Ene';
    return `${total} (+${pct}%, ~${fromVital} from ${vitalLabel})`;
  }

  function experienceLabel(u: UnitStats): string {
    const current = num(u.stats, 13);
    const levelStart = num(u.stats, 905);
    const levelNext = num(u.stats, 906);
    if (levelNext < 0) {
      return `${current.toLocaleString()} (MAX)`;
    }
    const span = levelNext - levelStart;
    const pct = span > 0 ? Math.max(0, Math.min(100, ((current - levelStart) / span) * 100)) : 0;
    return `${current.toLocaleString()} / ${levelNext.toLocaleString()} (${pct.toFixed(1)}%)`;
  }

  function spellFocusCapLabel(u: UnitStats): string {
    // stats[904] is effective Spell Focus, not yet divided by 10 — done
    // here (not in Rust) to keep the fractional percent (105 SF = 10.5%).
    const effectiveSf = num(u.stats, 904);
    const raw = effectiveSf / 10;
    const capped = Math.min(raw, 100);
    return raw > 100
      ? `${capped.toFixed(1)}% (overcap, raw ${raw.toFixed(1)}%)`
      : `${capped.toFixed(1)}%`;
  }

  function resistLabel(
    u: UnitStats,
    currentId: number,
    maxBonusId: number | null,
    capAt90 = false,
  ): string {
    const current = num(u.stats, currentId);
    const rawMax =
      maxBonusId === null ? PHYSICAL_RESIST_CAP : BASE_RESIST_CAP + num(u.stats, maxBonusId);
    if (!capAt90) {
      return `${current}% / max ${rawMax}%`;
    }
    const cappedMax = Math.min(rawMax, ABSOLUTE_RESIST_CAP);
    const overcap = rawMax - ABSOLUTE_RESIST_CAP;
    return overcap > 0
      ? `${current}% / max ${cappedMax}% (+${overcap}%)`
      : `${current}% / max ${cappedMax}%`;
  }

  interface StatRow {
    label: string;
    render: (u: UnitStats) => string;
    tooltip?: string;
    colorVar?: string;
    /** When set, the row only renders if this returns truthy. */
    visible?: (u: UnitStats) => boolean;
  }

  interface StatSection {
    title: string;
    rows: StatRow[];
  }

  const tpl =
    (template: string): ((u: UnitStats) => string) =>
    (u) =>
      template.replace(/\{(\w+)\}/g, (_m, key: string) => {
        switch (key) {
          case '__phys1h':
            return u.damage ? `${u.damage.physMin1h}-${u.damage.physMax1h}` : '0-0';
          case '__phys2h':
            return u.damage ? `${u.damage.physMin2h}-${u.damage.physMax2h}` : '0-0';
          case '__fire':
            return u.damage ? `${u.damage.fireMin}-${u.damage.fireMax}` : '0-0';
          case '__cold':
            return u.damage ? `${u.damage.coldMin}-${u.damage.coldMax}` : '0-0';
          case '__lightning':
            return u.damage ? `${u.damage.lightningMin}-${u.damage.lightningMax}` : '0-0';
          case '__magic':
            return u.damage ? `${u.damage.magicMin}-${u.damage.magicMax}` : '0-0';
          case '__poison':
            return u.damage ? `${u.damage.poisonMinPerSec}-${u.damage.poisonMaxPerSec}` : '0-0';
          case '__strdmg':
            return String(u.damage?.strDamageBonusPct ?? 0);
          case '__dexdmg':
            return String(u.damage?.dexDamageBonusPct ?? 0);
          default:
            return String(num(u.stats, Number(key)));
        }
      });

  // Mirrors D2Stats.au3's `CreateGUI()` stat panels (Basic / Page 1 / Page 2),
  // D2Stats.au3:2510-2636, reorganized per in-house feedback (attribute and
  // life/mana breakdowns, weapon-damage regrouping, spell-damage renaming).
  // Stat ids are vanilla ItemStatCost.txt row indices.
  function buildSections(u: UnitStats): StatSection[] {
    return [
      {
        title: 'Character',
        rows: [
          { label: 'Class', render: () => CLASSES.find((c) => c.id === u.class)?.name ?? '?' },
          { label: 'Level', render: tpl('{12}') },
          {
            label: 'Experience',
            render: experienceLabel,
            tooltip: 'Current / needed for next level (approximate — see tooltip on the tab)',
          },
          { label: 'Gold (carried)', render: tpl('{14}') },
          { label: 'Gold (stash)', render: tpl('{15}') },
          { label: 'Signets of Learning', render: tpl('{185} / 400') },
          // Charms and Max Skill Level are disabled for now, pending
          // further verification of the locally-computed Charms count
          // (stat 356 — the engine's own GetUnitStat was found to badly
          // under-report) and the Max Skill Level row (stat 479).
          { label: 'Magic Find', render: tpl('{80}%') },
          { label: 'Gold Find', render: tpl('{79}%') },
          { label: 'Experience Gain', render: tpl('+{85}%') },
        ],
      },
      {
        title: 'Attributes',
        rows: [
          {
            label: 'Strength',
            render: (u) => attributeBreakdown(u, 0, 359),
            tooltip:
              'points + flat bonus (item/skill) + percent bonus, in points (percent) = total',
          },
          { label: 'Dexterity', render: (u) => attributeBreakdown(u, 2, 360) },
          { label: 'Vitality', render: (u) => attributeBreakdown(u, 3, 362) },
          { label: 'Energy', render: (u) => attributeBreakdown(u, 1, 361) },
          {
            label: 'Life',
            render: (u) => lifeManaBreakdown(u, 7, 76, 3, LIFE_PER_VIT[u.class] ?? 2),
            tooltip:
              'Total is read directly from the game. The %Life bonus applies to your whole life pool, including the Vitality-derived part. The "from Vitality" figure is an estimate (current total Vitality × class factor) — not a precise engine breakdown. Wielding Azurewrath multiplies Life-per-Vitality by 0.9, which this estimate does not account for (reads ~11% high while equipped).',
          },
          {
            label: 'Mana',
            render: (u) => lifeManaBreakdown(u, 9, 77, 1, MANA_PER_ENE[u.class] ?? 2),
            tooltip:
              'Total is read directly from the game. The %Mana bonus applies to your whole mana pool, including the Energy-derived part. The "from Energy" figure is an estimate (current total Energy × class factor) — not a precise engine breakdown.',
          },
        ],
      },
      {
        title: 'Weapon Damage',
        rows: [
          { label: 'Fire', render: tpl('{__fire}'), colorVar: 'var(--stat-fire, #e05d44)' },
          { label: 'Cold', render: tpl('{__cold}'), colorVar: 'var(--stat-cold, #5b9bd5)' },
          {
            label: 'Lightning',
            render: tpl('{__lightning}'),
            colorVar: 'var(--stat-lightning, #d4b106)',
          },
          { label: 'Magic', render: tpl('{__magic}'), colorVar: 'var(--stat-magic, #b366cc)' },
          { label: 'Poison', render: tpl('{__poison}/s'), colorVar: 'var(--stat-poison, #4caf50)' },
          {
            label: 'Innate Elemental Damage',
            render: tpl('+{484}%'),
            tooltip:
              "Bonus % to a weapon base's built-in elemental-from-attribute conversion (elemental bows/claws, etc). Only relevant for weapons with an innate elemental base.",
          },
          {
            label: 'Weapon Physical Damage',
            render: tpl('{25}%'),
            tooltip: 'Enhanced Weapon Damage',
          },
          {
            label: 'Strength Damage Bonus',
            render: tpl('+{__strdmg}%'),
            tooltip:
              'Approximate individual contribution — see Physical rows below for the actual total',
          },
          { label: 'Dexterity Damage Bonus', render: tpl('+{__dexdmg}%') },
          { label: 'Physical (1H)', render: tpl('{__phys1h}') },
          { label: 'Physical (2H/Ranged)', render: tpl('{__phys2h}') },
        ],
      },
      {
        title: 'Combat',
        rows: [
          { label: 'Total Character Defense', render: tpl('{171}%') },
          { label: 'Attack Rating', render: tpl('+{119}% / +{19} flat') },
          { label: 'Physical Damage Reduction', render: tpl('{34}') },
          { label: 'Magic Damage Reduction', render: tpl('{35}') },
          {
            label: 'Grit',
            render: tpl('{184}%'),
            tooltip: 'Damage reduction from all sources, mostly Grit',
          },
          {
            label: 'Dodge',
            render: tpl('{338}%'),
            tooltip: 'Chance to avoid melee attacks while standing still',
          },
          {
            label: 'Avoid',
            render: tpl('{339}%'),
            tooltip: 'Chance to avoid projectiles while standing still',
          },
          {
            label: 'Evade',
            render: tpl('{340}%'),
            tooltip: 'Chance to avoid any attack while moving',
          },
          { label: 'Crushing Blow', render: tpl('{136}%') },
          { label: 'Deadly Strike', render: tpl('{141}%') },
          { label: 'Critical Strike', render: tpl('{344}%') },
        ],
      },
      {
        title: 'Resistances',
        rows: [
          {
            label: 'Fire',
            render: (u) => resistLabel(u, 39, 40, true),
            colorVar: 'var(--stat-fire, #e05d44)',
            tooltip: 'Hard-capped at 90% — any excess +max-resist bonus is shown as (+X%)',
          },
          {
            label: 'Cold',
            render: (u) => resistLabel(u, 43, 44, true),
            colorVar: 'var(--stat-cold, #5b9bd5)',
            tooltip: 'Hard-capped at 90% — any excess +max-resist bonus is shown as (+X%)',
          },
          {
            label: 'Lightning',
            render: (u) => resistLabel(u, 41, 42, true),
            colorVar: 'var(--stat-lightning, #d4b106)',
            tooltip: 'Hard-capped at 90% — any excess +max-resist bonus is shown as (+X%)',
          },
          {
            label: 'Poison',
            render: (u) => resistLabel(u, 45, 46, true),
            colorVar: 'var(--stat-poison, #4caf50)',
            tooltip: 'Hard-capped at 90% — any excess +max-resist bonus is shown as (+X%)',
          },
          {
            label: 'Magic',
            render: (u) => resistLabel(u, 37, 38),
            colorVar: 'var(--stat-magic, #b366cc)',
          },
          {
            label: 'Physical',
            render: (u) => resistLabel(u, 36, null),
            tooltip: 'Fixed 50% cap — no known item stat raises this max',
          },
          { label: 'Curse Length Reduction', render: tpl('{109}%') },
          { label: 'Poison Length Reduction', render: tpl('{110}%') },
        ],
      },
      {
        title: 'Spell Damage',
        rows: [
          {
            label: 'Fire',
            render: tpl('{329}% dmg / {333}% pierce'),
            colorVar: 'var(--stat-fire, #e05d44)',
          },
          {
            label: 'Cold',
            render: tpl('{331}% dmg / {335}% pierce'),
            colorVar: 'var(--stat-cold, #5b9bd5)',
          },
          {
            label: 'Lightning',
            render: tpl('{330}% dmg / {334}% pierce'),
            colorVar: 'var(--stat-lightning, #d4b106)',
          },
          {
            label: 'Poison',
            render: tpl('{332}% dmg / {336}% pierce'),
            colorVar: 'var(--stat-poison, #4caf50)',
          },
          {
            label: 'Poison Skill Duration',
            render: tpl('{431}%'),
            colorVar: 'var(--stat-poison, #4caf50)',
          },
          { label: 'Physical / Magic Pierce', render: tpl('{357}% / 0%') },
          { label: 'Spell Focus (flat)', render: tpl('{485}') },
          {
            label: 'Spell Focus (%)',
            render: tpl('+{488}%'),
            tooltip: 'Bonus % from items/runes, boosts flat Spell Focus multiplicatively',
          },
          {
            label: 'Spell Damage (from SF)',
            render: spellFocusCapLabel,
            tooltip:
              'min(Spell Focus / 10, 100)% — 1000 Spell Focus reaches the 100% cap; anything past that is wasted',
          },
          {
            label: 'Spell Damage (from Energy)',
            render: tpl('+{907}%'),
            tooltip: '130*(Energy+20)/500 + Energy — scales with Energy, no cap',
          },
        ],
      },
      {
        title: 'Speed',
        rows: [
          {
            label: 'Increased Attack Speed',
            render: tpl('{93}% item / {68}% skill'),
            tooltip: 'Item IAS and skill-granted IAS behave differently for breakpoints',
          },
          { label: 'Faster Hit Recovery', render: tpl('{99}% item / {69}% skill') },
          { label: 'Faster Block Rate', render: tpl('{102}% item / {69}% skill') },
          { label: 'Faster Run/Walk', render: tpl('{96}% item / {67}% skill') },
          { label: 'Faster Cast Rate', render: tpl('{105}%') },
        ],
      },
      {
        title: 'Absorb',
        rows: [
          {
            label: 'Fire',
            render: tpl('{142}% / {143} flat'),
            colorVar: 'var(--stat-fire, #e05d44)',
          },
          {
            label: 'Cold',
            render: tpl('{148}% / {149} flat'),
            colorVar: 'var(--stat-cold, #5b9bd5)',
          },
          {
            label: 'Lightning',
            render: tpl('{144}% / {145} flat'),
            colorVar: 'var(--stat-lightning, #d4b106)',
          },
          {
            label: 'Magic',
            render: tpl('{146}% / {147} flat'),
            colorVar: 'var(--stat-magic, #b366cc)',
          },
        ],
      },
      {
        title: 'Life / Mana on Hit',
        rows: [
          { label: 'Leech (Life / Mana)', render: tpl('{60}% / {62}%') },
          { label: 'After Each Kill (Life / Mana)', render: tpl('{86} / {138}') },
          { label: 'On Striking (Life / Mana)', render: tpl('{208} / {209}') },
          { label: 'On Attack (Life / Mana)', render: tpl('{210} / {295}') },
        ],
      },
      {
        title: 'Minions',
        rows: [
          { label: 'Life', render: tpl('+{444}%') },
          { label: 'Damage', render: tpl('+{470}%') },
          { label: 'Resistances', render: tpl('+{487}%') },
          { label: 'Attack Rating', render: tpl('+{500}%') },
        ],
      },
      {
        title: 'Misc',
        rows: [
          { label: 'Buff/Debuff Duration', render: tpl('{409}%') },
          { label: 'Life Regenerated / Sec', render: tpl('{74}') },
          { label: 'Mana Regeneration', render: tpl('{27}%') },
          { label: 'Target Takes Additional Damage', render: tpl('{489}') },
          { label: 'Damage to Demons', render: tpl('+{121}%') },
          { label: 'Damage to Undead', render: tpl('+{122}%') },
          { label: 'Slows Target / Melee Target', render: tpl('{150}% / {376}%') },
          { label: 'Slows Attacker / Ranged Attacker', render: tpl('{363}% / {493}%') },
        ],
      },
      {
        title: 'Flags',
        rows: [
          {
            label: 'Slain Monsters Rest In Peace',
            render: tpl('Yes'),
            visible: (u) => num(u.stats, 108) >= 1,
          },
          {
            label: 'Half Freeze Duration',
            render: tpl('Yes'),
            visible: (u) => num(u.stats, 118) >= 1,
          },
          { label: 'Cannot Be Frozen', render: tpl('Yes'), visible: (u) => num(u.stats, 153) >= 1 },
        ],
      },
    ];
  }

  let visibleSections = $derived.by(() => {
    const unit = active;
    if (!unit) return [];
    return buildSections(unit)
      .map((section) => ({
        title: section.title,
        rows: section.rows
          .filter((row) => !row.visible || row.visible(unit))
          .map((row) => ({
            label: row.label,
            value: row.render(unit),
            tooltip: row.tooltip,
            colorVar: row.colorVar,
          })),
      }))
      .filter((section) => section.rows.length > 0);
  });

  // Zeroed stand-in unit, used only to pull the static section/row shape
  // (titles, labels, tooltips) out of `buildSections` while we wait for real
  // data — its computed values are never shown. Rows that are conditionally
  // `visible` based on real data (e.g. the Flags section) can't be resolved
  // from a stub and are left out of the skeleton; they appear once real data
  // arrives, same as any other stat that changes over time.
  const EMPTY_UNIT: UnitStats = { class: 0, stats: {}, baseStats: {}, damage: null };
  let skeletonSections = $derived.by(() =>
    buildSections(EMPTY_UNIT)
      .filter((section) => section.rows.some((row) => !row.visible))
      .map((section) => ({
        title: section.title,
        rows: section.rows.filter((row) => !row.visible),
      })),
  );

  onMount(() => {
    statsStore.initListeners();
    statsStore.startPolling();

    return () => {
      statsStore.stopPolling();
    };
  });
</script>

<div class="stats-tab">
  <div class="entity-toggle">
    <button
      class="entity-btn"
      class:active={activeEntity === 'player'}
      onclick={() => {
        activeEntity = 'player';
      }}
    >
      Player
    </button>
    <button
      class="entity-btn"
      class:active={activeEntity === 'merc'}
      onclick={() => {
        activeEntity = 'merc';
      }}
    >
      Mercenary
    </button>
  </div>

  {#if !active && !awaitingFirstData}
    <p class="no-data">
      No character data — make sure Diablo II is running and the {activeEntity === 'merc'
        ? 'mercenary is hired'
        : 'character is loaded'}.
    </p>
  {:else if awaitingFirstData}
    <div class="stats-grid">
      {#each skeletonSections as section (section.title)}
        <div class="stats-card">
          <h3 class="stats-card-title">{section.title}</h3>
          <div class="stats-list">
            {#each section.rows as row (row.label)}
              <div class="stat-row" title={row.tooltip}>
                <span class="label-cell">{row.label}</span>
                <span class="value-cell skeleton" style:color={row.colorVar}>&nbsp;</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="stats-grid">
      {#each visibleSections as section (section.title)}
        <div class="stats-card">
          <h3 class="stats-card-title">{section.title}</h3>
          <div class="stats-list">
            {#each section.rows as row (row.label)}
              <div class="stat-row" title={row.tooltip}>
                <span class="label-cell">{row.label}</span>
                <span class="value-cell" style:color={row.colorVar}>{row.value}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .stats-tab {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    overflow-y: auto;
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

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(450px, 1fr));
    gap: var(--space-3);
    align-content: start;
  }

  .stats-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    overflow: hidden;
    min-width: 0;
  }

  .stats-card-title {
    margin: 0;
    padding: var(--space-2);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border-primary);
    background: var(--bg-tertiary, transparent);
  }

  .stats-list {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }

  .stat-row {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    column-gap: var(--space-3);
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border-primary);
  }

  .stat-row:last-child {
    border-bottom: none;
  }

  .label-cell {
    color: var(--text-secondary);
    flex: 0 0 210px;
  }

  .value-cell {
    text-align: right;
    color: var(--text-primary);
    font-weight: 500;
    word-break: break-word;
    flex: 1 1 auto;
    min-width: 0;
  }

  .no-data {
    color: var(--text-muted);
    font-size: var(--text-sm);
    text-align: center;
    padding: var(--space-4);
  }

  .value-cell.skeleton {
    display: inline-block;
    width: 64px;
    height: 0.9em;
    border-radius: var(--radius-sm);
    background: linear-gradient(
      90deg,
      var(--bg-tertiary) 25%,
      var(--border-primary) 50%,
      var(--bg-tertiary) 75%
    );
    background-size: 200% 100%;
    animation: skeleton-shimmer 1.4s linear infinite;
  }

  @keyframes skeleton-shimmer {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
  }
</style>
