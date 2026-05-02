<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Button, HotkeyInput, Toggle } from '../components';
  import { settingsStore, updaterStore, type HotkeyConfig } from '../stores';
  import { playSound } from '../lib/sound-player';

  // Derived state from settings store
  let soundVolume = $derived(settingsStore.settings.soundVolume);
  let verboseFilterLogging = $derived(settingsStore.settings.verboseFilterLogging);
  let autoAlwaysShowItems = $derived(settingsStore.settings.autoAlwaysShowItems);

  type HotkeyId = 'toggleWindow' | 'editOverlay' | 'revealHidden' | 'lootHistory';
  interface HotkeyRow {
    id: HotkeyId;
    label: string;
    hint: string;
    setter: (h: HotkeyConfig) => void;
  }
  const HOTKEY_ROWS: readonly HotkeyRow[] = [
    {
      id: 'toggleWindow',
      label: 'Toggle window',
      hint: 'Show/hide main window over game',
      setter: (h) => settingsStore.setToggleWindowHotkey(h),
    },
    {
      id: 'editOverlay',
      label: 'Reposition notifications',
      hint: 'Hold to drag the drop-notification anchor on the overlay',
      setter: (h) => settingsStore.setEditOverlayHotkey(h),
    },
    {
      id: 'revealHidden',
      label: 'Reveal hidden items',
      hint: 'Hold to show every item on the ground, including those filtered out by `hide` rules',
      setter: (h) => settingsStore.setRevealHiddenHotkey(h),
    },
    {
      id: 'lootHistory',
      label: 'Loot history',
      hint: 'Toggle the in-game loot log overlay (session drops)',
      setter: (h) => settingsStore.setLootHistoryHotkey(h),
    },
  ];

  // Map id -> live HotkeyConfig from the store. Values stay reactive because
  // the getter is invoked inside a $derived.
  const HOTKEY_GETTERS: Record<HotkeyId, () => HotkeyConfig> = {
    toggleWindow: () => settingsStore.settings.toggleWindowHotkey,
    editOverlay:  () => settingsStore.settings.editOverlayHotkey,
    revealHidden: () => settingsStore.settings.revealHiddenHotkey,
    lootHistory:  () => settingsStore.settings.lootHistoryHotkey,
  };
  let hotkeyValues = $derived(
    Object.fromEntries(
      (Object.keys(HOTKEY_GETTERS) as HotkeyId[]).map((id) => [id, HOTKEY_GETTERS[id]()]),
    ) as Record<HotkeyId, HotkeyConfig>,
  );

  let updaterState = $derived(updaterStore.state);
  let checkDisabled = $derived(
    updaterState.kind === 'checking' ||
    updaterState.kind === 'downloading' ||
    updaterState.kind === 'ready',
  );

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

  function updateStatusText(): string {
    const s = updaterState;
    switch (s.kind) {
      case 'idle':        return '';
      case 'checking':    return 'Checking…';
      case 'up_to_date':  return 'You have the latest version';
      case 'available':   return `Update v${s.latest} available — click the button in the top right`;
      case 'downloading': return `Downloading ${formatBytes(s.downloaded)}`;
      case 'ready':       return 'Ready to install. Click "Restart" in the top right';
      case 'error':
        return s.phase === 'install'
          ? 'Update failed — likely antivirus blocking. Use the "Download manually" button in the top right.'
          : 'Failed to check for updates. Check your connection.';
    }
  }

  function handleVolumeInput(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    settingsStore.setSoundVolume(parseFloat(target.value));
  }

  const UNBOUND: HotkeyConfig = { keyCode: 0, modifiers: 0, display: 'None' };

  function sameChord(a: HotkeyConfig, b: HotkeyConfig): boolean {
    return a.keyCode === b.keyCode && a.modifiers === b.modifiers;
  }

  function isBound(h: HotkeyConfig): boolean {
    return h.keyCode !== 0 || h.modifiers !== 0;
  }

  function handleHotkeyChange(id: HotkeyId, hotkey: HotkeyConfig) {
    if (isBound(hotkey)) {
      for (const row of HOTKEY_ROWS) {
        if (row.id === id) continue;
        if (sameChord(hotkeyValues[row.id], hotkey)) {
          row.setter(UNBOUND);
        }
      }
    }
    HOTKEY_ROWS.find((r) => r.id === id)!.setter(hotkey);
  }

  function handleCheckForUpdates() {
    updaterStore.check(true);
  }

  async function handleOpenAppFolder() {
    try {
      await invoke('open_app_folder');
    } catch (err) {
      console.error('Failed to open app folder:', err);
    }
  }

  function handleVerboseLoggingChange(enabled: boolean) {
    settingsStore.setVerboseFilterLogging(enabled);
  }

  function handleAutoAlwaysShowItemsChange(enabled: boolean) {
    settingsStore.setAutoAlwaysShowItems(enabled);
  }

  let showChangelog = $state(false);
  let changelogHtml = $state('');

  async function handleOpenChangelog() {
    try {
      const md: string = await invoke('get_changelog');
      changelogHtml = renderChangelog(md);
      showChangelog = true;
    } catch (err) {
      console.error('Failed to load changelog:', err);
    }
  }

  function renderChangelog(md: string): string {
    const lines = md.split('\n');
    const out: string[] = [];
    let skipSection = false;
    let inVersion = false;

    for (const line of lines) {
      if (line.startsWith('# ') && !line.startsWith('## ')) continue;

      if (line.startsWith('## ')) {
        if (inVersion) out.push('</section>');
        inVersion = true;
        skipSection = false;
        out.push(`<section class="cl-version">`);
        out.push(`<h2>${line.slice(3)}</h2>`);
        continue;
      }

      if (line.startsWith('### ')) {
        const heading = line.slice(4);
        skipSection = heading === 'Other';
        if (!skipSection) out.push(`<h3>${heading}</h3>`);
        continue;
      }

      if (skipSection) continue;

      if (line.startsWith('- ')) {
        out.push(`<div class="cl-entry">${formatEntry(line.slice(2))}</div>`);
        continue;
      }
    }
    if (inVersion) out.push('</section>');
    return out.join('\n');
  }

  function formatEntry(text: string): string {
    text = text.replace(/^(?:Feat|Fix|Refactor|Perf|Chore|Docs|Style|Build|Ci|Test)(\([^)]+\)):\s*/i, (_, scope) => {
      return `<span class="cl-scope">${scope.slice(1, -1)}</span>`;
    });
    text = text.replace(/\(([0-9a-f]{7})\)$/, '<a class="cl-hash" href="https://github.com/synonymouse/D2MXLUtils/commit/$1" target="_blank">$1</a>');
    return text;
  }
