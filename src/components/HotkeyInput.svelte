<script lang="ts">
  import type { HotkeyConfig } from '../stores/settings.svelte';

  interface Props {
    value: HotkeyConfig;
    onchange?: (hotkey: HotkeyConfig) => void;
    label?: string;
    disabled?: boolean;
  }

  let {
    value = $bindable(),
    onchange,
    label = '',
    disabled = false,
  }: Props = $props();

  let isRecording = $state(false);
  // Peak modifier set seen during recording; used to commit modifier-only chords on release.
  let recordedModifiers = $state(0);

  // Windows modifier constants
  const MOD_ALT = 0x0001;
  const MOD_CONTROL = 0x0002;
  const MOD_SHIFT = 0x0004;
  const MOD_WIN = 0x0008;

  // Special keys mapping (code -> [vk, displayName])
  const SPECIAL_KEYS: Record<string, [number, string]> = {
    Space: [0x20, 'Space'], Enter: [0x0D, 'Enter'], Tab: [0x09, 'Tab'],
    Escape: [0x1B, 'Esc'], Backspace: [0x08, 'Backspace'],
    Delete: [0x2E, 'Del'], Insert: [0x2D, 'Ins'],
    Home: [0x24, 'Home'], End: [0x23, 'End'],
    PageUp: [0x21, 'PgUp'], PageDown: [0x22, 'PgDn'],
    ArrowUp: [0x26, '↑'], ArrowDown: [0x28, '↓'],
    ArrowLeft: [0x25, '←'], ArrowRight: [0x27, '→'],
    Semicolon: [0xBA, ';'], Equal: [0xBB, '='], Comma: [0xBC, ','],
    Minus: [0xBD, '-'], Period: [0xBE, '.'], Slash: [0xBF, '/'],
    Backquote: [0xC0, '`'], BracketLeft: [0xDB, '['],
    Backslash: [0xDC, '\\'], BracketRight: [0xDD, ']'], Quote: [0xDE, "'"],
  };

  // Convert KeyboardEvent to Windows VK code and display name
  function eventToVk(e: KeyboardEvent): [number, string] | null {
    const { key, code } = e;
    
    // Letters: KeyA -> 0x41, display "A"
    if (code.startsWith('Key') && code.length === 4) {
      const letter = code[3];
      return [letter.charCodeAt(0), letter];
    }
    // Digits: Digit0 -> 0x30, display "0"
    if (code.startsWith('Digit') && code.length === 6) {
      const digit = code[5];
      return [digit.charCodeAt(0), digit];
    }
    // Function keys: F1-F12
    const fMatch = code.match(/^F(\d+)$/);
    if (fMatch) {
      const num = parseInt(fMatch[1]);
      if (num >= 1 && num <= 12) return [0x6F + num, `F${num}`];
    }
    // Special keys
    if (SPECIAL_KEYS[code]) return SPECIAL_KEYS[code];
    
    return null;
  }

  // Convert stored VK code to display name
  function vkToDisplay(vk: number): string {
    if (vk >= 0x41 && vk <= 0x5A) return String.fromCharCode(vk); // A-Z
    if (vk >= 0x30 && vk <= 0x39) return String.fromCharCode(vk); // 0-9
    if (vk >= 0x70 && vk <= 0x7B) return `F${vk - 0x6F}`; // F1-F12
    // Reverse lookup in special keys
    for (const [, [code, name]] of Object.entries(SPECIAL_KEYS)) {
      if (code === vk) return name;
    }
    return `0x${vk.toString(16).toUpperCase()}`;
  }

  function buildDisplayString(modifiers: number, keyCode: number): string {
    const parts: string[] = [];
    if (modifiers & MOD_CONTROL) parts.push('Ctrl');
    if (modifiers & MOD_SHIFT) parts.push('Shift');
    if (modifiers & MOD_ALT) parts.push('Alt');
    if (modifiers & MOD_WIN) parts.push('Win');
    if (keyCode !== 0) parts.push(vkToDisplay(keyCode));
    return parts.join('+');
  }

  function modifiersFromEvent(e: KeyboardEvent): number {
    let modifiers = 0;
    if (e.ctrlKey) modifiers |= MOD_CONTROL;
    if (e.shiftKey) modifiers |= MOD_SHIFT;
    if (e.altKey) modifiers |= MOD_ALT;
    if (e.metaKey) modifiers |= MOD_WIN;
    return modifiers;
  }

  function commit(hotkey: HotkeyConfig) {
    value = hotkey;
    onchange?.(hotkey);
    isRecording = false;
    recordedModifiers = 0;
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (!isRecording) return;

    e.preventDefault();
    e.stopPropagation();

    // Accumulate modifier-only chords; committed on keyup.
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
      recordedModifiers = modifiersFromEvent(e);
      return;
    }

    const result = eventToVk(e);
    if (!result) return;
    const [keyCode] = result;

    const modifiers = modifiersFromEvent(e);
    if (modifiers === 0) return;

    commit({ keyCode, modifiers, display: buildDisplayString(modifiers, keyCode) });
  }

  function handleKeyUp(e: KeyboardEvent) {
    if (!isRecording) return;

    // Use recordedModifiers, not e.*Key — the released modifier is already gone from the event.
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key) && recordedModifiers !== 0) {
      e.preventDefault();
      e.stopPropagation();
      const modifiers = recordedModifiers;
      commit({ keyCode: 0, modifiers, display: buildDisplayString(modifiers, 0) });
    }
  }

  function handleBlur() {
    isRecording = false;
    recordedModifiers = 0;
  }
</script>

<div class="hotkey-input-wrapper">
  {#if label}
    <span class="label">{label}</span>
  {/if}
  <button
    type="button"
    class="hotkey-input"
    class:recording={isRecording}
    {disabled}
    onclick={() => { if (!disabled) { isRecording = true; recordedModifiers = 0; } }}
    onkeydown={handleKeyDown}
    onkeyup={handleKeyUp}
    onblur={handleBlur}
  >
    {#if isRecording}
      <span class="recording-text">Press keys...</span>
    {:else}
      <span class="hotkey-display">{value.display}</span>
    {/if}
  </button>
</div>

<style>
  .hotkey-input-wrapper {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .hotkey-input {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 120px;
    padding: var(--space-2) var(--space-3);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .hotkey-input:hover:not(:disabled) {
    border-color: var(--accent-primary);
    background: var(--bg-elevated);
  }

  .hotkey-input:focus {
    outline: none;
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 2px var(--accent-primary-muted);
  }

  .hotkey-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .hotkey-input.recording {
    border-color: var(--status-warning-text);
    background: rgba(218, 165, 32, 0.1);
    animation: pulse 1s ease-in-out infinite;
  }

  .recording-text {
    color: var(--status-warning-text);
    font-style: italic;
  }

  .hotkey-display {
    font-weight: 500;
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.7;
    }
  }
</style>

