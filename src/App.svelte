<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  let status = $state("stopped");
  let message = $state("");

  async function toggleScanner() {
    try {
      if (status === "stopped") {
        message = await invoke('start_scanner');
      } else {
        message = await invoke('stop_scanner');
      }
    } catch (e) {
      message = `Error: ${e}`;
    }
  }

  onMount(() => {
    let unlisten: () => void;

    // Listen for status changes from backend
    listen<string>('scanner-status', (event) => {
      status = event.payload;
      message = `Event received: ${status}`;
    }).then(u => unlisten = u);

    return () => {
      if (unlisten) unlisten();
    };
  });
</script>

<main class="min-h-screen bg-slate-900 text-slate-100 flex items-center justify-center">
  <div
    class="rounded-xl border border-slate-700 bg-slate-800/80 px-8 py-6 shadow-xl shadow-black/40 space-y-4 text-center"
  >
    <h1 class="text-2xl font-semibold tracking-tight">
      D2MXLUtils Drop Notifier
    </h1>
    
    <div class="flex flex-col gap-2 items-center">
      <div class="text-lg">
        Status: 
        <span class={status === 'running' ? 'text-emerald-400' : 'text-slate-400'}>
          {status.toUpperCase()}
        </span>
      </div>
      
      {#if message}
        <p class="text-xs text-slate-500 font-mono">{message}</p>
      {/if}

      <button
        class="mt-4 inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium text-slate-900 shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-900 {status === 'running' ? 'bg-red-500 hover:bg-red-400 focus-visible:ring-red-400/80' : 'bg-emerald-500 hover:bg-emerald-400 focus-visible:ring-emerald-400/80'}"
        onclick={toggleScanner}
      >
        {status === 'running' ? 'Stop Scanner' : 'Start Scanner'}
      </button>
    </div>
  </div>
</main>
