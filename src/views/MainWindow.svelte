<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { onMount } from 'svelte';
  import { Tabs } from '../components';
  import { windowState, type WindowState } from '../stores';
  import { GeneralTab, LootFilterTab, NotificationsTab } from './index';

  // Scanner and game status from backend
  let scannerStatus = $state<'stopped' | 'starting' | 'running' | 'stopping' | 'error'>('stopped');
  let gameStatus = $state<'unknown' | 'ingame' | 'menu'>('unknown');
  
  // Active tab
  let activeTab = $state('general');
  
  const tabs = [
    { id: 'general', label: 'General' },
    { id: 'lootfilter', label: 'Loot Filter' },
    { id: 'notifications', label: 'Notifications' }
  ];

  function getStatusColor(status: string): string {
    switch (status) {
      case 'running': return 'var(--status-success-text)';
      case 'starting':
      case 'stopping': return 'var(--status-warning-text)';
      case 'error': return 'var(--status-error-text)';
      default: return 'var(--text-muted)';
    }
  }

  function getGameStatusText(): string {
    switch (gameStatus) {
      case 'ingame': return 'In Game';
      case 'menu': return 'Menu';
      default: return 'Not Found';
    }
  }

  /** Save current window position and size */
  async function saveWindowState() {
    try {
      const window = getCurrentWebviewWindow();
      const factor = await window.scaleFactor();
      const position = await window.outerPosition();
      const size = await window.outerSize();
      const maximized = await window.isMaximized();

      const state: WindowState = {
        x: Math.round(position.x / factor),
        y: Math.round(position.y / factor),
        width: Math.round(size.width / factor),
        height: Math.round(size.height / factor),
        maximized,
      };

      await windowState.save('main', state);
    } catch (error) {
      console.error('[MainWindow] Failed to save window state:', error);
    }
  }

  /** Restore window position and size from saved state */
  async function restoreWindowState() {
    try {
      const state = await windowState.load('main');
      if (!state) return;

      const window = getCurrentWebviewWindow();

      // Restore position and size
      await window.setPosition({ type: 'Logical', x: state.x, y: state.y });
      await window.setSize({ type: 'Logical', width: state.width, height: state.height });

      // Restore maximized state
      if (state.maximized) {
        await window.maximize();
      }

      console.log('[MainWindow] Restored window state:', state);
    } catch (error) {
      console.error('[MainWindow] Failed to restore window state:', error);
    }
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];

    // Restore window state
    restoreWindowState();

    // Listen for scanner status
    listen<string>('scanner-status', (event) => {
      scannerStatus = event.payload as typeof scannerStatus;
    }).then(u => unlisteners.push(u));

    // Listen for game status
    listen<string>('game-status', (event) => {
      gameStatus = event.payload as typeof gameStatus;
    }).then(u => unlisteners.push(u));

    // Get initial scanner status
    invoke('get_scanner_status').then((running: unknown) => {
      if (running) {
        scannerStatus = 'running';
      }
    });

    // Save window state on close
    const window = getCurrentWebviewWindow();
    window.onCloseRequested(async () => {
      await saveWindowState();
    }).then(u => unlisteners.push(u));

    // Also save window state periodically when moved/resized
    let saveTimeout: ReturnType<typeof setTimeout> | null = null;
    const debouncedSave = () => {
      if (saveTimeout) clearTimeout(saveTimeout);
      saveTimeout = setTimeout(saveWindowState, 1000);
    };

    window.onMoved(debouncedSave).then(u => unlisteners.push(u));
    window.onResized(debouncedSave).then(u => unlisteners.push(u));

    return () => {
      if (saveTimeout) clearTimeout(saveTimeout);
      unlisteners.forEach(u => u());
    };
  });
</script>

<main class="main-window">
  <!-- Header with status -->
  <header class="header">
    <div class="brand">
      <h1 class="title">D2MXL<span class="accent">Utils</span></h1>
      <span class="version">v1.2.0</span>
    </div>
    
    <div class="status-bar">
      <div class="status-item">
        <span class="status-label">Scanner</span>
        <span class="status-value" style:color={getStatusColor(scannerStatus)}>
          {scannerStatus.toUpperCase()}
        </span>
      </div>
      <div class="status-divider"></div>
      <div class="status-item">
        <span class="status-label">Diablo II</span>
        <span class="status-value" style:color={gameStatus === 'ingame' ? 'var(--status-success-text)' : 'var(--text-muted)'}>
          {getGameStatusText()}
        </span>
      </div>
    </div>
  </header>

  <!-- Main content with tabs -->
  <div class="content">
    <Tabs {tabs} bind:activeTab>
      {#snippet children(tab)}
        {#if tab === 'general'}
          <GeneralTab />
        {:else if tab === 'lootfilter'}
          <LootFilterTab />
        {:else if tab === 'notifications'}
          <NotificationsTab />
        {/if}
      {/snippet}
    </Tabs>
  </div>

  <!-- Footer -->
  <footer class="footer">
    <span class="footer-text">Made with ❤️ by synonymouse</span>
  </footer>
</main>

<style>
  .main-window {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    background: var(--bg-primary);
  }

  /* Header */
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border-primary);
  }

  .brand {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }

  .title {
    font-family: var(--font-mono);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    letter-spacing: -0.5px;
  }

  .accent {
    color: var(--accent-primary);
  }

  .version {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .status-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-3);
    background: var(--bg-tertiary);
    border-radius: var(--radius-lg);
  }

  .status-item {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .status-label {
    font-size: 10px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .status-value {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .status-divider {
    width: 1px;
    height: 28px;
    background: var(--border-primary);
  }

  /* Content */
  .content {
    flex: 1;
    padding: var(--space-3) var(--space-4);
    overflow-y: auto;
  }

  /* Footer */
  .footer {
    padding: var(--space-1) var(--space-4) var(--space-2);
    background: var(--bg-secondary);
    border-top: 1px solid var(--border-primary);
    text-align: right;
  }

  .footer-text {
    font-size: var(--text-xs);
    color: var(--text-muted);
    line-height: 1;
  }
</style>
