<script lang="ts">
  import { settingsStore } from '../stores';
  import { Toggle } from '../components';

  // Local reactive bindings to store values
  let duration = $derived(settingsStore.settings.notificationDuration);
  let fontSize = $derived(settingsStore.settings.notificationFontSize);
  let opacity = $derived(settingsStore.settings.notificationOpacity);
  let compactName = $derived(settingsStore.settings.compactName);

  function setDuration(value: number) {
    const clamped = Math.max(1000, Math.min(30000, value));
    settingsStore.set('notificationDuration', clamped);
  }

  function setFontSize(value: number) {
    const clamped = Math.max(10, Math.min(24, value));
    settingsStore.set('notificationFontSize', clamped);
  }

  function setOpacity(value: number) {
    const clamped = Math.max(0, Math.min(1, value));
    settingsStore.set('notificationOpacity', clamped);
  }

  function setCompactName(value: boolean) {
    settingsStore.set('compactName', value);
  }
</script>

<section class="tab-content">
  <div class="settings-section">
    <h2 class="section-title">Notification Settings</h2>
    <p class="section-description">
      Customize how item drop notifications appear in the overlay.
    </p>

    <div class="settings-grid">
      <!-- Duration -->
      <div class="setting-row">
        <div class="setting-info">
          <label class="setting-label" for="duration">Display Duration</label>
          <span class="setting-hint">How long notifications stay visible (1-30 seconds)</span>
        </div>
        <div class="setting-control">
          <input
            type="range"
            id="duration-slider"
            min="1000"
            max="30000"
            step="500"
            value={duration}
            oninput={(e) => setDuration(parseInt(e.currentTarget.value))}
            class="slider"
          />
          <span class="setting-value">{(duration / 1000).toFixed(1)}s</span>
        </div>
      </div>

      <!-- Font Size -->
      <div class="setting-row">
        <div class="setting-info">
          <label class="setting-label" for="font-size">Font Size</label>
          <span class="setting-hint">Text size for notifications (10-24 px)</span>
        </div>
        <div class="setting-control">
          <input
            type="range"
            id="font-size-slider"
            min="10"
            max="24"
            step="1"
            value={fontSize}
            oninput={(e) => setFontSize(parseInt(e.currentTarget.value))}
            class="slider"
          />
          <span class="setting-value">{fontSize}px</span>
        </div>
      </div>

      <!-- Opacity -->
      <div class="setting-row">
        <div class="setting-info">
          <label class="setting-label" for="opacity">Background Opacity</label>
          <span class="setting-hint">Transparency of notification background (0-100%)</span>
        </div>
        <div class="setting-control">
          <input
            type="range"
            id="opacity-slider"
            min="0"
            max="1"
            step="0.05"
            value={opacity}
            oninput={(e) => setOpacity(parseFloat(e.currentTarget.value))}
            class="slider"
          />
          <span class="setting-value">{Math.round(opacity * 100)}%</span>
        </div>
      </div>

      <!-- Compact name -->
      <div class="setting-row">
        <div class="setting-info">
          <label class="setting-label" for="compact-name">Compact name</label>
          <span class="setting-hint">
            Hide unique/set name line for Set/TU/SU/SSU/SSSU drops, show only base type.
            Rules with the <code>stat</code> flag ignore this.
          </span>
        </div>
        <div class="setting-control">
          <Toggle
            id="compact-name"
            checked={compactName}
            onchange={setCompactName}
          />
        </div>
      </div>
    </div>
  </div>

  <!-- Preview -->
  <div class="preview-section">
    <h3 class="preview-title">Preview</h3>
    <div class="preview-container">
      <div
        class="preview-notification"
        style:font-size="{fontSize}px"
        style:background-color="rgba(0, 0, 0, {opacity})"
      >
        <div class="preview-name" style:color="var(--quality-unique)">
          {#if compactName}Sacred Armor{:else}Tyrael's Might SU{/if}<span class="preview-badges">
            <span class="preview-eth">ETH</span>
          </span>
        </div>
        {#if !compactName}
          <div class="preview-base">Sacred Armor</div>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  .tab-content {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .settings-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .section-title {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .section-description {
    font-size: var(--text-sm);
    color: var(--text-muted);
    margin: 0;
  }

  .settings-grid {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-3);
    background: var(--bg-secondary);
    border-radius: var(--radius-md);
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .setting-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-primary);
  }

  .setting-hint {
    font-size: var(--text-xs);
    color: var(--text-muted);
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

  /* Preview */
  .preview-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .preview-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--text-muted);
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .preview-container {
    display: flex;
    justify-content: flex-start;
    padding: var(--space-4);
    background: repeating-linear-gradient(
      45deg,
      var(--bg-tertiary),
      var(--bg-tertiary) 10px,
      var(--bg-secondary) 10px,
      var(--bg-secondary) 20px
    );
    border-radius: var(--radius-md);
    min-height: 100px;
  }

  .preview-notification {
    font-family: var(--font-mono);
    padding: var(--space-2) var(--space-3);
    max-width: 300px;
  }

  .preview-name {
    font-weight: 600;
    line-height: 1.3;
  }

  .preview-base {
    margin-top: var(--space-1);
    font-size: 0.85em;
    color: var(--text-muted);
    line-height: 1.3;
  }

  .preview-badges {
    margin-left: var(--space-2);
    font-size: 0.85em;
    font-weight: 400;
  }

  .preview-eth {
    color: var(--quality-ethereal);
  }

  .setting-hint code {
    background: var(--bg-tertiary);
    padding: 0 4px;
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.95em;
  }
</style>
