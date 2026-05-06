<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Button } from '../components';
  import { settingsStore, type SoundSlot, type SoundSource } from '../stores';
  import { playSound } from '../lib/sound-player';

  const ACCEPT = '.mp3,.wav,.ogg,.m4a,.flac';
  const MAX_BYTES = 5 * 1024 * 1024;

  let masterVolume = $derived(settingsStore.settings.soundVolume);
  let slots = $derived(settingsStore.settings.sounds);
  let goblinAlertSlot = $derived(settingsStore.settings.goblinAlertSlot);

  // Non-empty slots, exposed to the goblin-alert dropdown. Slot index is
  // 1-based and matches the position in `slots`.
  let alertChoices = $derived(
    slots
      .map((slot, i) => ({ index: i + 1, slot }))
      .filter(({ slot }) => slot.source.kind !== 'empty')
  );

  // Inline error message per slot (1-based index → message). Cleared on
  // any successful import or on next attempt.
  let errors = $state<Record<number, string>>({});

  function setError(slot: number, msg: string | null) {
    if (msg === null) {
      const next = { ...errors };
      delete next[slot];
      errors = next;
    } else {
      errors = { ...errors, [slot]: msg };
    }
  }

  function handleMasterVolumeInput(e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    settingsStore.setSoundVolume(parseFloat(target.value));
  }

  function handleGoblinAlertChange(e: Event) {
    const target = e.currentTarget as HTMLSelectElement;
    const next = target.value === '' ? null : parseInt(target.value, 10);
    settingsStore.set('goblinAlertSlot', next);
  }

  function handleSlotVolumeInput(slot: number, e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    settingsStore.updateSoundSlot(slot, { volume: parseFloat(target.value) });
  }

  function handleLabelInput(slot: number, e: Event) {
    const target = e.currentTarget as HTMLInputElement;
    settingsStore.updateSoundSlot(slot, { label: target.value });
  }

  function handlePlay(slot: number) {
    void playSound(slot, masterVolume);
  }

  async function handleFilePicked(slot: number, e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    // Reset the input so re-selecting the same file fires `change` again.
    input.value = '';
    if (!file) return;

    setError(slot, null);
    if (file.size > MAX_BYTES) {
      setError(slot, `File too large (${Math.ceil(file.size / 1024)} KB, max 5 MB)`);
      return;
    }

    try {
      const buf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const fileName = await invoke<string>('import_sound_file', {
        slot,
        fileName: file.name,
        bytes,
      });
      const source: SoundSource = { kind: 'custom', fileName };
      settingsStore.updateSoundSlot(slot, { source });
    } catch (err) {
      setError(slot, String(err));
    }
  }

  async function handleReset(slot: number) {
    setError(slot, null);
    try {
      await invoke('delete_sound_file', { slot });
    } catch (err) {
      setError(slot, String(err));
      return;
    }
    settingsStore.updateSoundSlot(slot, { source: { kind: 'default' } });
  }

  async function handleDelete(slot: number) {
    setError(slot, null);
    try {
      await invoke('delete_sound_file', { slot });
    } catch (err) {
      setError(slot, String(err));
      return;
    }
    settingsStore.updateSoundSlot(slot, { source: { kind: 'empty' } });
  }

  function handleAdd() {
    settingsStore.appendSoundSlot();
  }

  function isBuiltin(slot: number): boolean {
    return slot >= 1 && slot <= 7;
  }

  function isCustom(source: SoundSource): boolean {
    return source.kind === 'custom';
  }

  function isEmpty(source: SoundSource): boolean {
    return source.kind === 'empty';
  }

  function fileInputId(slot: number): string {
    return `sound-file-${slot}`;
  }
</script>

