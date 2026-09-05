<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Button, HotkeyInput, Toggle } from '../components';
  import { settingsStore, updaterStore, uniqueStatsDbStore, type HotkeyConfig } from '../stores';

  let verboseFilterLogging = $derived(settingsStore.settings.verboseFilterLogging);
  let liveMatchHighlightDurationMs = $derived(settingsStore.settings.liveMatchHighlightDurationMs);
  let autoAlwaysShowItems = $derived(settingsStore.settings.autoAlwaysShowItems);
  let autoNoPickup = $derived(settingsStore.settings.autoNoPickup);
  let showItemsHiddenIndicator = $derived(settingsStore.settings.showItemsHiddenIndicator);
  let dpsMeterEnabled = $derived(settingsStore.settings.dpsMeter?.enabled ?? false);
  let gameCreateNamePrefix = $derived(settingsStore.settings.gameCreateNamePrefix);
  let gameCreatePassword = $derived(settingsStore.settings.gameCreatePassword);
  let gameCreatePasswordPrefix = $derived(settingsStore.settings.gameCreatePasswordPrefix);
  let gameCreatePasswordUsePrefix = $derived(settingsStore.settings.gameCreatePasswordUsePrefix);
  let gameCreateDescription = $derived(settingsStore.settings.gameCreateDescription);

  const UNBOUND_HOTKEY: HotkeyConfig = { keyCode: 0, modifiers: 0, display: 'None' };

  type HotkeyId =
    | 'toggleWindow'
    | 'editOverlay'
    | 'revealHidden'
    | 'lootHistory'
    | 'itemSearch'
    | 'dpsMeterReset'
    | 'gameCreateAutofill';
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
      label: 'Reposition UI elements',
      hint: 'Hold to drag the element anchor on the overlay',
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
    {
      id: 'itemSearch',
      label: 'Item search',
      hint: 'Open the in-game MXL item database search overlay',
      setter: (h) => settingsStore.setItemSearchHotkey(h),
    },
  ];

  const DPS_HOTKEY_ROWS: readonly HotkeyRow[] = [
    {
      id: 'dpsMeterReset',
      label: 'Reset DPS session',
      hint: 'Clear DPS stats',
      setter: (h) => settingsStore.setDpsMeterResetHotkey(h),
    },
  ];

  const GAME_CREATE_HOTKEY_ROWS: readonly HotkeyRow[] = [
    {
      id: 'gameCreateAutofill',
      label: 'Autofill create-game fields',
      hint: 'Click into the Game Name field first, then press this — types Name, Tab, Password, and Description (if set) for you',
      setter: (h) => settingsStore.setGameCreateAutofillHotkey(h),
    },
  ];

  const HOTKEY_GETTERS: Record<HotkeyId, () => HotkeyConfig> = {
    toggleWindow: () => settingsStore.settings.toggleWindowHotkey,
    editOverlay: () => settingsStore.settings.editOverlayHotkey,
    revealHidden: () => settingsStore.settings.revealHiddenHotkey,
    lootHistory: () => settingsStore.settings.lootHistoryHotkey,
    itemSearch: () => settingsStore.settings.itemSearchHotkey,
    dpsMeterReset: () => settingsStore.settings.dpsMeter?.hotkeyReset ?? UNBOUND_HOTKEY,
    gameCreateAutofill: () => settingsStore.settings.gameCreateAutofillHotkey,
  };
  let hotkeyValues = $derived(
    Object.fromEntries(
      (Object.keys(HOTKEY_GETTERS) as HotkeyId[]).map((id) => [id, HOTKEY_GETTERS[id]()]),
    ) as Record<HotkeyId, HotkeyConfig>,
  );

  function handleDpsMeterEnabledChange(enabled: boolean) {
    settingsStore.setDpsMeterEnabled(enabled);
  }

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
      case 'idle':
        return '';
      case 'checking':
        return 'Checking…';
      case 'up_to_date':
        return 'You have the latest version';
      case 'available':
        return `Update v${s.latest} available — click the button in the top right`;
      case 'downloading':
        return `Downloading ${formatBytes(s.downloaded)}`;
      case 'ready':
        return 'Ready to install. Click "Restart" in the top right';
      case 'error':
        return s.phase === 'install'
          ? 'Update failed — likely antivirus blocking. Use the "Download manually" button in the top right.'
          : 'Failed to check for updates. Check your connection.';
    }
  }

  let uniqueDbState = $derived(uniqueStatsDbStore.state);
  let uniqueDbButtonDisabled = $derived(
    uniqueDbState.kind === 'checking' || uniqueDbState.kind === 'downloading',
  );

  function uniqueDbStatusText(): string {
    const s = uniqueDbState;
    switch (s.kind) {
      case 'idle':
        return '';
      case 'checking':
        return 'Checking…';
      case 'not_downloaded':
        return 'Not downloaded yet — click "Download" to enable roll-range annotations';
      case 'up_to_date':
        return 'Up to date';
      case 'available':
        return 'An updated database is available — click "Download"';
      case 'downloading':
        return 'Downloading…';
      case 'downloaded':
        return 'Downloaded — restart D2MXLUtils to apply';
      case 'error':
        return `Failed: ${s.message}`;
    }
  }

  function uniqueDbButtonLabel(): string {
    const s = uniqueDbState;
    return s.kind === 'not_downloaded' || s.kind === 'available' ? 'Download' : 'Check for update';
  }

  function handleUniqueDbButtonClick() {
    if (uniqueDbState.kind === 'not_downloaded' || uniqueDbState.kind === 'available') {
      uniqueStatsDbStore.download();
    } else {
      uniqueStatsDbStore.check();
    }
  }

  const UNBOUND: HotkeyConfig = { keyCode: 0, modifiers: 0, display: 'None' };

  function sameChord(a: HotkeyConfig, b: HotkeyConfig): boolean {
    return a.keyCode === b.keyCode && a.modifiers === b.modifiers;
  }

  function isBound(h: HotkeyConfig): boolean {
    return h.keyCode !== 0 || h.modifiers !== 0;
  }

  function handleHotkeyChange(id: HotkeyId, hotkey: HotkeyConfig) {
    const allRows = [...HOTKEY_ROWS, ...DPS_HOTKEY_ROWS, ...GAME_CREATE_HOTKEY_ROWS];
    if (isBound(hotkey)) {
      for (const row of allRows) {
        if (row.id === id) continue;
        if (sameChord(hotkeyValues[row.id], hotkey)) {
          row.setter(UNBOUND);
        }
      }
    }
    allRows.find((r) => r.id === id)!.setter(hotkey);
  }

  function handleCheckForUpdates() {
    updaterStore.check(true);
  }

  let refreshGameDataStatus = $state<'idle' | 'refreshing' | 'done' | 'error'>('idle');

  async function handleRefreshGameData() {
    refreshGameDataStatus = 'refreshing';
    try {
      await invoke('refresh_game_data_caches');
      refreshGameDataStatus = 'done';
    } catch (err) {
      console.error('Failed to refresh game data caches:', err);
      refreshGameDataStatus = 'error';
    }
  }

  function refreshGameDataStatusText(): string {
    switch (refreshGameDataStatus) {
      case 'idle':
        return '';
      case 'refreshing':
        return 'Refreshing…';
      case 'done':
        return 'Done — rebuilding from the game live now (or on next attach if D2 isn’t running).';
      case 'error':
        return 'Failed to refresh — check d2mxlutils.log.';
    }
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

  function setLiveMatchHighlightDuration(value: number) {
    const clamped = Math.max(200, Math.min(5000, value));
    settingsStore.set('liveMatchHighlightDurationMs', clamped);
  }

  function handleAutoAlwaysShowItemsChange(enabled: boolean) {
    settingsStore.setAutoAlwaysShowItems(enabled);
  }

  function handleAutoNoPickupChange(enabled: boolean) {
    settingsStore.setAutoNoPickup(enabled);
  }

  function handleShowItemsHiddenIndicatorChange(enabled: boolean) {
    settingsStore.set('showItemsHiddenIndicator', enabled);
  }

  function handleGameCreateNamePrefixInput(e: Event) {
    settingsStore.setGameCreateNamePrefix((e.target as HTMLInputElement).value);
  }

  function handleGameCreatePasswordInput(e: Event) {
    settingsStore.setGameCreatePassword((e.target as HTMLInputElement).value);
  }

  function handleGameCreatePasswordPrefixInput(e: Event) {
    settingsStore.setGameCreatePasswordPrefix((e.target as HTMLInputElement).value);
  }

  function handleGameCreatePasswordUsePrefixChange(enabled: boolean) {
    settingsStore.setGameCreatePasswordUsePrefix(enabled);
  }

  function handleGameCreateDescriptionInput(e: Event) {
    settingsStore.setGameCreateDescription((e.target as HTMLInputElement).value);
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
    text = text.replace(
      /^(?:Feat|Fix|Refactor|Perf|Chore|Docs|Style|Build|Ci|Test)(\([^)]+\)):\s*/i,
      (_, scope) => {
        return `<span class="cl-scope">${scope.slice(1, -1)}</span>`;
      },
    );
    text = text.replace(
      /\(([0-9a-f]{7})\)$/,
      '<a class="cl-hash" href="https://github.com/synonymouse/D2MXLUtils/commit/$1" target="_blank">$1</a>',
    );
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
          <span class="setting-hint">{@html row.hint.replace(/`([^`]+)`/g, '<code>$1</code>')}</span
          >
        </div>
        <HotkeyInput value={hotkeyValues[row.id]} onchange={(h) => handleHotkeyChange(row.id, h)} />
      </div>
    {/each}
  </div>

  <div class="settings-section">
    <h2 class="section-title">DPS Meter</h2>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Show DPS meter</span>
      </div>
      <Toggle checked={dpsMeterEnabled} onchange={handleDpsMeterEnabledChange} />
    </div>

    {#each DPS_HOTKEY_ROWS as row (row.id)}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{row.label}</span>
          <span class="setting-hint">{row.hint}</span>
        </div>
        <HotkeyInput value={hotkeyValues[row.id]} onchange={(h) => handleHotkeyChange(row.id, h)} />
      </div>
    {/each}
  </div>

  <div class="settings-section">
    <h2 class="section-title">Create Game Autofill</h2>

    {#each GAME_CREATE_HOTKEY_ROWS as row (row.id)}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{row.label}</span>
          <span class="setting-hint">{row.hint}</span>
        </div>
        <HotkeyInput value={hotkeyValues[row.id]} onchange={(h) => handleHotkeyChange(row.id, h)} />
      </div>
    {/each}

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Game name prefix</span>
        <span class="setting-hint"
          >Game name = prefix + an auto-incrementing number (not saved between launches)</span
        >
      </div>
      <input
        type="text"
        class="text-input"
        value={gameCreateNamePrefix}
        oninput={handleGameCreateNamePrefixInput}
        placeholder="e.g. MyGame"
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Password auto-increments too</span>
        <span class="setting-hint">Uses the same number as the game name for this run</span>
      </div>
      <Toggle
        checked={gameCreatePasswordUsePrefix}
        onchange={handleGameCreatePasswordUsePrefixChange}
      />
    </div>

    {#if gameCreatePasswordUsePrefix}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Password prefix</span>
        </div>
        <input
          type="text"
          class="text-input"
          value={gameCreatePasswordPrefix}
          oninput={handleGameCreatePasswordPrefixInput}
          placeholder="e.g. pw"
        />
      </div>
    {:else}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Password</span>
        </div>
        <input
          type="text"
          class="text-input"
          value={gameCreatePassword}
          oninput={handleGameCreatePasswordInput}
        />
      </div>
    {/if}

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Description</span>
        <span class="setting-hint">Left as-is if empty</span>
      </div>
      <input
        type="text"
        class="text-input"
        value={gameCreateDescription}
        oninput={handleGameCreateDescriptionInput}
      />
    </div>
  </div>

  <div class="settings-section">
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Auto-toggle item highlight (alt) on new game</span>
        <span class="setting-hint">Highlights ground drops automatically without pressing Alt.</span
        >
      </div>
      <Toggle checked={autoAlwaysShowItems} onchange={handleAutoAlwaysShowItemsChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Show "Items hidden" indicator</span>
        <span class="setting-hint"
          >Shows an on-screen reminder to press Alt when item highlight is off.</span
        >
      </div>
      <Toggle checked={showItemsHiddenIndicator} onchange={handleShowItemsHiddenIndicatorChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Auto-enable /nopickup on new game</span>
        <span class="setting-hint"
          >Prevents accidental pickup; changes in game apply immediately.</span
        >
      </div>
      <Toggle checked={autoNoPickup} onchange={handleAutoNoPickupChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Verbose filter logging</span>
        <span class="setting-hint"
          >Log per-item filter decisions to d2mxlutils.log. Useful when debugging rules.</span
        >
      </div>
      <Toggle checked={verboseFilterLogging} onchange={handleVerboseLoggingChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Show matches highlight duration</span>
        <span class="setting-hint"
          >How long the Loot Filter tab's "Show matches" mode keeps a rule line flashed (0.2-5s).</span
        >
      </div>
      <div class="setting-control">
        <input
          type="range"
          id="live-match-highlight-duration-slider"
          min="200"
          max="5000"
          step="100"
          value={liveMatchHighlightDurationMs}
          oninput={(e) => setLiveMatchHighlightDuration(parseInt(e.currentTarget.value))}
          class="slider"
        />
        <span class="setting-value">{(liveMatchHighlightDurationMs / 1000).toFixed(1)}s</span>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">App data folder</span>
        <span class="setting-hint">Settings, profiles, logs</span>
      </div>
      <div class="update-control">
        <Button variant="secondary" size="sm" onclick={handleOpenAppFolder}>Open folder</Button>
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
        <Button
          variant="secondary"
          size="sm"
          disabled={checkDisabled}
          onclick={handleCheckForUpdates}
        >
          Check for updates
        </Button>
      </div>
    </div>

    {#if updateStatusText()}
      <div class="update-status" class:is-error={updaterState.kind === 'error'}>
        {updateStatusText()}
      </div>
    {/if}

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Unique/set roll-range database</span>
        <span class="setting-hint">
          Adds possible roll ranges to unique/set item stats. Maintainer-built; downloading it skips
          every client crawling the item API themselves.
        </span>
      </div>
      <div class="update-control">
        <Button
          variant="secondary"
          size="sm"
          disabled={uniqueDbButtonDisabled}
          onclick={handleUniqueDbButtonClick}
        >
          {uniqueDbButtonLabel()}
        </Button>
      </div>
    </div>

    {#if uniqueDbStatusText()}
      <div class="update-status" class:is-error={uniqueDbState.kind === 'error'}>
        {uniqueDbStatusText()}
      </div>
    {/if}

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Refresh game data cache</span>
        <span class="setting-hint">
          Rebuilds item/unique/set names and weapon bases from the game. Use this after an MXL patch
          if drops look mislabeled. No restart needed — takes effect immediately if D2 is attached,
          or on next attach otherwise.
        </span>
      </div>
      <div class="update-control">
        <Button
          variant="secondary"
          size="sm"
          disabled={refreshGameDataStatus === 'refreshing'}
          onclick={handleRefreshGameData}
        >
          Refresh
        </Button>
      </div>
    </div>

    {#if refreshGameDataStatusText()}
      <div class="update-status" class:is-error={refreshGameDataStatus === 'error'}>
        {refreshGameDataStatusText()}
      </div>
    {/if}
  </div>
</section>

{#if showChangelog}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="changelog-backdrop"
    role="dialog"
    aria-modal="true"
    onkeydown={(e) => e.key === 'Escape' && (showChangelog = false)}
    onclick={() => (showChangelog = false)}
  >
    <div class="changelog-modal" onclick={(e) => e.stopPropagation()}>
      <div class="changelog-header">
        <h2 class="changelog-title">Changelog</h2>
        <button type="button" class="changelog-close" onclick={() => (showChangelog = false)}
          >&times;</button
        >
      </div>
      <div
        class="changelog-body"
        onclick={(e) => {
          const a = (e.target as HTMLElement).closest('a.cl-hash');
          if (a) {
            e.preventDefault();
            invoke('open_external_url', { url: (a as HTMLAnchorElement).href });
          }
        }}
      >
        {@html changelogHtml}
      </div>
    </div>
  </div>
{/if}

<style>
  .text-input {
    padding: var(--space-1) var(--space-2);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font: inherit;
    min-width: 180px;
  }

  .update-control {
    display: flex;
    align-items: center;
  }

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
