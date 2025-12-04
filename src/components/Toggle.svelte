<script lang="ts">
  interface Props {
    checked?: boolean;
    disabled?: boolean;
    label?: string;
    id?: string;
    onchange?: (checked: boolean) => void;
  }
  
  let {
    checked = $bindable(false),
    disabled = false,
    label = '',
    id = '',
    onchange
  }: Props = $props();
  
  function handleChange(e: Event) {
    const target = e.target as HTMLInputElement;
    checked = target.checked;
    onchange?.(checked);
  }
</script>

<label class="toggle" class:disabled>
  <input
    class="toggle-input"
    type="checkbox"
    {id}
    {disabled}
    bind:checked
    onchange={handleChange}
  />
  <span class="toggle-track">
    <span class="toggle-thumb"></span>
  </span>
  {#if label}
    <span class="toggle-label">{label}</span>
  {/if}
</label>

<style>
  .toggle.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>

