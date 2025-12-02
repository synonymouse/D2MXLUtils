<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { onMount } from 'svelte';

  interface ItemDrop {
    unit_id: number;
    class: number;
    quality: string;
    name: string;
    stats: string;
    is_ethereal: boolean;
    is_identified: boolean;
  }

  let scannerStatus = "stopped";
  let gameStatus = "unknown";
  let message = "";
  let items: ItemDrop[] = [];
  let logs: string[] = [];

  // True when running in the dedicated overlay window (label = "overlay")
  let isOverlay = false;

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

  function getQualityColor(quality: string): string {
    switch (quality) {
      case 'Unique': return 'color: #fbbf24; border-color: #f59e0b;';
      case 'Set': return 'color: #4ade80; border-color: #22c55e;';
      case 'Rare': return 'color: #facc15; border-color: #eab308;';
      case 'Magic': return 'color: #60a5fa; border-color: #3b82f6;';
      case 'Crafted': return 'color: #fb923c; border-color: #f97316;';
      case 'Superior': return 'color: #cbd5e1; border-color: #94a3b8;';
      case 'Normal': return 'color: #94a3b8; border-color: #64748b;';
      default: return 'color: #64748b; border-color: #475569;';
    }
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
<main style="position: fixed; inset: 0; background: transparent; color: #e2e8f0; font-family: monospace; pointer-events: none;">
  <div style="position: absolute; bottom: 24px; right: 24px; display: flex; flex-direction: column; gap: 8px; align-items: flex-end;">
    {#each items as item}
      <div
        style="
          pointer-events: auto;
          padding: 8px 12px;
          border-radius: 8px;
          background: rgba(15,23,42,0.8);
          border: 1px solid rgba(148, 163, 184, 0.6);
          max-width: 320px;
          box-shadow: 0 10px 40px rgba(0,0,0,0.7);
        "
      >
        <div style="font-size: 13px; font-weight: 600; {getQualityColor(item.quality)}">
          {item.name}
        </div>
        <div style="font-size: 11px; color: #9ca3af; margin-top: 2px;">
          {item.quality}
          {#if item.is_ethereal}<span style="color: #22d3ee; margin-left: 4px;">ETH</span>{/if}
          {#if !item.is_identified}<span style="color: #9ca3af; margin-left: 4px;">[UNID]</span>{/if}
        </div>
        {#if item.stats}
          <div style="font-size: 11px; color: #6b7280; margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
            {item.stats.substring(0, 80)}{item.stats.length > 80 ? '...' : ''}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</main>
{:else}
<main style="min-height: 100vh; background: #0f172a; color: #e2e8f0; padding: 16px; font-family: monospace;">
  <!-- Header -->
  <div style="max-width: 900px; margin: 0 auto 24px auto;">
    <div style="display: flex; align-items: center; justify-content: space-between; background: rgba(30, 41, 59, 0.8); border-radius: 12px; border: 1px solid #334155; padding: 16px;">
      <div>
        <h1 style="font-size: 20px; font-weight: bold; margin: 0;">
          D2MXLUtils <span style="color: #34d399;">Drop Notifier</span>
        </h1>
        <p style="font-size: 12px; color: #64748b; margin: 4px 0 0 0;">MedianXL Item Scanner</p>
      </div>
      
      <div style="display: flex; align-items: center; gap: 16px;">
        <!-- Status -->
        <div style="text-align: right; font-size: 14px;">
          <div>
            <span style="color: #64748b;">Scanner:</span>
            <span style="color: {scannerStatus === 'running' ? '#34d399' : scannerStatus === 'error' ? '#f87171' : '#64748b'};">
              {scannerStatus.toUpperCase()}
            </span>
          </div>
          <div style="margin-top: 4px;">
            <span style="color: #64748b;">Game:</span>
            <span style="color: {gameStatus === 'ingame' ? '#34d399' : '#64748b'};">
              {gameStatus === 'ingame' ? 'IN GAME' : gameStatus === 'menu' ? 'MENU' : 'UNKNOWN'}
            </span>
          </div>
        </div>
        
        <!-- Button -->
        <button
          style="padding: 8px 16px; border-radius: 8px; font-size: 14px; font-weight: 500; border: none; cursor: pointer; background: {scannerStatus === 'running' || scannerStatus === 'starting' ? '#dc2626' : '#059669'}; color: white;"
          on:click={toggleScanner}
        >
          {scannerStatus === 'running' || scannerStatus === 'starting' ? 'Stop' : 'Start'}
        </button>
      </div>
    </div>
  </div>

  <!-- Content grid -->
  <div style="max-width: 900px; margin: 0 auto; display: grid; grid-template-columns: 1fr 1fr; gap: 16px;">
    <!-- Items list -->
    <div style="background: rgba(30, 41, 59, 0.8); border-radius: 12px; border: 1px solid #334155; overflow: hidden;">
      <div style="display: flex; justify-content: space-between; align-items: center; padding: 12px 16px; border-bottom: 1px solid #334155; background: rgba(30, 41, 59, 0.5);">
        <h2 style="font-size: 14px; font-weight: 600; color: #cbd5e1; margin: 0;">Found Items ({items.length})</h2>
        <button 
          style="font-size: 12px; padding: 4px 8px; border-radius: 4px; background: #334155; color: #94a3b8; border: none; cursor: pointer;"
          on:click={clearItems}
        >
          Clear
        </button>
      </div>
      
      <div style="max-height: 400px; overflow-y: auto;">
        {#if items.length === 0}
          <div style="padding: 32px; text-align: center; color: #475569;">
            <p>No items found yet</p>
            <p style="font-size: 12px; margin-top: 4px;">Items will appear here when detected in game</p>
          </div>
        {:else}
          {#each items as item}
            <div style="padding: 12px; border-bottom: 1px solid rgba(51, 65, 85, 0.5); border-left: 2px solid; {getQualityColor(item.quality)}">
              <div style="font-weight: 500;">{item.name}</div>
              <div style="font-size: 12px; color: #64748b; margin-top: 2px;">
                {item.quality}
                {#if item.is_ethereal}<span style="color: #22d3ee; margin-left: 4px;">ETH</span>{/if}
                {#if !item.is_identified}<span style="color: #64748b; margin-left: 4px;">[UNID]</span>{/if}
              </div>
              {#if item.stats}
                <div style="font-size: 11px; color: #475569; margin-top: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                  {item.stats.substring(0, 60)}{item.stats.length > 60 ? '...' : ''}
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>

    <!-- Logs -->
    <div style="background: rgba(30, 41, 59, 0.8); border-radius: 12px; border: 1px solid #334155; overflow: hidden;">
      <div style="padding: 12px 16px; border-bottom: 1px solid #334155; background: rgba(30, 41, 59, 0.5);">
        <h2 style="font-size: 14px; font-weight: 600; color: #cbd5e1; margin: 0;">Activity Log</h2>
      </div>
      
      <div style="max-height: 400px; overflow-y: auto; padding: 8px; font-size: 12px;">
        {#if logs.length === 0}
          <div style="padding: 16px; text-align: center; color: #475569;">
            No activity yet
          </div>
        {:else}
          {#each logs as log}
            <div style="padding: 4px 8px; color: #64748b;">
              {log}
            </div>
          {/each}
        {/if}
      </div>
    </div>
  </div>

  <!-- Message -->
  {#if message}
    <div style="max-width: 900px; margin: 16px auto 0 auto;">
      <div style="font-size: 12px; color: #475569; background: rgba(30, 41, 59, 0.5); border-radius: 4px; padding: 8px 12px; border: 1px solid #334155;">
        {message}
      </div>
    </div>
  {/if}
</main>
{/if}
