<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import {
    AlwaysShowItemsIndicator,
    DpsMeter,
    ItemSearchOverlay,
    LootHistoryPanel,
    NotificationStack,
    OverlayEditGrid,
  } from '../components';
  import { dpsMeterStore, settingsStore } from '../stores';
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

  let notificationDuration = $derived(settingsStore.settings.notificationDuration);
  let notificationFontSize = $derived(settingsStore.settings.notificationFontSize);
  let notificationOpacity = $derived(settingsStore.settings.notificationOpacity);
  let compactName = $derived(settingsStore.settings.compactName);
  let showOnlyMatchedStats = $derived(settingsStore.settings.showOnlyMatchedStats);
  let soundVolume = $derived(settingsStore.settings.soundVolume);

  let editActive = $state(false);
  let historyVisible = $state(false);
  let itemSearchActive = $state(false);
  let inGame = $state(false);

  let dpsMeterEnabled = $derived(settingsStore.settings.dpsMeter?.enabled ?? false);
  let dpsMeterVisible = $derived(dpsMeterEnabled && inGame);

  const EXIT_ANIMATION_DURATION = 0;

  const removalTimers = new Map<number, number>();

  function removeItem(unit_id: number) {
    items = items.filter((item) => item.unit_id !== unit_id);
    removalTimers.delete(unit_id);
  }

  function startExitAnimation(unit_id: number) {
    items = items.map((item) => (item.unit_id === unit_id ? { ...item, exiting: true } : item));

    if (EXIT_ANIMATION_DURATION > 0) {
      setTimeout(() => {
        removeItem(unit_id);
      }, EXIT_ANIMATION_DURATION);
    } else {
      removeItem(unit_id);
    }
  }

  function addItem(item: ItemDrop, duration: number) {
    const itemWithState: ItemWithState = { ...item, exiting: false };
    // The backend re-emits `item-drop` for the same ground item on
    // re-evaluation (e.g. still unpicked a tick later) — prepending
    // unconditionally left two array entries sharing the same unit_id,
    // which breaks NotificationStack's keyed `{#each item.unit_id}` and
    // visually layered two toasts for the same item. Replace in place
    // instead when one's already showing.
    const existingIndex = items.findIndex((existing) => existing.unit_id === item.unit_id);
    if (existingIndex !== -1) {
      const next = items.slice();
      next[existingIndex] = itemWithState;
      items = next;
    } else {
      items = [itemWithState, ...items].slice(0, 100);
    }

    const existingTimer = removalTimers.get(item.unit_id);
    if (existingTimer) {
      clearTimeout(existingTimer);
    }

    const timer = window.setTimeout(() => {
      startExitAnimation(item.unit_id);
    }, duration);

    removalTimers.set(item.unit_id, timer);
  }

  async function syncOverlayInteractivity() {
    try {
      await invoke('set_overlay_interactive', {
        active: editActive || historyVisible || itemSearchActive,
        keyboardActive: itemSearchActive,
      });
    } catch (err) {
      console.error('[Overlay] set_overlay_interactive failed:', err);
    }
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let syncTimer: number | null = null;

    dpsMeterStore.initialize();

    listen<ItemDrop>('item-drop', (event) => {
      addItem(event.payload, notificationDuration);
      const s = event.payload.filter?.sound;
      if (s != null) playSound(s, soundVolume);
    }).then((u) => unlisteners.push(u));

    listen<{ unit_id: number; class: number }>('goblin-detected', () => {
      const slot = settingsStore.settings.goblinAlertSlot;
      if (slot != null) playSound(slot, soundVolume);
    }).then((u) => unlisteners.push(u));

    listen<{ active: boolean }>('overlay-edit-mode', async (event) => {
      editActive = event.payload.active;
      try {
        await invoke('set_overlay_edit_mode', {
          active: editActive,
        });
        await syncOverlayInteractivity();
      } catch (err) {
        console.error('[Overlay] edit mode update failed:', err);
      }
    }).then((u) => unlisteners.push(u));

    listen<{ visible?: boolean } | null>('toggle-loot-history', async (event) => {
      const next = event.payload?.visible ?? !historyVisible;
      if (next === historyVisible) return;
      historyVisible = next;
      await syncOverlayInteractivity();
    }).then((u) => unlisteners.push(u));

    listen<string>('game-status', (event) => {
      inGame = event.payload === 'ingame';
    }).then((u) => unlisteners.push(u));

    invoke('get_game_status')
      .then((status: unknown) => {
        inGame = status === 'ingame';
      })
      .catch(() => {
        inGame = false;
      });

    listen('reset-dps-session', async () => {
      try {
        await invoke('reset_dps_session');
      } catch (err) {
        console.error('[Overlay] reset_dps_session failed:', err);
      }
    }).then((u) => unlisteners.push(u));

    syncTimer = window.setInterval(() => {
      invoke('sync_overlay_with_game').catch(() => {
        // Silent: game might not be running or not focused
      });
    }, 250);

    return () => {
      unlisteners.forEach((u) => u());
      if (syncTimer !== null) {
        clearInterval(syncTimer);
      }
      removalTimers.forEach((timer) => clearTimeout(timer));
      removalTimers.clear();
    };
  });
</script>

<main class="overlay">
  <NotificationStack
    {items}
    maxVisible={10}
    fontSize={notificationFontSize}
    opacity={notificationOpacity}
    {compactName}
    {showOnlyMatchedStats}
  />
  <AlwaysShowItemsIndicator />
  <ItemSearchOverlay
    onActiveChange={(active) => {
      itemSearchActive = active;
      return syncOverlayInteractivity();
    }}
  />
  {#if historyVisible}
    <LootHistoryPanel
      onClose={() => {
        historyVisible = false;
        void syncOverlayInteractivity();
      }}
    />
  {/if}
  {#if dpsMeterVisible}
    <DpsMeter />
  {/if}
  {#if editActive}
    <OverlayEditGrid />
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
