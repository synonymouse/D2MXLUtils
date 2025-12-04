<script lang="ts">
  import Notification from './Notification.svelte';
  
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
    items: ItemDrop[];
    position?: 'bottom-right' | 'bottom-left' | 'top-right' | 'top-left';
    maxVisible?: number;
  }
  
  let {
    items,
    position = 'bottom-right',
    maxVisible = 10
  }: Props = $props();
  
  const visibleItems = $derived(items.slice(0, maxVisible));
  
  const positionStyles: Record<string, string> = {
    'bottom-right': 'bottom: var(--space-5); right: var(--space-5); align-items: flex-end;',
    'bottom-left': 'bottom: var(--space-5); left: var(--space-5); align-items: flex-start;',
    'top-right': 'top: var(--space-5); right: var(--space-5); align-items: flex-end;',
    'top-left': 'top: var(--space-5); left: var(--space-5); align-items: flex-start;'
  };
  
  const stackDirection = $derived(position.startsWith('bottom') ? 'column-reverse' : 'column');
</script>

<div 
  class="notification-stack"
  style="{positionStyles[position]} flex-direction: {stackDirection};"
>
  {#each visibleItems as item (item.unit_id)}
    <Notification {item} />
  {/each}
</div>

<style>
  .notification-stack {
    position: fixed;
    display: flex;
    gap: var(--space-2);
    pointer-events: none;
    z-index: 9999;
  }
  
  .notification-stack > :global(*) {
    pointer-events: auto;
  }
</style>

