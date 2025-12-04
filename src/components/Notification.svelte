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
  }
  
  let { item, exiting = false }: Props = $props();
  
  const qualityColors: Record<string, { color: string; border: string }> = {
    'Unique': { color: 'var(--quality-unique)', border: 'var(--quality-unique)' },
    'Set': { color: 'var(--quality-set)', border: 'var(--quality-set)' },
    'Rare': { color: 'var(--quality-rare)', border: 'var(--quality-rare)' },
    'Magic': { color: 'var(--quality-magic)', border: 'var(--quality-magic)' },
    'Crafted': { color: 'var(--quality-crafted)', border: 'var(--quality-crafted)' },
    'Superior': { color: 'var(--quality-superior)', border: 'var(--quality-superior)' },
    'Normal': { color: 'var(--quality-normal)', border: 'var(--quality-normal)' }
  };
  
  const style = $derived(qualityColors[item.quality] ?? { color: 'var(--text-muted)', border: 'var(--border-primary)' });
</script>

<div 
  class="notification notification-quality"
  class:exiting
  style:border-left-color={style.border}
>
  <div class="item-name" style:color={style.color}>
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
  }
  
  .notification.exiting {
    animation: notification-exit 200ms ease-out forwards;
  }
  
  .item-name {
    font-size: var(--text-sm);
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

