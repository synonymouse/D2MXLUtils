<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { CLASSES } from '../lib/breakpoint-constants';

  interface DamageStats {
    physMin1h: number;
    physMax1h: number;
    physMin2h: number;
    physMax2h: number;
    fireMin: number;
    fireMax: number;
    coldMin: number;
    coldMax: number;
    lightningMin: number;
    lightningMax: number;
    magicMin: number;
    magicMax: number;
    poisonMinPerSec: number;
    poisonMaxPerSec: number;
    strDamageBonusPct: number;
    dexDamageBonusPct: number;
  }

  interface UnitStats {
    class: number;
    stats: Record<string, number>;
    baseStats: Record<string, number>;
    damage: DamageStats | null;
  }

  interface StatsPayload {
    player: UnitStats | null;
    merc: UnitStats | null;
  }

  let livePlayer = $state<UnitStats | null>(null);
  let liveMerc = $state<UnitStats | null>(null);
  let activeEntity = $state<'player' | 'merc'>('player');

  let active = $derived(activeEntity === 'player' ? livePlayer : liveMerc);

  // Life-per-Vitality / Mana-per-Energy growth factors, indexed by class id
  // (0=Amazon..6=Assassin, matches `CLASSES` in breakpoint-constants.ts).
  // Sourced from Median XL's CharStats.txt via the community
  // MedianXLOfflineTools data files — these differ from vanilla D2's
  // per-class values, so don't fall back to vanilla numbers here.
  // Re-verify against a current game patch if the "from Vit/Ene" figures
  // look off.
  const LIFE_PER_VIT = [2, 2, 1, 3, 3, 2, 2];
  const MANA_PER_ENE = [2.25, 2.5, 3, 1.5, 1.5, 3, 2.25];

  // Elemental resist cap is 75% by default, extendable via `+max resist`
  // item stats. Physical resist cap is a fixed 50% with no known
  // cap-increasing stat — its value can still overcap above 50 through
  // other means, it just isn't shown with a computed max here.
  const BASE_RESIST_CAP = 75;
  const PHYSICAL_RESIST_CAP = 50;

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
    const raw = num(u.stats, 904);
    const capped = Math.min(raw, 100);
    return raw > 100 ? `${capped}% (overcap, raw ${raw}%)` : `${capped}%`;
  }

  function resistLabel(u: UnitStats, currentId: number, maxBonusId: number | null): string {
    const current = num(u.stats, currentId);
    const max = maxBonusId === null ? PHYSICAL_RESIST_CAP : BASE_RESIST_CAP + num(u.stats, maxBonusId);
    return `${current}% / max ${max}%`;
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
          { label: 'Charms', render: tpl('{356} / 97') },
          { label: 'Magic Find', render: tpl('{80}%') },
          { label: 'Gold Find', render: tpl('{79}%') },
          { label: 'Experience Gain', render: tpl('+{85}%') },
          { label: 'Max Skill Level', render: tpl('+{479}') },
        ],
      },
      {
        title: 'Attributes',
        rows: [
          {
            label: 'Strength',
            render: (u) => attributeBreakdown(u, 0, 359),
            tooltip: 'points + flat bonus (item/skill) + percent bonus, in points (percent) = total',
          },
          { label: 'Dexterity', render: (u) => attributeBreakdown(u, 2, 360) },
          { label: 'Vitality', render: (u) => attributeBreakdown(u, 3, 362) },
          { label: 'Energy', render: (u) => attributeBreakdown(u, 1, 361) },
          {
            label: 'Life',
            render: (u) => lifeManaBreakdown(u, 7, 76, 3, LIFE_PER_VIT[u.class] ?? 2),
            tooltip:
              'Total is read directly from the game. The %Life bonus applies to your whole life pool, including the Vitality-derived part. The "from Vitality" figure is an estimate (current total Vitality × class factor) — not a precise engine breakdown.',
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
          { label: 'Lightning', render: tpl('{__lightning}'), colorVar: 'var(--stat-lightning, #d4b106)' },
          { label: 'Magic', render: tpl('{__magic}'), colorVar: 'var(--stat-magic, #b366cc)' },
          { label: 'Poison', render: tpl('{__poison}/s'), colorVar: 'var(--stat-poison, #4caf50)' },
          {
            label: 'Innate Elemental Damage',
            render: tpl('+{484}%'),
            tooltip:
              'Bonus % to a weapon base\'s built-in elemental-from-attribute conversion (elemental bows/claws, etc). Only relevant for weapons with an innate elemental base.',
          },
          { label: 'Weapon Physical Damage', render: tpl('{25}%'), tooltip: 'Enhanced Weapon Damage' },
          {
            label: 'Strength Damage Bonus',
            render: tpl('+{__strdmg}%'),
            tooltip: 'Approximate individual contribution — see Physical rows below for the actual total',
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
          { label: 'Evade', render: tpl('{340}%'), tooltip: 'Chance to avoid any attack while moving' },
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
            render: (u) => resistLabel(u, 39, 40),
            colorVar: 'var(--stat-fire, #e05d44)',
          },
          {
            label: 'Cold',
            render: (u) => resistLabel(u, 43, 44),
            colorVar: 'var(--stat-cold, #5b9bd5)',
          },
          {
            label: 'Lightning',
            render: (u) => resistLabel(u, 41, 42),
            colorVar: 'var(--stat-lightning, #d4b106)',
          },
          {
            label: 'Poison',
            render: (u) => resistLabel(u, 45, 46),
            colorVar: 'var(--stat-poison, #4caf50)',
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
          { label: 'Poison Skill Duration', render: tpl('{431}%'), colorVar: 'var(--stat-poison, #4caf50)' },
          { label: 'Physical / Magic Pierce', render: tpl('{357}% / 0%') },
          { label: 'Spell Focus (flat)', render: tpl('{485}') },
          { label: 'Spell Focus (%)', render: tpl('+{488}%'), tooltip: 'Bonus % from items/runes, boosts flat Spell Focus multiplicatively' },
          {
            label: 'Spell Focus Cap',
            render: spellFocusCapLabel,
            tooltip: "100% (reached around 1000 effective Spell Focus) means you don't benefit from more — anything past that is wasted",
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
          { label: 'Fire', render: tpl('{142}% / {143} flat'), colorVar: 'var(--stat-fire, #e05d44)' },
          { label: 'Cold', render: tpl('{148}% / {149} flat'), colorVar: 'var(--stat-cold, #5b9bd5)' },
          {
            label: 'Lightning',
            render: tpl('{144}% / {145} flat'),
            colorVar: 'var(--stat-lightning, #d4b106)',
          },
          { label: 'Magic', render: tpl('{146}% / {147} flat'), colorVar: 'var(--stat-magic, #b366cc)' },
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
          { label: 'Slain Monsters Rest In Peace', render: tpl('Yes'), visible: (u) => num(u.stats, 108) >= 1 },
          { label: 'Half Freeze Duration', render: tpl('Yes'), visible: (u) => num(u.stats, 118) >= 1 },
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

  onMount(() => {
    const unlisteners: UnlistenFn[] = [];

    invoke('set_stats_polling', { enabled: true });

    listen<StatsPayload>('stats-update', (event) => {
      livePlayer = event.payload.player;
      liveMerc = event.payload.merc;
    }).then((u) => unlisteners.push(u));

    return () => {
      invoke('set_stats_polling', { enabled: false });
      unlisteners.forEach((u) => u());
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

  {#if !active}
    <p class="no-data">
      No character data — make sure Diablo II is running and the {activeEntity === 'merc'
        ? 'mercenary is hired'
        : 'character is loaded'}.
    </p>
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
</style>
