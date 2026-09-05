<script lang="ts">
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { invoke } from '@tauri-apps/api/core';
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

    // Desktop-feel: suppress the browser context menu except inside the
    // rules editor, inputs/textareas, and the DSL help content where users
    // legitimately need copy/paste.
    window.addEventListener('contextmenu', (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;
      if (
        target.closest('.cm-editor') ||
        target.closest('input') ||
        target.closest('textarea') ||
        target.closest('.syntax-help') ||
        target.closest('.help-content')
      ) {
        return;
      }
      e.preventDefault();
    });

    // F12: open devtools. The context-menu suppression above (and this
    // app having no menu bar) leaves no other way to reach them — see
    // `open_devtools` (main.rs) for why this is a real command instead of
    // a JS-only affordance. No-ops in release builds (command checks
    // debug_assertions itself), so the shortcut is harmless to leave wired.
    window.addEventListener('keydown', (e: KeyboardEvent) => {
      if (e.key === 'F12') {
        e.preventDefault();
        invoke('open_devtools').catch(() => {});
      }
    });

    // Load settings from backend (applies theme automatically)
    await settingsStore.load();
    // Cross-window sync: each webview has its own store instance, so without
    // this a change in one window would be clobbered by a stale save from the other.
    await settingsStore.initSync();
  });
</script>

{#if windowType === 'overlay'}
  <OverlayWindow />
{:else if windowType === 'main'}
  <MainWindow />
{/if}
