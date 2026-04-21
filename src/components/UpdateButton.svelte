<script lang="ts">
  import { updaterStore } from '../stores';

  let state = $derived(updaterStore.state);

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

  function handleClick() {
    if (state.kind === 'available') {
      updaterStore.install();
    } else if (state.kind === 'ready') {
      updaterStore.restart();
    }
  }
</script>

{#if state.kind === 'available'}
  <button class="update-pill available" onclick={handleClick}>
    <span class="dot" aria-hidden="true"></span>
    Update v{state.latest}
  </button>
{:else if state.kind === 'downloading'}
  <button class="update-pill downloading" disabled>
    <span class="shimmer" aria-hidden="true"></span>
    <span class="label">Downloading {formatBytes(state.downloaded)}</span>
  </button>
{:else if state.kind === 'ready'}
  <button class="update-pill ready" onclick={handleClick}>
    Restart
  </button>
{/if}

<style>
  .update-pill {
    position: relative;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    height: 36px;
    padding: 0 14px;
    border: 1px solid var(--accent-primary);
    border-radius: var(--radius-md);
    background: var(--accent-primary);
    color: #1a1a1a;
    font-family: inherit;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.2px;
    cursor: pointer;
    transition: background 0.15s ease, transform 0.05s ease;
  }

  .update-pill:hover:not(:disabled) {
    background: var(--accent-primary-hover);
  }

  .update-pill:active:not(:disabled) {
    transform: translateY(1px);
  }

  .update-pill:disabled {
    cursor: default;
  }

  /* available — subtle pulse so the user notices it */
  .update-pill.available {
    animation: pulse 2.2s ease-in-out infinite;
  }

  .update-pill.available .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #1a1a1a;
    box-shadow: 0 0 0 0 rgba(0, 0, 0, 0.4);
    animation: dot-pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { box-shadow: 0 0 0 0 var(--accent-primary-muted); }
    50%      { box-shadow: 0 0 0 4px var(--accent-primary-muted); }
  }

  @keyframes dot-pulse {
    0%, 100% { opacity: 0.6; }
    50%      { opacity: 1; }
  }

  /* downloading — indeterminate shimmer sliding across the pill */
  .update-pill.downloading {
    background: var(--accent-primary-muted);
    color: var(--text-primary);
    border-color: var(--accent-primary);
  }

  .update-pill.downloading .label {
    position: relative;
    z-index: 1;
    font-variant-numeric: tabular-nums;
  }

  .update-pill.downloading .shimmer {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      90deg,
      transparent 0%,
      var(--accent-primary) 50%,
      transparent 100%
    );
    opacity: 0.45;
    transform: translateX(-100%);
    animation: shimmer 1.4s linear infinite;
  }

  @keyframes shimmer {
    from { transform: translateX(-100%); }
    to   { transform: translateX(100%); }
  }

  /* ready — solid, confident */
  .update-pill.ready {
    background: var(--accent-primary);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.15);
  }
</style>
