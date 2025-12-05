<script lang="ts">
  interface ItemDrop {
    unit_id: number;
    class: number;
    quality: string;
    name: string;
    stats: string;
    is_ethereal: boolean;
    is_identified: boolean;
  }
  
  interface Props {
    item: ItemDrop;
    exiting?: boolean;
    fontSize?: number;
    opacity?: number;
  }
  
  let { item, exiting = false, fontSize = 14, opacity = 0.9 }: Props = $props();
  
  const qualityColors: Record<string, string> = {
    'Unique': 'var(--quality-unique)',
    'Set': 'var(--quality-set)',
    'Rare': 'var(--quality-rare)',
    'Magic': 'var(--quality-magic)',
    'Crafted': 'var(--quality-crafted)',
    'Superior': 'var(--quality-superior)',
    'Normal': 'var(--quality-normal)'
  };
  
  const nameColor = $derived(qualityColors[item.quality] ?? 'var(--text-muted)');
</script>

<div 
  class="notification"
  class:exiting
  style:font-size="{fontSize}px"
  style:background-color="rgba(0, 0, 0, {opacity})"
>
  <div class="item-name" style:color={nameColor}>
    {item.name}
  </div>
  <div class="item-meta">
    <span class="quality">{item.quality}</span>
    {#if item.is_ethereal}
      <span class="ethereal">ETH</span>
    {/if}
    {#if !item.is_identified}
      <span class="unid">[UNID]</span>
    {/if}
  </div>
  {#if item.stats}
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
    /* Animation placeholder - currently instant */
    /* animation: notification-enter 300ms ease-out; */
  }

  
  .item-name {
    font-weight: 600;
    line-height: 1.3;
  }
  
  .item-meta {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }
  
  .ethereal {
    color: var(--quality-ethereal);
  }
  
  .unid {
    color: var(--text-muted);
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
