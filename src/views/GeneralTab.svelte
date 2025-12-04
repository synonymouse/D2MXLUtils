<script lang="ts">
  import { Toggle } from '../components';
  import { settingsStore } from '../stores';

  // Derived state from settings store
  let soundEnabled = $derived(settingsStore.settings.soundEnabled);
  let currentTheme = $derived(settingsStore.settings.theme);

  function handleSoundToggle(checked: boolean) {
    settingsStore.setSoundEnabled(checked);
  }

  function handleThemeToggle(checked: boolean) {
    settingsStore.setTheme(checked ? 'dark' : 'light');
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
