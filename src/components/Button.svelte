<script lang="ts">
  import type { Snippet } from 'svelte';
  
  interface Props {
    variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
    size?: 'sm' | 'md' | 'lg';
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  }
  
  let {
    variant = 'primary',
    size = 'md',
    disabled = false,
    onclick,
    children
  }: Props = $props();
  
  const sizeClass = $derived(size === 'sm' ? 'btn-sm' : size === 'lg' ? 'btn-lg' : '');
</script>

<button
  class="btn btn-{variant} {sizeClass}"
  {disabled}
  onclick={onclick}
>
  {@render children()}
</button>