<section class="tab-content">
  <div class="settings-section">
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Master volume</span>
        <span class="setting-hint">
          Multiplied with each slot's per-sound volume. Set to 0 to silence everything.
        </span>
      </div>
      <div class="setting-control">
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={masterVolume}
          oninput={handleMasterVolumeInput}
          class="slider"
          aria-label="Master sound volume"
        />
        <span class="setting-value">{Math.round(masterVolume * 100)}%</span>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Goblin alert</span>
        <span class="setting-hint">
          Plays the selected sound when a goblin appears nearby. Pick "None" to disable.
        </span>
      </div>
      <div class="setting-control">
        <select
          class="goblin-select"
          value={goblinAlertSlot ?? ''}
          onchange={handleGoblinAlertChange}
          aria-label="Goblin alert sound"
        >
          <option value="">None</option>
          {#each alertChoices as { index, slot } (index)}
            <option value={index}>{slot.label || `Sound ${index}`}</option>
          {/each}
        </select>
      </div>
    </div>
  </div>

  <div class="settings-section">
    <h2 class="section-title">Sounds</h2>

    {#each slots as slot, i (i)}
      {@const slotIndex = i + 1}
      {@const empty = isEmpty(slot.source)}
      <div class="slot-row">
        <div class="slot-num" class:slot-num-empty={empty}>{slotIndex}</div>

        <input
          class="slot-label"
          class:slot-label-empty={empty}
          type="text"
          value={slot.label}
          placeholder={`Sound ${slotIndex}`}
          oninput={(e) => handleLabelInput(slotIndex, e)}
          aria-label={`Label for sound ${slotIndex}`}
        />

        <div class="slot-volume" class:slot-volume-empty={empty}>
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={slot.volume}
            disabled={empty}
            oninput={(e) => handleSlotVolumeInput(slotIndex, e)}
            class="slider"
            aria-label={`Volume for sound ${slotIndex}`}
          />
          <span class="setting-value">{Math.round(slot.volume * 100)}%</span>
        </div>

        <div class="slot-actions">
          {#if !empty}
            <Button
              variant="secondary"
              size="sm"
              onclick={() => handlePlay(slotIndex)}
            >
              Play
            </Button>
          {/if}

          <Button variant="secondary" size="sm" onclick={() => {
            document.getElementById(fileInputId(slotIndex))?.click();
          }}>
            {empty ? 'Upload' : 'Replace'}
          </Button>
          <input
            id={fileInputId(slotIndex)}
            type="file"
            accept={ACCEPT}
            class="file-input-hidden"
            onchange={(e) => handleFilePicked(slotIndex, e)}
          />

          {#if isBuiltin(slotIndex) && isCustom(slot.source)}
            <Button variant="secondary" size="sm" onclick={() => handleReset(slotIndex)}>
              Reset
            </Button>
          {/if}

          {#if !isBuiltin(slotIndex) && isCustom(slot.source)}
            <Button variant="secondary" size="sm" onclick={() => handleDelete(slotIndex)}>
              Delete
            </Button>
          {/if}
        </div>

        {#if errors[slotIndex]}
          <div class="slot-error">{errors[slotIndex]}</div>
        {/if}
      </div>
    {/each}

    <div class="add-row">
      <Button variant="secondary" size="sm" onclick={handleAdd}>+ Add sound</Button>
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

  .goblin-select {
    padding: var(--space-1) var(--space-2);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font: inherit;
    min-width: 180px;
  }

  .slot-row {
    display: grid;
    grid-template-columns: 32px 180px 220px 1fr;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border-primary);
  }

  .slot-num {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-muted);
    text-align: right;
  }

  .slot-num-empty,
  .slot-label-empty,
  .slot-volume-empty {
    opacity: 0.6;
  }

  .slot-label {
    padding: var(--space-1) var(--space-2);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font: inherit;
  }

  .slot-volume {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .slot-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    justify-self: end;
  }

  .file-input-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .slot-error {
    grid-column: 2 / -1;
    color: var(--status-error-text);
    font-size: var(--text-sm);
  }

  .add-row {
    margin-top: var(--space-3);
  }
</style>
