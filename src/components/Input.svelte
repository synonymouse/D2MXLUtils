<script lang="ts">
  interface Props {
    value?: string;
    placeholder?: string;
    type?: 'text' | 'password' | 'number' | 'email';
    disabled?: boolean;
    label?: string;
    id?: string;
    oninput?: (e: Event) => void;
    onchange?: (e: Event) => void;
  }
  
  let {
    value = $bindable(''),
    placeholder = '',
    type = 'text',
    disabled = false,
    label = '',
    id = '',
    oninput,
    onchange
  }: Props = $props();
</script>

<div class="input-wrapper">
  {#if label}
    <label class="label" for={id}>{label}</label>
  {/if}
  <input
    class="input"
    {type}
    {id}
    {placeholder}
    {disabled}
    bind:value
    oninput={oninput}
    onchange={onchange}
  />
</div>

<style>
  .input-wrapper {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
</style>

