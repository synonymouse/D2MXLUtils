<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { onMount } from 'svelte';
  import { Button, NotificationStack } from './components';

  interface ItemDrop {
    unit_id: number;
    class: number;
    quality: string;
    name: string;
    stats: string;
    is_ethereal: boolean;
    is_identified: boolean;
  }

  let scannerStatus = $state("stopped");
  let gameStatus = $state("unknown");
  let message = $state("");
  let items = $state<ItemDrop[]>([]);
  let logs = $state<string[]>([]);

  // True when running in the dedicated overlay window (label = "overlay")
  let isOverlay = $state(false);

  function addLog(text: string) {
    const time = new Date().toLocaleTimeString();
    logs = [`[${time}] ${text}`, ...logs].slice(0, 50);
  }

  async function toggleScanner() {
    try {
      if (scannerStatus === "stopped" || scannerStatus === "error") {
        message = await invoke('start_scanner') as string;
        addLog("Start scanner requested");
      } else {
        message = await invoke('stop_scanner') as string;
        addLog("Stop scanner requested");
      }
    } catch (e) {
      message = `Error: ${e}`;
      addLog(`Error: ${e}`);
    }
  }

  function clearItems() {
    items = [];
    addLog("Cleared item list");
  }

  function getQualityColor(quality: string): { color: string; border: string } {
    const colors: Record<string, { color: string; border: string }> = {
      'Unique': { color: 'var(--quality-unique)', border: 'var(--quality-unique)' },
      'Set': { color: 'var(--quality-set)', border: 'var(--quality-set)' },
      'Rare': { color: 'var(--quality-rare)', border: 'var(--quality-rare)' },
      'Magic': { color: 'var(--quality-magic)', border: 'var(--quality-magic)' },
      'Crafted': { color: 'var(--quality-crafted)', border: 'var(--quality-crafted)' },
      'Superior': { color: 'var(--quality-superior)', border: 'var(--quality-superior)' },
      'Normal': { color: 'var(--quality-normal)', border: 'var(--quality-normal)' }
    };
    return colors[quality] ?? { color: 'var(--text-muted)', border: 'var(--border-primary)' };
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let syncTimer: number | null = null;

    // Detect if this webview is the overlay window
    const current = getCurrentWebviewWindow();
    isOverlay = current.label === 'overlay';

    // Listen for backend events (both main and overlay listen to item drops)
    listen<string>('scanner-status', (event) => {
      scannerStatus = event.payload;
      if (!isOverlay) {
        addLog(`Scanner status: ${event.payload}`);
      }
    }).then(u => unlisteners.push(u));

    listen<string>('game-status', (event) => {
      gameStatus = event.payload;
      if (!isOverlay) {
        addLog(`Game status: ${event.payload}`);
      }
    }).then(u => unlisteners.push(u));

    listen<ItemDrop>('item-drop', (event) => {
      const item = event.payload;
      items = [item, ...items].slice(0, 100);
      if (!isOverlay) {
        addLog(`Item: ${item.name} (${item.quality})`);
      }
    }).then(u => unlisteners.push(u));

    if (isOverlay) {
      // Periodically sync overlay window with Diablo II position/size
      syncTimer = window.setInterval(() => {
        invoke('sync_overlay_with_game')
          .catch(() => {
            // Silent: game might not be running yet
          });
      }, 250);
    } else {
      addLog("App initialized, listening for events");
    }

    return () => {
      unlisteners.forEach(u => u());
      if (syncTimer !== null) {
        clearInterval(syncTimer);
      }
    };
  });
</script>

