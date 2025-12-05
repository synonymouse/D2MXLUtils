<script lang="ts">
  import { RulesEditor, type ValidationResult } from "../editor";
  import { Button, ProfileSelector } from "../components";
  import { settingsStore } from "../stores";

  // Default example filter for new/empty profiles
  const DEFAULT_FILTER = `# D2MXLUtils Loot Filter
# Lines starting with # are comments

# Notify on unique items with gold color and sound
"." unique gold sound1

# Notify on set items with green color
"." set lime sound2

# Hide normal/low quality items
"." normal hide
"." low hide

# Rings with +skills - show stats
"Ring$" rare {Skills} lime sound2 stat

# Ethereal sacred items
"." sacred eth gold sound1 name

# All runes
"Rune$" gold sound3 name
`;

  let dslText = $state(DEFAULT_FILTER);
  let selectedProfile = $state(settingsStore.settings.activeProfile || "");
  let validationStatus = $state<"idle" | "valid" | "error">("idle");
  let errorCount = $state(0);
  let ruleCount = $state(0);
  let isSaving = $state(false);
  let hasUnsavedChanges = $state(false);

  /**
   * Handle validation results from the editor's linter
   */
  function handleValidation(result: ValidationResult) {
    if (result.errors.length > 0) {
      validationStatus = "error";
      errorCount = result.errors.length;
    } else {
      validationStatus = "valid";
      ruleCount = result.ruleCount;
    }
  }

  /**
   * Handle editor content changes
   */
  function handleChange(_newValue: string) {
    // Reset status when content changes (will be updated by linter after debounce)
    validationStatus = "idle";
    hasUnsavedChanges = true;
  }

  /**
   * Handle Ctrl+S save shortcut
   */
  async function handleSave(newValue: string) {
    dslText = newValue;
    // Trigger profile save via ProfileSelector
    const profileSelector = document.querySelector('.profile-selector button[class*="primary"]') as HTMLButtonElement | null;
    profileSelector?.click();
  }

  /**
   * Handle profile load from ProfileSelector
   */
  function handleProfileLoad(name: string, dslSource: string) {
    dslText = dslSource || DEFAULT_FILTER;
    selectedProfile = name;
    hasUnsavedChanges = false;
    validationStatus = "idle";
    
    // Update settings with active profile
    if (name) {
      settingsStore.set('activeProfile', name);
    }
  }

  /**
   * Handle profile selection change
   */
  function handleProfileSelect(_profile: { name: string } | null) {
    hasUnsavedChanges = false;
  }

  /**
   * Get current DSL for saving
   */
  function getCurrentDsl(): string {
    return dslText;
  }

  /**
   * Handle save completion
   */
  function handleSaveComplete() {
    hasUnsavedChanges = false;
    console.log("[LootFilterTab] Profile saved successfully");
  }

  /**
   * Reset to default filter (for current profile)
   */
  function resetToDefault() {
    if (hasUnsavedChanges && !confirm("Discard unsaved changes?")) {
      return;
    }
    dslText = DEFAULT_FILTER;
    validationStatus = "idle";
    errorCount = 0;
    ruleCount = 0;
    hasUnsavedChanges = true;
  }
</script>

