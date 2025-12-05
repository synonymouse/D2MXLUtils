<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { RulesEditor } from "../editor";
  import { Button } from "../components";

  // Default example filter
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
  let parseStatus = $state<"idle" | "parsing" | "valid" | "error">("idle");
  let parseErrors = $state<string[]>([]);
  let ruleCount = $state(0);
  let isSaving = $state(false);

  /**
   * Parse the filter and validate it
   */
  async function parseFilter() {
    parseStatus = "parsing";
    parseErrors = [];

    try {
      const config = await invoke<{ rules: unknown[] }>("parse_filter_dsl", {
        text: dslText,
      });
      ruleCount = config.rules?.length ?? 0;
      parseStatus = "valid";
    } catch (e: unknown) {
      if (Array.isArray(e)) {
        parseErrors = e.map((err: { message?: string }) =>
          err.message ?? String(err)
        );
      } else {
        parseErrors = [String(e)];
      }
      parseStatus = "error";
    }
  }

  /**
   * Handle editor content changes
   */
  function handleChange(newValue: string) {
    dslText = newValue;
    // Reset status when content changes
    if (parseStatus !== "idle") {
      parseStatus = "idle";
    }
  }

  /**
   * Handle Ctrl+S save shortcut
   */
  async function handleSave(newValue: string) {
    dslText = newValue;
    await saveFilter();
  }

  /**
   * Save the filter
   */
  async function saveFilter() {
    isSaving = true;

    try {
      // First validate
      await parseFilter();

      if (parseStatus === "valid") {
        // TODO: Save to profile (Phase 7 - profiles support)
        console.log("[LootFilterTab] Filter saved successfully");
      }
    } finally {
      isSaving = false;
    }
  }

  /**
   * Reset to default filter
   */
  function resetToDefault() {
    dslText = DEFAULT_FILTER;
    parseStatus = "idle";
    parseErrors = [];
    ruleCount = 0;
  }
</script>

<section class="loot-filter-tab">
  <header class="tab-header">
    <div class="header-left">
      <h2>Loot Filter Editor</h2>
      <span class="status-badge" class:valid={parseStatus === "valid"} class:error={parseStatus === "error"}>
        {#if parseStatus === "parsing"}
          <span class="spinner"></span> Parsing...
        {:else if parseStatus === "valid"}
          ✓ {ruleCount} {ruleCount === 1 ? "rule" : "rules"}
        {:else if parseStatus === "error"}
          ✗ {parseErrors.length} {parseErrors.length === 1 ? "error" : "errors"}
        {:else}
          —
        {/if}
      </span>
    </div>

    <div class="header-actions">
      <Button
        variant="ghost"
        size="sm"
        onclick={resetToDefault}
        disabled={isSaving}
      >
        Reset
      </Button>
      <Button
        variant="secondary"
        size="sm"
        onclick={parseFilter}
        disabled={parseStatus === "parsing" || isSaving}
      >
        {parseStatus === "parsing" ? "Parsing..." : "Validate"}
      </Button>
      <Button
        variant="primary"
        size="sm"
        onclick={saveFilter}
        disabled={parseStatus === "parsing" || isSaving}
      >
        {isSaving ? "Saving..." : "Save"}
      </Button>
    </div>
  </header>

  <div class="editor-container">
    <RulesEditor
      bind:value={dslText}
      onchange={handleChange}
      onsave={handleSave}
    />
  </div>

  {#if parseErrors.length > 0}
    <div class="error-panel">
      <div class="error-header">
        <span class="error-icon">⚠</span>
        <span>Validation Errors</span>
      </div>
      <ul class="error-list">
        {#each parseErrors as error}
          <li class="error-item">{error}</li>
        {/each}
      </ul>
    </div>
  {/if}

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
    height: 100%;
    gap: var(--space-3, 12px);
    padding: var(--space-4, 16px);
  }

  .tab-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-shrink: 0;
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
    background: var(--bg-tertiary, #12121a);
    color: var(--text-tertiary, #888);
  }

  .status-badge.valid {
    background: rgba(0, 255, 0, 0.1);
    color: var(--quality-set, #00ff00);
  }

  .status-badge.error {
    background: rgba(255, 68, 68, 0.1);
    color: var(--stat-fire, #ff4444);
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .editor-container {
    flex: 1;
    min-height: 200px;
    overflow: hidden;
  }

  .error-panel {
    flex-shrink: 0;
    background: rgba(255, 68, 68, 0.08);
    border: 1px solid rgba(255, 68, 68, 0.3);
    border-radius: var(--radius-md, 8px);
    max-height: 120px;
    overflow-y: auto;
  }

  .error-header {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
    padding: var(--space-2, 8px) var(--space-3, 12px);
    font-size: var(--text-sm, 13px);
    font-weight: 600;
    color: var(--stat-fire, #ff4444);
    border-bottom: 1px solid rgba(255, 68, 68, 0.2);
  }

  .error-icon {
    font-size: 14px;
  }

  .error-list {
    list-style: none;
    margin: 0;
    padding: var(--space-2, 8px) var(--space-3, 12px);
  }

  .error-item {
    font-family: var(--font-mono);
    font-size: var(--text-sm, 13px);
    color: var(--text-secondary);
    padding: var(--space-1, 4px) 0;
  }

  .error-item::before {
    content: "→ ";
    color: var(--stat-fire, #ff4444);
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
