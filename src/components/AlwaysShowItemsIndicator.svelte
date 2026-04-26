<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  let ingame = $state(false);
  let alwaysShowOn = $state<boolean | null>(null);

  const visible = $derived(ingame && alwaysShowOn === false);

  onMount(() => {
    const unlisteners: Array<() => void> = [];

    listen<string>('game-status', (event) => {
      ingame = event.payload === 'ingame';
      if (!ingame) {
        alwaysShowOn = null;
      }
    }).then((u) => unlisteners.push(u));

    listen<boolean>('always-show-items-state', (event) => {
      alwaysShowOn = event.payload;
    }).then((u) => unlisteners.push(u));

    return () => {
      unlisteners.forEach((u) => u());
    };
  });
</script>

{#if visible}
  <div class="indicator" role="status" aria-live="polite">
    Items hidden — press Alt
  </div>
{/if}

<style>
  .indicator {
    position: fixed;
    top: var(--space-3);
    left: var(--space-3);
    color: #fff;
    font-size: var(--text-sm);
    line-height: 1;
    pointer-events: none;
    white-space: nowrap;
    z-index: 9999;
  }
</style>
