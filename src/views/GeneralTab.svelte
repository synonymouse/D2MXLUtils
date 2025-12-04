<script lang="ts">
  import { Toggle, HotkeyInput } from '../components';
  import { settingsStore, type HotkeyConfig } from '../stores';

  // Derived state from settings store
  let soundEnabled = $derived(settingsStore.settings.soundEnabled);
  let currentTheme = $derived(settingsStore.settings.theme);
  let toggleWindowHotkey = $derived(settingsStore.settings.toggleWindowHotkey);

  function handleSoundToggle(checked: boolean) {
    settingsStore.setSoundEnabled(checked);
  }

  function handleThemeToggle(checked: boolean) {
    settingsStore.setTheme(checked ? 'dark' : 'light');
  }

  function handleHotkeyChange(hotkey: HotkeyConfig) {
    settingsStore.setToggleWindowHotkey(hotkey);
  }
</script>

<section class="tab-content">
  <div class="settings-section">
    <h2 class="section-title">Appearance</h2>
    
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Dark theme</span>
        <span class="setting-hint">Use dark color scheme</span>
      </div>
      <Toggle checked={currentTheme === 'dark'} onchange={handleThemeToggle} />
    </div>
  </div>

  <div class="settings-section">
    <h2 class="section-title">Hotkeys</h2>
    
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Toggle window</span>
        <span class="setting-hint">Show/hide main window over game</span>
      </div>
      <HotkeyInput value={toggleWindowHotkey} onchange={handleHotkeyChange} />
    </div>
  </div>

  <div class="settings-section">
    <h2 class="section-title">Sound</h2>
    
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Enable sounds</span>
        <span class="setting-hint">Play sound effects for item drops</span>
      </div>
      <Toggle checked={soundEnabled} onchange={handleSoundToggle} />
    </div>
  </div>
</section>
