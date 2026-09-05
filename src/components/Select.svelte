<script lang="ts" generics="T extends string | number">
  /**
   * Custom dropdown standing in for native <select>. WebKitGTK's native
   * option-list popup can't be restyled beyond background/text color (no
   * rounded corners, no shadow, no custom hover state), so it never
   * matches the rest of the app's chrome — this renders its own menu
   * instead, same pattern as ProfileSelector's profile picker.
   */
  interface Option<T> {
    value: T;
    label: string;
  }

  interface Props<T> {
    value: T;
    options: Option<T>[];
    placeholder?: string;
    disabled?: boolean;
    class?: string;
    ariaLabel?: string;
    onchange?: (value: T) => void;
  }

  let {
    value = $bindable(),
    options,
    placeholder = '',
    disabled = false,
    class: className = '',
    ariaLabel,
    onchange,
  }: Props<T> = $props();

  let open = $state(false);

  const selectedLabel = $derived(options.find((o) => o.value === value)?.label ?? placeholder);

  function selectOption(opt: Option<T>) {
    value = opt.value;
    onchange?.(opt.value);
    open = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      open = false;
    }
  }

  function handleBlur(e: FocusEvent) {
    const container = e.currentTarget as HTMLElement;
    const related = e.relatedTarget as HTMLElement | null;
    if (!container.contains(related)) {
      open = false;
    }
  }
</script>

<svelte:window onkeydown={open ? handleKeydown : undefined} />

<div class="select-container" onfocusout={handleBlur}>
  <button
    type="button"
    class="select-trigger {className}"
    class:open
    {disabled}
    aria-label={ariaLabel}
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <span class="select-value">{selectedLabel}</span>
    <span class="select-arrow" class:open>▼</span>
  </button>

  {#if open}
    <div class="select-menu">
      {#each options as opt (opt.value)}
        <button
          type="button"
          class="select-item"
          class:selected={opt.value === value}
          onclick={() => selectOption(opt)}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .select-container {
    position: relative;
    display: inline-block;
  }

  .select-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-1) var(--space-2);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font: inherit;
    font-size: var(--text-xs);
    line-height: 1.5;
    cursor: pointer;
    min-width: 80px;
    transition: var(--transition-fast);
  }

  .select-trigger:hover:not(:disabled) {
    border-color: var(--border-hover);
  }

  .select-trigger.open {
    border-color: var(--accent-primary);
  }

  .select-trigger:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .select-value {
    flex: 1;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .select-arrow {
    font-size: 10px;
    color: var(--text-muted);
    transition: transform var(--transition-fast);
  }

  .select-arrow.open {
    transform: rotate(180deg);
  }

  .select-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 100%;
    max-height: 280px;
    overflow-y: auto;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    z-index: 100;
  }

  .select-item {
    display: flex;
    align-items: center;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: none;
    border: none;
    color: var(--text-primary);
    font: inherit;
    font-size: var(--text-sm);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .select-item:hover {
    background: var(--bg-tertiary);
  }

  .select-item.selected {
    background: var(--accent-primary-muted);
    color: var(--accent-primary);
  }
</style>
