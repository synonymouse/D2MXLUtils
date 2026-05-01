<script lang="ts">
  import { onMount } from 'svelte';
  import { lootHistoryStore, type LootHistoryEntry } from '../stores';

  let { onClose } = $props<{ onClose: () => void }>();

  let scrollContainer: HTMLDivElement | null = $state(null);
  let stickToBottom = $state(true);

  // Palette mirrors Notification.svelte's nameColor cascade exactly:
  // explicit rule color → quality color → muted fallback.
  const notifyColors: Record<string, string> = {
    white:  'var(--notify-white)',
    red:    'var(--notify-red)',
    lime:   'var(--notify-lime)',
    blue:   'var(--notify-blue)',
    gold:   'var(--notify-gold)',
    grey:   'var(--notify-grey)',
    black:  'var(--notify-black)',
    pink:   'var(--notify-pink)',
    orange: 'var(--notify-orange)',
    yellow: 'var(--notify-yellow)',
    green:  'var(--notify-green)',
    purple: 'var(--notify-purple)',
  };

  const qualityColors: Record<string, string> = {
    'Unique':    'var(--quality-unique)',
    'Set':       'var(--quality-set)',
    'Rare':      'var(--quality-rare)',
    'Magic':     'var(--quality-magic)',
    'Crafted':   'var(--quality-crafted)',
    'Honorific': 'var(--quality-crafted)',
    'Superior':  'var(--quality-superior)',
    'Inferior':  'var(--quality-normal)',
    'Normal':    'var(--quality-normal)',
  };

  function formatTime(ms: number): string {
    const d = new Date(ms);
    const hh = d.getHours().toString().padStart(2, '0');
    const mm = d.getMinutes().toString().padStart(2, '0');
    const ss = d.getSeconds().toString().padStart(2, '0');
    return `${hh}:${mm}:${ss}`;
  }

  function pickupIcon(state: LootHistoryEntry['pickup']): string {
    switch (state) {
      case 'picked_up': return '✓';
      case 'lost': return '⊘';
      case 'pending': return '⏳';
    }
  }

  function pickupClass(state: LootHistoryEntry['pickup']): string {
    return `pickup pickup-${state}`;
  }

  function nameColor(entry: LootHistoryEntry): string {
    // Final fallback is hardcoded — `--text-muted` flips to a dark value
    // under the light theme and would be invisible on our dark panel.
    return (entry.color ? notifyColors[entry.color] : undefined)
      ?? qualityColors[entry.quality]
      ?? '#bdbdbd';
  }

  function onScroll() {
    if (!scrollContainer) return;
    const el = scrollContainer;
    stickToBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 50;
  }

  // Auto-scroll to bottom only when the user is already near the bottom.
  $effect(() => {
    void lootHistoryStore.entries.length;
    if (stickToBottom && scrollContainer) {
      queueMicrotask(() => {
        if (scrollContainer) {
          scrollContainer.scrollTop = scrollContainer.scrollHeight;
        }
      });
    }
  });

  onMount(() => {
    void lootHistoryStore.initialize();
  });
</script>

<div class="loot-history-panel" role="dialog" aria-label="Loot history">
  <header>
    <h2>Loot History</h2>
    <div class="header-actions">
      <button
        type="button"
        class="clear-btn"
        onclick={() => lootHistoryStore.clear()}
        aria-label="Clear history"
      >Clear</button>
      <button
        type="button"
        class="close"
        onclick={onClose}
        aria-label="Close"
      >×</button>
    </div>
  </header>
  <div
    class="list"
    bind:this={scrollContainer}
    onscroll={onScroll}
  >
    {#each lootHistoryStore.entries as entry (entry.seed !== 0 ? `s:${entry.seed}` : `u:${entry.unit_id}`)}
      <div class="row">
        <span class="time">[{formatTime(entry.timestamp_ms)}]</span>
        <span class={pickupClass(entry.pickup)}>{pickupIcon(entry.pickup)}</span>
        <span class="name" style:color={nameColor(entry)}>{entry.name}</span>
      </div>
    {/each}
    {#if lootHistoryStore.entries.length === 0}
      <div class="empty">No drops in this session yet.</div>
    {/if}
  </div>
</div>

<style>
  .loot-history-panel {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    max-width: min(700px, 60vw);
    width: 100%;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    background: rgba(0, 0, 0, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: var(--radius-md, 8px);
    color: #f0f0f0;
    pointer-events: auto;
    font-family: var(--font-mono, monospace);
    font-size: 13px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .header-actions {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .clear-btn {
    background: transparent;
    border: 1px solid rgba(255, 255, 255, 0.2);
    color: inherit;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 3px;
    cursor: pointer;
  }
  .clear-btn:hover { background: rgba(255, 255, 255, 0.1); }
  .close {
    background: transparent;
    border: none;
    color: inherit;
    font-size: 20px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }
  .close:hover { color: #f88; }
  .list {
    overflow-y: auto;
    padding: 6px 12px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: grid;
    grid-template-columns: auto auto 1fr;
    align-items: baseline;
    gap: 8px;
  }
  .time { color: rgba(255, 255, 255, 0.5); }
  .pickup { width: 1em; text-align: center; }
  :global(.pickup-picked_up) { color: #5cd66a; }
  :global(.pickup-lost) { color: rgba(255, 255, 255, 0.4); }
  :global(.pickup-pending) { color: #f0b400; }
  .name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty { padding: 16px; text-align: center; color: rgba(255, 255, 255, 0.4); }
</style>