</script>

<section class="tab-content">
  <div class="settings-section">
    <h2 class="section-title">Hotkeys</h2>

    {#each HOTKEY_ROWS as row (row.id)}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{row.label}</span>
          <span class="setting-hint">{@html row.hint.replace(/`([^`]+)`/g, '<code>$1</code>')}</span>
        </div>
        <HotkeyInput
          value={hotkeyValues[row.id]}
          onchange={(h) => handleHotkeyChange(row.id, h)}
        />
      </div>
    {/each}
  </div>

  <div class="settings-section">
    <h2 class="section-title">Sound</h2>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Volume</span>
        <span class="setting-hint">Master volume for drop notification sounds. Set to 0 to silence.</span>
      </div>
      <div class="setting-control">
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={soundVolume}
          oninput={handleVolumeInput}
          class="slider"
          aria-label="Sound volume"
        />
        <span class="setting-value">{Math.round(soundVolume * 100)}%</span>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Test sounds</span>
        <span class="setting-hint">
          Preview each filter sound at the current volume. Filter rules reference them as
          <code>sound1</code>..<code>sound7</code>.
        </span>
      </div>
      <div class="test-buttons">
        {#each [1, 2, 3, 4, 5, 6, 7] as n (n)}
          <Button variant="secondary" size="sm" onclick={() => playSound(n, soundVolume)}>
            {n}
          </Button>
        {/each}
      </div>
    </div>
  </div>

  <div class="settings-section">
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Auto-toggle item highlight (alt) on new game</span>
        <span class="setting-hint">Highlights ground drops automatically without pressing Alt.</span>
      </div>
      <Toggle checked={autoAlwaysShowItems} onchange={handleAutoAlwaysShowItemsChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Verbose filter logging</span>
        <span class="setting-hint">Log per-item filter decisions to d2mxlutils.log. Useful when debugging rules.</span>
      </div>
      <Toggle checked={verboseFilterLogging} onchange={handleVerboseLoggingChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">App data folder</span>
        <span class="setting-hint">Settings, profiles, logs</span>
      </div>
      <div class="update-control">
        <Button variant="secondary" size="sm" onclick={handleOpenAppFolder}>
          Open folder
        </Button>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Current version</span>
        <span class="setting-hint">
          v{__APP_VERSION__}
          <button type="button" class="link-button" onclick={handleOpenChangelog}>Changelog</button>
        </span>
      </div>
      <div class="update-control">
        <Button variant="secondary" size="sm" disabled={checkDisabled} onclick={handleCheckForUpdates}>
          Check for updates
        </Button>
      </div>
    </div>

    {#if updateStatusText()}
      <div class="update-status" class:is-error={updaterState.kind === 'error'}>
        {updateStatusText()}
      </div>
    {/if}
  </div>
</section>

{#if showChangelog}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="changelog-backdrop" role="dialog" aria-modal="true" onkeydown={(e) => e.key === 'Escape' && (showChangelog = false)} onclick={() => (showChangelog = false)}>
    <div class="changelog-modal" onclick={(e) => e.stopPropagation()}>
      <div class="changelog-header">
        <h2 class="changelog-title">Changelog</h2>
        <button type="button" class="changelog-close" onclick={() => (showChangelog = false)}>&times;</button>
      </div>
      <div class="changelog-body" onclick={(e) => {
        const a = (e.target as HTMLElement).closest('a.cl-hash');
        if (a) { e.preventDefault(); invoke('open_external_url', { url: (a as HTMLAnchorElement).href }); }
      }}>
        {@html changelogHtml}
      </div>
    </div>
  </div>
{/if}

<style>
  .setting-control {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .slider {
    width: 160px;
    height: 6px;
    appearance: none;
    background: var(--bg-tertiary);
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .slider::-webkit-slider-thumb {
    appearance: none;
    width: 16px;
    height: 16px;
    background: var(--accent-primary);
    border-radius: var(--radius-full);
    cursor: pointer;
    transition: transform 0.1s ease;
  }

  .slider::-webkit-slider-thumb:hover {
    transform: scale(1.1);
  }

  .slider::-moz-range-thumb {
    width: 16px;
    height: 16px;
    background: var(--accent-primary);
    border: none;
    border-radius: var(--radius-full);
    cursor: pointer;
  }

  .setting-value {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-primary);
    min-width: 50px;
    text-align: right;
  }

  .test-buttons {
    display: flex;
    gap: var(--space-2);
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.95em;
    padding: 0 2px;
    background: var(--bg-tertiary);
    border-radius: var(--radius-sm);
  }

  .update-control {
    display: flex;
    align-items: center;
  }

  .update-status {
    margin-top: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--bg-tertiary);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    color: var(--text-secondary, var(--text-primary));
  }

  .update-status.is-error {
    color: var(--status-error-text);
  }

  .link-button {
    margin-left: var(--space-2);
    padding: 0;
    background: none;
    border: none;
    color: var(--accent-primary);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }

  .link-button:hover {
    opacity: 0.85;
  }

  .changelog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.6);
  }

  .changelog-modal {
    display: flex;
    flex-direction: column;
    width: 92%;
    max-width: 640px;
    max-height: 85vh;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
  }

  .changelog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border-primary);
  }

  .changelog-title {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text-primary);
  }

  .changelog-close {
    padding: 0;
    background: none;
    border: none;
    font-size: var(--text-2xl);
    line-height: 1;
    color: var(--text-muted);
    cursor: pointer;
  }

  .changelog-close:hover {
    color: var(--text-primary);
  }

  .changelog-body {
    padding: var(--space-3) var(--space-4);
    overflow-y: auto;
    font-size: var(--text-sm);
    color: var(--text-secondary);
    line-height: 1.6;
  }

  .changelog-body :global(.cl-version) {
    padding-bottom: var(--space-3);
    margin-bottom: var(--space-3);
    border-bottom: 1px solid var(--border-primary);
  }

  .changelog-body :global(.cl-version:last-child) {
    border-bottom: none;
    margin-bottom: 0;
  }

  .changelog-body :global(h2) {
    margin: 0 0 var(--space-2);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--accent-primary);
  }

  .changelog-body :global(h3) {
    margin: var(--space-2) 0 var(--space-1);
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-primary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .changelog-body :global(.cl-entry) {
    padding: 1px 0 1px var(--space-3);
    color: var(--text-primary);
  }

  .changelog-body :global(.cl-scope) {
    font-family: var(--font-mono);
    font-size: 0.9em;
    color: var(--text-secondary);
    opacity: 0.85;
  }

  .changelog-body :global(.cl-scope::after) {
    content: ':  ';
  }

  .changelog-body :global(.cl-hash) {
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--text-muted);
    text-decoration: underline;
    opacity: 0.5;
    margin-left: var(--space-1);
    cursor: pointer;
  }

  .changelog-body :global(.cl-hash:hover) {
    opacity: 1;
  }
</style>
