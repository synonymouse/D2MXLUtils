<script lang="ts">
  import { Button, HotkeyInput } from '../components';
  import { settingsStore, type HotkeyConfig } from '../stores';
  import { playSound } from '../lib/sound-player';

  // Derived state from settings store
  let soundVolume = $derived(settingsStore.settings.soundVolume);
  let toggleWindowHotkey = $derived(settingsStore.settings.toggleWindowHotkey);
  let editOverlayHotkey = $derived(settingsStore.settings.editOverlayHotkey);

  function handleVolumeInput(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    settingsStore.setSoundVolume(parseFloat(target.value));
  }

  function handleHotkeyChange(hotkey: HotkeyConfig) {
    settingsStore.setToggleWindowHotkey(hotkey);
  }

  function handleEditOverlayHotkeyChange(hotkey: HotkeyConfig) {
    settingsStore.setEditOverlayHotkey(hotkey);
  }
</script>

<section class="tab-content">
  <div class="settings-section">
    <h2 class="section-title">Hotkeys</h2>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Toggle window</span>
        <span class="setting-hint">Show/hide main window over game</span>
      </div>
      <HotkeyInput value={toggleWindowHotkey} onchange={handleHotkeyChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Reposition notifications</span>
        <span class="setting-hint">Hold to drag the drop-notification anchor on the overlay</span>
      </div>
      <HotkeyInput value={editOverlayHotkey} onchange={handleEditOverlayHotkeyChange} />
    </div>
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
          <code>sound1</code>..<code>sound6</code>.
        </span>
      </div>
      <div class="test-buttons">
        {#each [1, 2, 3, 4, 5, 6] as n (n)}
          <Button variant="secondary" size="sm" onclick={() => playSound(n, soundVolume)}>
            {n}
          </Button>
        {/each}
      </div>
    </div>
  </div>
</section>

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
</style>
