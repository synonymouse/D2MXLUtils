<script lang="ts">
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { onMount } from 'svelte';
  import { MainWindow, OverlayWindow } from './views';
  import { settingsStore } from './stores';

  // Determine which window we're in
  let windowType = $state<'main' | 'overlay' | null>(null);

  onMount(async () => {
    const current = getCurrentWebviewWindow();
    windowType = current.label === 'overlay' ? 'overlay' : 'main';
    
    // Add class to html element for overlay styling
    if (windowType === 'overlay') {
      document.documentElement.classList.add('overlay-mode');
      document.body.style.background = 'transparent';
    }

    // Load settings from backend (applies theme automatically)
    await settingsStore.load();
  });
</script>

{#if windowType === 'overlay'}
  <OverlayWindow />
{:else if windowType === 'main'}
  <MainWindow />
{/if}