<section class="loot-filter-tab">
  <header class="tab-header">
    <div class="header-left">
      <h2>Loot Filter Editor</h2>
      <span class="status-badge" class:valid={validationStatus === "valid"} class:error={validationStatus === "error"}>
        {#if validationStatus === "valid"}
          ✓ {ruleCount} {ruleCount === 1 ? "rule" : "rules"}
        {:else if validationStatus === "error"}
          ✗ {errorCount} {errorCount === 1 ? "error" : "errors"}
        {:else}
          —
        {/if}
      </span>
      {#if hasUnsavedChanges}
        <span class="unsaved-indicator" title="Unsaved changes">●</span>
      {/if}
    </div>

    <div class="header-actions">
      <ProfileSelector
        bind:selectedProfile
        onselect={handleProfileSelect}
        onload={handleProfileLoad}
        getCurrentDsl={getCurrentDsl}
        onsave={handleSaveComplete}
        canSave={validationStatus === "valid"}
      />
      <Button
        variant="ghost"
        size="sm"
        onclick={resetToDefault}
        disabled={isSaving}
      >
        Reset
      </Button>
    </div>
  </header>

  <div class="editor-container">
    <RulesEditor
      bind:value={dslText}
      onchange={handleChange}
      onsave={handleSave}
      onvalidate={handleValidation}
    />
  </div>

  <div class="syntax-help">
    <details>
      <summary>Syntax Reference</summary>
      <div class="help-content">
        <p>Each line follows the format:</p>
        <code>"Pattern" [quality] [tier] [eth] &#123;stat&#125; [color] [sound] [name] [stat]</code>

        <div class="help-columns">
          <div class="help-column">
            <h4>Quality</h4>
            <ul>
              <li><span class="kw-unique">unique</span></li>
              <li><span class="kw-set">set</span></li>
              <li><span class="kw-rare">rare</span></li>
              <li><span class="kw-magic">magic</span>, craft</li>
              <li>normal, low, superior</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Tier</h4>
            <ul>
              <li><span class="kw-tier">sacred</span>, angelic, master</li>
              <li>0, 1, 2, 3, 4</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Colors</h4>
            <ul>
              <li><span class="kw-color">gold</span>, lime, red, blue</li>
              <li>white, yellow, orange, pink</li>
              <li>hide, show</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Sounds</h4>
            <ul>
              <li><span class="kw-sound">sound1</span> - sound6</li>
              <li>sound_none</li>
            </ul>
          </div>
        </div>

        <p class="help-note">
          <strong>eth</strong> - match ethereal items only<br />
          <strong>name</strong> - display item name<br />
          <strong>stat</strong> - display item stats<br />
          <strong>&#123;pattern&#125;</strong> - match stat text (regex)
        </p>
      </div>
    </details>
  </div>
</section>

<style>
  .loot-filter-tab {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0; /* Important: allows flex child to shrink below content size */
    gap: var(--space-3, 12px);
    overflow: hidden;
  }

  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-shrink: 0;
    flex-wrap: wrap;
    gap: var(--space-2, 8px);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3, 12px);
  }

  .tab-header h2 {
    margin: 0;
    font-size: var(--text-lg, 18px);
    font-weight: 600;
    color: var(--text-primary);
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
  }

  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1, 4px);
    padding: 4px 10px;
    border-radius: var(--radius-full, 9999px);
    font-size: var(--text-xs, 12px);
    font-weight: 500;
    /* Neutral state */
    background: color-mix(in srgb, var(--text-secondary) 10%, transparent);
    color: var(--text-secondary);
  }

  .status-badge.valid {
    /* Use theme status colors for better contrast in light & dark themes */
    background: color-mix(in srgb, var(--status-success-text) 16%, transparent);
    color: var(--status-success-text);
  }

  .status-badge.error {
    background: color-mix(in srgb, var(--status-error-text) 18%, transparent);
    color: var(--status-error-text);
  }

  .unsaved-indicator {
    color: var(--accent);
    font-size: 12px;
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .editor-container {
    flex: 1;
    min-height: 0; /* Important: allows flex child to shrink below content size */
    overflow: hidden;
  }

  .syntax-help {
    flex-shrink: 0;
  }

  .syntax-help details {
    background: var(--bg-tertiary, #12121a);
    border: 1px solid var(--border, #2a2a35);
    border-radius: var(--radius-md, 8px);
  }

  .syntax-help summary {
    padding: var(--space-2, 8px) var(--space-3, 12px);
    font-size: var(--text-sm, 13px);
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
  }

  .syntax-help summary:hover {
    color: var(--text-primary);
  }

  .help-content {
    padding: var(--space-3, 12px);
    padding-top: 0;
    font-size: var(--text-sm, 13px);
    color: var(--text-secondary);
  }

  .help-content p {
    margin: 0 0 var(--space-2, 8px);
  }

  .help-content code {
    display: block;
    padding: var(--space-2, 8px);
    background: var(--bg-secondary, #1a1a1f);
    border-radius: var(--radius-sm, 4px);
    font-family: var(--font-mono);
    margin-bottom: var(--space-3, 12px);
  }

  .help-columns {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: var(--space-3, 12px);
    margin-bottom: var(--space-3, 12px);
  }

  .help-column h4 {
    margin: 0 0 var(--space-1, 4px);
    font-size: var(--text-xs, 12px);
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .help-column ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .help-column li {
    padding: 2px 0;
  }

  .kw-unique {
    color: #c7b377;
    font-weight: 600;
  }

  .kw-set {
    color: #00ff00;
    font-weight: 600;
  }

  .kw-rare {
    color: #ffff00;
  }

  .kw-magic {
    color: #6969ff;
  }

  .kw-tier {
    color: #bd93f9;
  }

  .kw-color {
    color: #ff79c6;
  }

  .kw-sound {
    color: #8be9fd;
  }

  .help-note {
    margin-top: var(--space-2, 8px);
    padding: var(--space-2, 8px);
    background: var(--bg-secondary, #1a1a1f);
    border-radius: var(--radius-sm, 4px);
    font-size: var(--text-xs, 12px);
  }
</style>
