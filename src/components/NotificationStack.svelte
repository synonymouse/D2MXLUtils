<script lang="ts">
  import Notification from './Notification.svelte';
  
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
    exiting?: boolean;
  }

  interface Props {
    items: ItemDrop[];
    /** Anchor x position as percentage of overlay width (0-100). */
    x?: number;
    /** Anchor y position as percentage of overlay height (0-100). */
    y?: number;
    maxVisible?: number;
    fontSize?: number;
    opacity?: number;
    compactName?: boolean;
  }

  let {
    items,
    x = 1,
    y = 1,
    maxVisible = 10,
    fontSize = 14,
    opacity = 0.9,
    compactName = false,
  }: Props = $props();

  const visibleItems = $derived(items.slice(0, maxVisible));
</script>

<div
  class="notification-stack"
  style="top: {y}%; left: {x}%;"
>
  {#each visibleItems as item (item.unit_id)}
    <Notification
      {item}
      exiting={item.exiting ?? false}
      {fontSize}
      {opacity}
      {compactName}
    />
  {/each}
</div>

<style>
  .notification-stack {
    position: fixed;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    pointer-events: none;
    z-index: 9999;
  }
  
  .notification-stack > :global(*) {
    pointer-events: auto;
  }
</style>
