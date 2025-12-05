<script lang="ts">
  import { settingsStore } from '../stores';
  
  let currentTheme = $derived(settingsStore.settings.theme);
  let isDark = $derived(currentTheme === 'dark');
  
  function toggleTheme() {
    settingsStore.toggleTheme();
  }
</script>

<button 
  class="theme-toggle" 
  onclick={toggleTheme}
  aria-label={isDark ? 'Switch to light theme' : 'Switch to dark theme'}
  title={isDark ? 'Switch to light theme' : 'Switch to dark theme'}
>
  {#if isDark}
    <!-- Sun icon for switching to light mode -->
    <svg width="20" height="20" viewBox="0 0 24 24" fill="var(--accent-primary)" stroke="var(--accent-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <circle cx="12" cy="12" r="5"/>
      <line x1="12" y1="1" x2="12" y2="3"/>
      <line x1="12" y1="21" x2="12" y2="23"/>
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/>
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/>
      <line x1="1" y1="12" x2="3" y2="12"/>
      <line x1="21" y1="12" x2="23" y2="12"/>
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/>
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>
    </svg>
  {:else}
    <!-- Moon icon for switching to dark mode -->
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>
    </svg>
  {/if}
</button>

<style>
  .theme-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: none;
    border-radius: var(--radius-md);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.2s ease;
    flex-shrink: 0;
  }

  .theme-toggle:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    transform: scale(1.05);
  }

  .theme-toggle:active {
    transform: scale(0.95);
  }

  .theme-toggle svg {
    transition: transform 0.3s ease;
  }

  .theme-toggle:hover svg {
    transform: rotate(15deg);
  }
</style>

