<script lang="ts">
  type UniqueKind = 'tu' | 'su' | 'ssu' | 'sssu';

  interface NotificationFilter {
    color?: string | null;
    sound?: number | null;
    display_stats: boolean;
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
      {item.stats.length > 80 ? item.stats.substring(0, 80) + '...' : item.stats}
    </div>
  {/if}
</div>

<style>
  .notification {
    max-width: 320px;
    font-family: var(--font-mono);
    padding: var(--space-2) var(--space-3);
  }

  .item-name {
    font-weight: 600;
    line-height: 1.3;
  }

  .item-base {
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-muted);
    line-height: 1.3;
  }

  .badges {
    margin-left: var(--space-2);
    font-size: var(--text-xs);
    font-weight: 400;
  }

  .ethereal {
    color: var(--quality-ethereal);
  }

  .unid {
    color: var(--text-muted);
    margin-left: var(--space-1);
  }

  .item-stats {
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