{#if isOverlay}
  <main class="overlay">
    <NotificationStack {items} position="bottom-right" maxVisible={10} />
  </main>
{:else}
  <main class="main-window">
    <!-- Header -->
    <header class="header">
      <div class="header-brand">
        <h1 class="header-title">
          D2MXLUtils <span class="text-accent">Drop Notifier</span>
        </h1>
        <p class="header-subtitle">MedianXL Item Scanner</p>
      </div>
      
      <div class="header-controls">
        <div class="status-panel">
          <div class="status-row">
            <span class="status-label">Scanner:</span>
            <span class="status-value" class:success={scannerStatus === 'running'} class:error={scannerStatus === 'error'}>
              {scannerStatus.toUpperCase()}
            </span>
          </div>
          <div class="status-row">
            <span class="status-label">Game:</span>
            <span class="status-value" class:success={gameStatus === 'ingame'}>
              {gameStatus === 'ingame' ? 'IN GAME' : gameStatus === 'menu' ? 'MENU' : 'UNKNOWN'}
            </span>
          </div>
        </div>
        
        <Button 
          variant={scannerStatus === 'running' || scannerStatus === 'starting' ? 'danger' : 'primary'}
          onclick={toggleScanner}
        >
          {scannerStatus === 'running' || scannerStatus === 'starting' ? 'Stop' : 'Start'}
        </Button>
      </div>
    </header>

    <!-- Content grid -->
    <div class="content-grid">
      <!-- Items list -->
      <section class="panel">
        <div class="panel-header">
          <h2 class="panel-title">Found Items ({items.length})</h2>
          <Button variant="ghost" size="sm" onclick={clearItems}>Clear</Button>
        </div>
        
        <div class="items-list">
          {#if items.length === 0}
            <div class="empty-state">
              <p>No items found yet</p>
              <p class="text-muted">Items will appear here when detected in game</p>
            </div>
          {:else}
            {#each items as item (item.unit_id)}
              {@const qc = getQualityColor(item.quality)}
              <div class="item-row" style:border-left-color={qc.border}>
                <div class="item-name" style:color={qc.color}>{item.name}</div>
                <div class="item-meta">
                  {item.quality}
                  {#if item.is_ethereal}<span class="ethereal">ETH</span>{/if}
                  {#if !item.is_identified}<span class="unid">[UNID]</span>{/if}
                </div>
                {#if item.stats}
                  <div class="item-stats">
                    {item.stats.length > 60 ? item.stats.substring(0, 60) + '...' : item.stats}
                  </div>
                {/if}
              </div>
            {/each}
          {/if}
        </div>
      </section>

      <!-- Logs -->
      <section class="panel">
        <div class="panel-header">
          <h2 class="panel-title">Activity Log</h2>
        </div>
        
        <div class="logs-list font-mono">
          {#if logs.length === 0}
            <div class="empty-state">
              <p>No activity yet</p>
            </div>
          {:else}
            {#each logs as log}
              <div class="log-entry">{log}</div>
            {/each}
          {/if}
        </div>
      </section>
    </div>

    <!-- Message -->
    {#if message}
      <div class="message-bar">
        {message}
      </div>
    {/if}
  </main>
{/if}

<style>
  /* Overlay mode */
  .overlay {
    position: fixed;
    inset: 0;
    background: transparent;
    pointer-events: none;
  }

  /* Main window */
  .main-window {
    min-height: 100vh;
    padding: var(--space-4);
    font-family: var(--font-mono);
  }

  /* Header */
  .header {
    max-width: 900px;
    margin: 0 auto var(--space-5) auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-lg);
  }

  .header-title {
    font-size: var(--text-xl);
    font-weight: 700;
    margin: 0;
  }

  .header-subtitle {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin: var(--space-1) 0 0 0;
  }

  .header-controls {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .status-panel {
    text-align: right;
    font-size: var(--text-sm);
  }

  .status-row {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .status-row + .status-row {
    margin-top: var(--space-1);
  }

  .status-label {
    color: var(--text-muted);
  }

  .status-value {
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .status-value.success {
    color: var(--status-success-text);
  }

  .status-value.error {
    color: var(--status-error-text);
  }

  /* Content grid */
  .content-grid {
    max-width: 900px;
    margin: 0 auto;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-4);
  }

  /* Lists */
  .items-list,
  .logs-list {
    max-height: 400px;
    overflow-y: auto;
  }

  .empty-state {
    padding: var(--space-6);
    text-align: center;
    color: var(--text-muted);
  }

  .empty-state p + p {
    font-size: var(--text-xs);
    margin-top: var(--space-1);
  }

  /* Item rows */
  .item-row {
    padding: var(--space-3);
    border-bottom: 1px solid var(--border-primary);
    border-left: 3px solid var(--border-primary);
  }

  .item-name {
    font-weight: 500;
  }

  .item-meta {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin-top: var(--space-1);
    display: flex;
    gap: var(--space-2);
  }

  .ethereal {
    color: var(--quality-ethereal);
  }

  .unid {
    color: var(--text-muted);
  }

  .item-stats {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin-top: var(--space-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Log entries */
  .log-entry {
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  /* Message bar */
  .message-bar {
    max-width: 900px;
    margin: var(--space-4) auto 0 auto;
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
  }
</style>
