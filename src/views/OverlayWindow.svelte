<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { NotificationStack } from '../components';

  interface ItemDrop {
    unit_id: number;
    class: number;
    quality: string;
    name: string;
    stats: string;
    is_ethereal: boolean;
    is_identified: boolean;
  }

  let items = $state<ItemDrop[]>([]);

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let syncTimer: number | null = null;

    // Listen for item drops
    listen<ItemDrop>('item-drop', (event) => {
      const item = event.payload;
      items = [item, ...items].slice(0, 100);
    }).then(u => unlisteners.push(u));

    // Periodically sync overlay position with Diablo II window
    syncTimer = window.setInterval(() => {
      invoke('sync_overlay_with_game').catch(() => {
        // Silent: game might not be running or not focused
      });
    }, 250);

    return () => {
      unlisteners.forEach(u => u());
      if (syncTimer !== null) {
        clearInterval(syncTimer);
      }
    };
  });
</script>

<main class="overlay">
  <NotificationStack {items} position="bottom-right" maxVisible={10} />
</main>

<style>
  :global(body) {
    background: var(--bg-overlay) !important;
  }
  
  .overlay {
    position: fixed;
    inset: 0;
    background: var(--bg-overlay);
    pointer-events: none;
  }
</style>

