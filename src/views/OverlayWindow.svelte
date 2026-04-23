<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { NotificationStack, OverlayEditGrid } from '../components';
  import { settingsStore } from '../stores';
  import { playSound } from '../lib/sound-player';

  type UniqueKind = 'tu' | 'su' | 'ssu' | 'sssu';

  interface NotificationFilter {
    color?: string | null;
    sound?: number | null;
    display_stats: boolean;
    matched_stat_lines?: number[] | null;
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

  interface ItemWithState extends ItemDrop {
    exiting: boolean;
  }

  let items = $state<ItemWithState[]>([]);

  // Read settings from store (reactive)
  let notificationDuration = $derived(settingsStore.settings.notificationDuration);
  let notificationFontSize = $derived(settingsStore.settings.notificationFontSize);
  let notificationOpacity = $derived(settingsStore.settings.notificationOpacity);
  let compactName = $derived(settingsStore.settings.compactName);
  let notificationX = $derived(settingsStore.settings.notificationX);
  let notificationY = $derived(settingsStore.settings.notificationY);
  let soundVolume = $derived(settingsStore.settings.soundVolume);

  let editActive = $state(false);
  let pendingX = $state(0);
  let pendingY = $state(0);
  
  // Animation duration placeholder (currently 0 for instant, can be changed later)
  const EXIT_ANIMATION_DURATION = 0;
  
  const removalTimers = new Map<number, number>();

  function removeItem(unit_id: number) {
    items = items.filter(item => item.unit_id !== unit_id);
    removalTimers.delete(unit_id);
  }

  function startExitAnimation(unit_id: number) {
    // Mark item as exiting to trigger animation (placeholder for future use)
    items = items.map(item => 
      item.unit_id === unit_id 
        ? { ...item, exiting: true } 
        : item
    );
    
    // Remove item after animation completes (instant for now)
    if (EXIT_ANIMATION_DURATION > 0) {
      setTimeout(() => {
        removeItem(unit_id);
      }, EXIT_ANIMATION_DURATION);
    } else {
      removeItem(unit_id);
    }
  }

  function addItem(item: ItemDrop, duration: number) {
    // Add item to the stack with exiting = false
    const itemWithState: ItemWithState = { ...item, exiting: false };
    items = [itemWithState, ...items].slice(0, 100);
    
    // Clear existing timer if item already exists (shouldn't happen but just in case)
    const existingTimer = removalTimers.get(item.unit_id);
    if (existingTimer) {
      clearTimeout(existingTimer);
    }
    
    // Set timer to start exit after duration
    const timer = window.setTimeout(() => {
      startExitAnimation(item.unit_id);
    }, duration);
    
    removalTimers.set(item.unit_id, timer);
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let syncTimer: number | null = null;

    // Listen for item drops
    listen<ItemDrop>('item-drop', (event) => {
      addItem(event.payload, notificationDuration);
      const s = event.payload.filter?.sound;
      if (s != null) playSound(s, soundVolume);
    }).then(u => unlisteners.push(u));

    listen<{ active: boolean }>('overlay-edit-mode', async (event) => {
      const active = event.payload.active;
      if (active) {
        pendingX = notificationX;
        pendingY = notificationY;
        editActive = true;
        try {
          await invoke('set_overlay_interactive', { active: true });
        } catch (err) {
          console.error('[Overlay] set_overlay_interactive(true) failed:', err);
        }
      } else {
        editActive = false;
        try {
          await invoke('set_overlay_interactive', { active: false });
        } catch (err) {
          console.error('[Overlay] set_overlay_interactive(false) failed:', err);
        }
        settingsStore.setNotificationPosition(pendingX, pendingY);
      }
    }).then(u => unlisteners.push(u));

    listen('settings-updated', () => {
      settingsStore.load();
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
      // Clear all removal timers
      removalTimers.forEach(timer => clearTimeout(timer));
      removalTimers.clear();
    };
  });
</script>

<main class="overlay">
  <NotificationStack
    {items}
    x={notificationX}
    y={notificationY}
    maxVisible={10}
    fontSize={notificationFontSize}
    opacity={notificationOpacity}
    {compactName}
  />
  {#if editActive}
    <OverlayEditGrid
      x={pendingX}
      y={pendingY}
      onchange={(nx, ny) => { pendingX = nx; pendingY = ny; }}
    />
  {/if}
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
