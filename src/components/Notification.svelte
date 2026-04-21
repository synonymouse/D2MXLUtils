<script lang="ts">
  type UniqueKind = 'tu' | 'su' | 'ssu' | 'sssu';

  interface NotificationFilter {
    color?: string | null;
    sound?: number | null;
    display_stats: boolean;
    matched_stat_line?: number | null;
  }

  interface ItemDrop {
    unit_id: number;
    class: number;
    quality: string;
    name: string;
    base_name: string;
    stats: string;
    is_ethereal: boolean;
    is_identified: boolean;
    unique_kind?: UniqueKind | null;
    filter?: NotificationFilter | null;
  }

  interface Props {
    item: ItemDrop;
    exiting?: boolean;
    fontSize?: number;
    opacity?: number;
    compactName?: boolean;
  }

  let {
    item,
    exiting = false,
    fontSize = 14,
    opacity = 0.9,
    compactName = false,
  }: Props = $props();

  const qualityColors: Record<string, string> = {
    'Unique': 'var(--quality-unique)',
    'Set': 'var(--quality-set)',
    'Rare': 'var(--quality-rare)',
    'Magic': 'var(--quality-magic)',
    'Crafted': 'var(--quality-crafted)',
    'Honorific': 'var(--quality-crafted)',
    'Superior': 'var(--quality-superior)',
    'Inferior': 'var(--quality-normal)',
    'Normal': 'var(--quality-normal)'
  };

  const nameColor = $derived(qualityColors[item.quality] ?? 'var(--text-muted)');

  // Items that get the two-line "name + base" treatment.
  const isLargeDrop = $derived(item.quality === 'Set' || item.unique_kind != null);

  const showStats = $derived(item.filter?.display_stats === true && item.stats.length > 0);
  const statLines = $derived(showStats ? item.stats.split('\n') : []);
  const matchedLineIdx = $derived(item.filter?.matched_stat_line ?? null);

  // Compact-name yields a single line, but the stat-flag exception keeps
  // the full two-line header so the drop reads cleanly above its stats.
  const showTwoLines = $derived(isLargeDrop && (!compactName || showStats));

  const primary = $derived(showTwoLines ? item.name : item.base_name);
  const secondary = $derived(showTwoLines ? item.base_name : null);

  const hasBadges = $derived(item.is_ethereal || !item.is_identified);
</script>

<div
  class="notification"
  class:exiting
  style:font-size="{fontSize}px"
  style:background-color="rgba(0, 0, 0, {opacity})"
>
  <div class="item-name" style:color={nameColor}>
    {primary}{#if hasBadges}
      <span class="badges">
        {#if item.is_ethereal}<span class="ethereal">ETH</span>{/if}
        {#if !item.is_identified}<span class="unid">[UNID]</span>{/if}
      </span>
    {/if}
  </div>
  {#if secondary}
    <div class="item-base">{secondary}</div>
  {/if}
  {#if showStats}
    <div class="item-stats">
      {#each statLines as line, i}
        <div class="stat-line" class:matched={i === matchedLineIdx}>{line}</div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .notification {
    max-width: 22em;
    font-family: var(--font-mono);
    padding: 0.45em 0.65em;
  }

  .item-name {
    font-weight: 600;
    line-height: 1.3;
  }

  .item-base {
    margin-top: 0.22em;
    font-size: 0.85em;
    color: var(--notif-base);
    line-height: 1.3;
  }

  .badges {
    margin-left: 0.45em;
    font-size: 0.85em;
    font-weight: 400;
  }

  .ethereal {
    color: var(--quality-ethereal);
  }

  .unid {
    color: var(--notif-muted);
    margin-left: 0.22em;
  }

  .item-stats {
    margin-top: 0.22em;
    font-size: 0.85em;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }

  .stat-line {
    color: var(--notif-stat);
  }

  .stat-line.matched {
    color: var(--notif-stat-matched);
  }
</style>
