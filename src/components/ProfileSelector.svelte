<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Button } from "./index";

  /** Profile info from backend */
  interface ProfileInfo {
    name: string;
    ruleCount: number;
    modified: string | null;
  }

  interface Props {
    /** Currently selected profile name */
    selectedProfile: string;
    /** Whether saving is allowed (e.g. valid content) */
    canSave?: boolean;
    /** Callback when profile selection changes */
    onselect?: (profile: ProfileInfo | null) => void;
    /** Callback when profile is loaded (with raw DSL text) */
    onload?: (name: string, rulesText: string) => void;
    /** Callback to get current DSL for saving */
    getCurrentDsl?: () => string;
    /** Callback when save is completed */
    onsave?: () => void;
  }

  let {
    selectedProfile = $bindable(""),
    canSave = true,
    onselect,
    onload,
    getCurrentDsl,
    onsave,
  }: Props = $props();

  let profiles = $state<ProfileInfo[]>([]);
  let isLoading = $state(false);
  let error = $state<string | null>(null);
  
  // Dialog state
  let showDialog = $state(false);
  let dialogMode = $state<"new" | "rename" | "duplicate">("new");
  let dialogInput = $state("");
  let dialogError = $state<string | null>(null);

  // Dropdown state
  let showDropdown = $state(false);

  // Load profiles on mount
  $effect(() => {
    loadProfiles();
  });

  async function loadProfiles() {
    isLoading = true;
    error = null;
    
    try {
      profiles = await invoke<ProfileInfo[]>("list_profiles");
      
      if (selectedProfile) {
        // If profile is already selected (e.g. from settings), load its content
        // Verify it exists in the list first
        const exists = profiles.some(p => p.name === selectedProfile);
        if (exists) {
          await loadProfileContent(selectedProfile);
        } else {
          // Profile from settings doesn't exist anymore
          selectedProfile = "";
          if (profiles.length > 0) {
            await selectProfile(profiles[0]);
          }
        }
      } else if (profiles.length > 0) {
        // If no profile selected, select first one
        await selectProfile(profiles[0]);
      }
    } catch (e) {
      error = String(e);
      console.error("[ProfileSelector] Failed to load profiles:", e);
    } finally {
      isLoading = false;
    }
  }

  async function loadProfileContent(name: string) {
    try {
      const rulesText = await invoke<string>("load_profile", { name });
      onload?.(name, rulesText);
    } catch (e) {
      console.error(`[ProfileSelector] Failed to load content for ${name}:`, e);
      error = String(e);
    }
  }

  async function selectProfile(profile: ProfileInfo) {
    // Even if name is same, we might want to reload content if explicit select
    // But usually we guard against redundant loads
    if (profile.name === selectedProfile && !isLoading) {
      // Just ensure content is loaded if it wasn't
      // But for now, let's force reload to be safe or just return
    }
    
    isLoading = true;
    error = null;
    
    try {
      await loadProfileContent(profile.name);
      selectedProfile = profile.name;
      onselect?.(profile);
      showDropdown = false;
    } catch (e) {
      error = String(e);
    } finally {
      isLoading = false;
    }
  }

  async function saveCurrentProfile() {
    if (!selectedProfile || !getCurrentDsl || !canSave) return;

    isLoading = true;
    error = null;

    try {
      await invoke("save_profile", {
        name: selectedProfile,
        rulesText: getCurrentDsl(),
      });
      await loadProfiles(); // Reload to update rule count/modified time
      onsave?.();
    } catch (e) {
      error = String(e);
      console.error("[ProfileSelector] Failed to save profile:", e);
    } finally {
      isLoading = false;
    }
  }

  function openNewDialog() {
    dialogMode = "new";
    dialogInput = "";
    dialogError = null;
    showDialog = true;
    showDropdown = false;
  }

  function openRenameDialog() {
    if (!selectedProfile) return;
    dialogMode = "rename";
    dialogInput = selectedProfile;
    dialogError = null;
    showDialog = true;
    showDropdown = false;
  }

  function openDuplicateDialog() {
    if (!selectedProfile) return;
    dialogMode = "duplicate";
    dialogInput = `${selectedProfile} Copy`;
    dialogError = null;
    showDialog = true;
    showDropdown = false;
  }

  async function handleDialogSubmit() {
    if (!dialogInput.trim()) {
      dialogError = "Name cannot be empty";
      return;
    }
    
    isLoading = true;
    dialogError = null;
    
    try {
      if (dialogMode === "new") {
        const profile = await invoke<ProfileInfo>("create_profile", { name: dialogInput });
        await loadProfiles();
        await selectProfile(profile);
      } else if (dialogMode === "rename") {
        await invoke("rename_profile", { 
          oldName: selectedProfile, 
          newName: dialogInput 
        });
        selectedProfile = dialogInput;
        await loadProfiles();
      } else if (dialogMode === "duplicate") {
        const profile = await invoke<ProfileInfo>("duplicate_profile", { 
          name: selectedProfile, 
          newName: dialogInput 
        });
        await loadProfiles();
        await selectProfile(profile);
      }
      
      showDialog = false;
    } catch (e) {
      dialogError = String(e);
    } finally {
      isLoading = false;
    }
  }

  async function deleteProfile() {
    if (!selectedProfile) return;
    
    if (!confirm(`Delete profile "${selectedProfile}"?`)) return;
    
    isLoading = true;
    error = null;
    
    try {
      await invoke("delete_profile", { name: selectedProfile });
      selectedProfile = "";
      await loadProfiles();
      
      // Select first available profile
      if (profiles.length > 0) {
        await selectProfile(profiles[0]);
      } else {
        onselect?.(null);
        onload?.("", "# No profile selected\n");
      }
      
      showDropdown = false;
    } catch (e) {
      error = String(e);
    } finally {
      isLoading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      showDialog = false;
      showDropdown = false;
    } else if (e.key === "Enter" && showDialog) {
      handleDialogSubmit();
    }
  }

  function handleDropdownBlur(e: FocusEvent) {
    // Close dropdown when focus leaves the container
    const container = (e.currentTarget as HTMLElement);
    const related = e.relatedTarget as HTMLElement | null;
    
    if (!container.contains(related)) {
      showDropdown = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="profile-selector" onfocusout={handleDropdownBlur}>
  <!-- Profile dropdown -->
  <div class="dropdown-container">
    <button 
      class="dropdown-trigger"
      onclick={() => showDropdown = !showDropdown}
      disabled={isLoading}
    >
      <span class="profile-icon">📁</span>
      <span class="profile-name">
        {selectedProfile || "No profile"}
      </span>
      <span class="dropdown-arrow" class:open={showDropdown}>▼</span>
    </button>

    {#if showDropdown}
      <div class="dropdown-menu">
        {#if profiles.length === 0}
          <div class="dropdown-empty">No profiles yet</div>
        {:else}
          {#each profiles as profile}
            <button
              class="dropdown-item"
              class:selected={profile.name === selectedProfile}
              onclick={() => selectProfile(profile)}
            >
              <span class="item-name">{profile.name}</span>
              <span class="item-meta">{profile.ruleCount} rules</span>
            </button>
          {/each}
        {/if}
        
        <div class="dropdown-divider"></div>
        
        <button class="dropdown-action" onclick={openNewDialog}>
          <span>➕</span> New Profile
        </button>
        <button 
          class="dropdown-action" 
          onclick={openRenameDialog}
          disabled={!selectedProfile}
        >
          <span>✏️</span> Rename
        </button>
        <button 
          class="dropdown-action" 
          onclick={openDuplicateDialog}
          disabled={!selectedProfile}
        >
          <span>📋</span> Duplicate
        </button>
        <button 
          class="dropdown-action danger" 
          onclick={deleteProfile}
          disabled={!selectedProfile}
        >
          <span>🗑️</span> Delete
        </button>
      </div>
    {/if}
  </div>

  <!-- Save button -->
  <Button
    variant="primary"
    size="sm"
    onclick={saveCurrentProfile}
    disabled={!selectedProfile || isLoading || !canSave}
  >
    {isLoading ? "..." : "Save"}
  </Button>

  {#if error}
    <span class="error-text" title={error}>⚠️</span>
  {/if}
</div>

<!-- Dialog overlay -->
{#if showDialog}
  <div class="dialog-overlay" onclick={() => showDialog = false}>
    <div class="dialog" onclick={(e) => e.stopPropagation()}>
      <h3 class="dialog-title">
        {#if dialogMode === "new"}
          New Profile
        {:else if dialogMode === "rename"}
          Rename Profile
        {:else}
          Duplicate Profile
        {/if}
      </h3>
      
      <input
        class="dialog-input"
        type="text"
        placeholder="Profile name"
        bind:value={dialogInput}
        autofocus
      />
      
      {#if dialogError}
        <p class="dialog-error">{dialogError}</p>
      {/if}
      
      <div class="dialog-actions">
        <Button variant="ghost" size="sm" onclick={() => showDialog = false}>
          Cancel
        </Button>
        <Button 
          variant="primary" 
          size="sm" 
          onclick={handleDialogSubmit}
          disabled={isLoading || !dialogInput.trim()}
        >
          {isLoading ? "..." : dialogMode === "new" ? "Create" : dialogMode === "rename" ? "Rename" : "Duplicate"}
        </Button>
      </div>
    </div>
  </div>
{/if}

<style>
  .profile-selector {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
    position: relative;
  }

  .dropdown-container {
    position: relative;
  }

  .dropdown-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
    padding: var(--space-1) var(--space-2);
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 6px);
    color: var(--text-primary);
    font-size: var(--text-xs);
    line-height: 1.5;
    cursor: pointer;
    min-width: 180px;
    transition: all 0.15s ease;
  }

  .profile-icon {
    font-size: var(--text-xs);
  }

  .dropdown-trigger:hover:not(:disabled) {
    border-color: var(--border-hover);
    background: var(--bg-secondary);
  }

  .dropdown-trigger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .profile-name {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-arrow {
    font-size: 10px;
    color: var(--text-tertiary);
    transition: transform 0.15s ease;
  }

  .dropdown-arrow.open {
    transform: rotate(180deg);
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 220px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 6px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 100;
    overflow: hidden;
  }

  .dropdown-empty {
    padding: 12px 16px;
    color: var(--text-tertiary);
    font-size: var(--text-sm, 13px);
    font-style: italic;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: var(--text-sm, 13px);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s ease;
  }

  .dropdown-item:hover {
    background: var(--bg-tertiary);
  }

  .dropdown-item.selected {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }

  .item-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-meta {
    font-size: var(--text-xs, 11px);
    color: var(--text-tertiary);
    flex-shrink: 0;
    margin-left: 8px;
  }

  .dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }

  .dropdown-action {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: var(--text-sm, 13px);
    cursor: pointer;
    text-align: left;
    transition: all 0.1s ease;
  }

  .dropdown-action:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .dropdown-action:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .dropdown-action.danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--status-error) 15%, transparent);
    color: var(--status-error-text);
  }

  .error-text {
    color: var(--status-error-text);
    cursor: help;
  }

  /* Dialog styles */
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .dialog {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, 8px);
    padding: var(--space-4, 16px);
    min-width: 300px;
    max-width: 400px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .dialog-title {
    margin: 0 0 var(--space-3, 12px);
    font-size: var(--text-lg, 16px);
    font-weight: 600;
    color: var(--text-primary);
  }

  .dialog-input {
    width: 100%;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 6px);
    color: var(--text-primary);
    font-size: var(--text-sm, 13px);
    outline: none;
    transition: border-color 0.15s ease;
  }

  .dialog-input:focus {
    border-color: var(--accent);
  }

  .dialog-error {
    margin: var(--space-2, 8px) 0 0;
    padding: 8px;
    background: color-mix(in srgb, var(--status-error) 15%, transparent);
    border-radius: var(--radius-sm, 4px);
    color: var(--status-error-text);
    font-size: var(--text-xs, 12px);
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2, 8px);
    margin-top: var(--space-4, 16px);
  }
</style>
