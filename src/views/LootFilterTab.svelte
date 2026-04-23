<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { RulesEditor, type ValidationResult } from "../editor";
  import { ProfileSelector } from "../components";
  import { settingsStore } from "../stores";

  let dslText = $state("");
  let selectedProfile = $state(settingsStore.settings.activeProfile || "");
  let validationStatus = $state<"idle" | "valid" | "error">("idle");
  let errorCount = $state(0);
  let ruleCount = $state(0);
  let hasUnsavedChanges = $state(false);
  // Default-mode state derived from the parsed DSL (mirrors FilterConfig.hide_all).
  let hideAll = $state(false);

  onMount(async () => {
    try {
      await invoke("set_filter_enabled", { enabled: true });
    } catch (e) {
      console.error("[LootFilterTab] Failed to enable filter:", e);
    }
  });

  async function syncFilterConfig() {
    try {
      const config = await invoke<any>("parse_filter_dsl", { text: dslText });
      hideAll = !!config.hide_all;
      await invoke("set_filter_config", { config });
      await invoke("set_filter_enabled", { enabled: true });
    } catch (e) {
      console.error("[LootFilterTab] Failed to sync filter config:", e);
    }
  }

  /**
   * Handle validation results from the editor's linter.
   * Only hard errors block sync; warnings/info are advisory.
   */
  function handleValidation(result: ValidationResult) {
    const hardErrors = result.errors.filter((e) => e.severity === "error");
    if (hardErrors.length > 0) {
      validationStatus = "error";
      errorCount = hardErrors.length;
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
  async function handleProfileLoad(name: string, rulesText: string) {
    dslText = rulesText;
    selectedProfile = name;
    hasUnsavedChanges = false;
    validationStatus = "idle";

    // Update settings with active profile
    if (name) {
      settingsStore.set('activeProfile', name);
    }

    // Push the loaded filter to the scanner immediately. The backend is
    // authoritative for parsing, so there's no need to wait for the linter.
    await syncFilterConfig();
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
  async function handleSaveComplete() {
    hasUnsavedChanges = false;

    // Sync filter config to backend
    await syncFilterConfig();
  }
</script>

<section class="loot-filter-tab">
  <header class="tab-header">
    <div class="header-left">
      <span class="status-badge" class:valid={validationStatus === "valid"} class:error={validationStatus === "error"}>
        {#if validationStatus === "valid"}
          ✓ {ruleCount} {ruleCount === 1 ? "rule" : "rules"}
        {:else if validationStatus === "error"}
          ✗ {errorCount} {errorCount === 1 ? "error" : "errors"}
        {:else}
          —
        {/if}
      </span>
      <span
        class="default-mode-badge"
        class:hide={hideAll}
        title="Add 'hide default' at the top of the file to hide all unmatched items by default."
      >
        Default: {hideAll ? "hide" : "show"} unmatched
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
        <p>Rule format (all parts optional — rules are matched last-wins):</p>
        <code>["name"] [quality] [tier] [eth] &#123;stat&#125; [color] [show|hide] [sound] [notify] [stat] [map]</code>

        <div class="help-columns">
          <div class="help-column">
            <h4>Quality</h4>
            <ul>
              <li><span class="kw-unique">unique</span></li>
              <li><span class="kw-set">set</span></li>
              <li><span class="kw-rare">rare</span></li>
              <li><span class="kw-magic">magic</span>, craft, honor</li>
              <li>normal, low, superior</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Tier</h4>
            <ul>
              <li>
                <span class="kw-flag">sacred</span>,
                <span class="kw-flag">angelic</span>,
                <span class="kw-flag">master</span>
              </li>
              <li>0, 1, 2, 3, 4</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Colors</h4>
            <ul>
              <li>
                <span class="kw-c-gold">gold</span>,
                <span class="kw-c-lime">lime</span>,
                <span class="kw-c-red">red</span>,
                <span class="kw-c-blue">blue</span>
              </li>
              <li>
                <span class="kw-c-white">white</span>,
                <span class="kw-c-yellow">yellow</span>,
                <span class="kw-c-orange">orange</span>,
                <span class="kw-c-pink">pink</span>
              </li>
              <li>
                <span class="kw-c-grey">grey</span>,
                <span class="kw-c-black">black</span>,
                <span class="kw-c-purple">purple</span>,
                <span class="kw-c-green">green</span>
              </li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Visibility / Notify</h4>
            <ul>
              <li><span class="kw-flag">show</span>, <span class="kw-flag">hide</span></li>
              <li><span class="kw-flag">notify</span> (required for alerts)</li>
              <li><span class="kw-flag">map</span> (automap marker)</li>
            </ul>
          </div>

          <div class="help-column">
            <h4>Sounds</h4>
            <ul>
              <li><span class="kw-flag">sound1</span> - <span class="kw-flag">sound6</span></li>
              <li><span class="kw-flag">sound_none</span></li>
            </ul>
          </div>
        </div>

        <p class="help-note">
          <strong>eth</strong> — match ethereal items only<br />
          <strong>stat</strong> — include item stats in the notification<br />
          <strong>map</strong> — drop a red-cross marker on the in-game automap at the item's location (independent of <span class="kw-flag">notify</span>)<br />
          <strong>&#123;pattern&#125;</strong> — regex match on stat text<br />
          <strong>Groups:</strong> <code class="inline-code">[unique gold notify] &#123; "Jordan" "Mara" &#125;</code> — shared attributes for each rule inside<br />
          <strong>Default mode:</strong> place <code class="inline-code">hide default</code> (or <code class="inline-code">show default</code>) on its own line at the top of the file. With <code class="inline-code">hide default</code>, only rules with <span class="kw-flag">show</span> reveal items.
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

  .header-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
  }

  .default-mode-badge {
    display: inline-flex;
    align-items: center;
    padding: 4px 10px;
    border-radius: var(--radius-full, 9999px);
    font-size: var(--text-xs, 12px);
    font-weight: 500;
    background: color-mix(in srgb, var(--text-secondary) 10%, transparent);
    color: var(--text-secondary);
    cursor: help;
    user-select: none;
  }

  .default-mode-badge.hide {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
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
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .syntax-help details {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-tertiary, #12121a);
    border: 1px solid var(--border-primary, #2a2a35);
    border-radius: var(--radius-md, 8px);
  }

  .syntax-help summary {
    padding: var(--space-2, 8px) var(--space-3, 12px);
    font-size: var(--text-sm, 13px);
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
    flex-shrink: 0;
  }

  .syntax-help summary:hover {
    color: var(--text-primary);
  }

  .help-content {
    padding: var(--space-3, 12px);
    padding-top: 0;
    font-size: var(--text-sm, 13px);
    color: var(--text-secondary);
    overflow-y: auto;
    min-height: 0;
    max-height: 55vh;
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

  /* Quality keywords — match the D2 item-rarity palette so the reference
     echoes what each quality looks like in-game. */
  .kw-unique { color: #c7a84c; font-weight: 600; }
  .kw-set    { color: #3d9d4a; font-weight: 600; }
  .kw-rare   { color: #d4a72c; font-weight: 600; }
  .kw-magic  { color: #5080ff; font-weight: 600; }

  /* Unified flag color for tier / visibility / notify / map / sound — these
     are structural keywords, not semantically colored concepts. */
  .kw-flag {
    color: var(--accent-primary);
    font-weight: 600;
  }

  /* Literal color swatches: each color name is rendered in its own color
     so the reference doubles as a color palette. Values picked to stay
     readable on both the parchment light theme and the deep-black dark
     theme; black/white/grey use mid-tones instead of pure values for
     visibility on both backgrounds. */
  .kw-c-gold   { color: #c7a84c; font-weight: 600; }
  .kw-c-lime   { color: #4fbf2e; font-weight: 600; }
  .kw-c-red    { color: #d83a3a; font-weight: 600; }
  .kw-c-blue   { color: #4a6fe0; font-weight: 600; }
  .kw-c-white  { color: #bfbfbf; font-weight: 600; }
  .kw-c-yellow { color: #d6a517; font-weight: 600; }
  .kw-c-orange { color: #d97518; font-weight: 600; }
  .kw-c-pink   { color: #d56bb0; font-weight: 600; }
  .kw-c-grey   { color: #8a8a8a; font-weight: 600; }
  .kw-c-black  { color: #303030; font-weight: 600; }
  .kw-c-purple { color: #8a5fb8; font-weight: 600; }
  .kw-c-green  { color: #3d9050; font-weight: 600; }

  .inline-code {
    display: inline;
    padding: 1px 4px;
    font-family: var(--font-mono);
    background: var(--bg-secondary, #1a1a1f);
    border-radius: var(--radius-sm, 3px);
  }

  .help-note {
    margin-top: var(--space-2, 8px);
    padding: var(--space-2, 8px);
    background: var(--bg-secondary, #1a1a1f);
    border-radius: var(--radius-sm, 4px);
    font-size: var(--text-xs, 12px);
  }
</style>
