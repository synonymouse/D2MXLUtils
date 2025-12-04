<script lang="ts">
  import type { Snippet } from 'svelte';
  
  interface Tab {
    id: string;
    label: string;
  }
  
  interface Props {
    tabs: Tab[];
    activeTab?: string;
    onTabChange?: (tabId: string) => void;
    children: Snippet<[string]>;
  }
  
  let {
    tabs,
    activeTab = $bindable(tabs[0]?.id ?? ''),
    onTabChange,
    children
  }: Props = $props();
  
  function selectTab(tabId: string) {
    activeTab = tabId;
    onTabChange?.(tabId);
  }
  
  function handleKeyDown(e: KeyboardEvent, tabId: string) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      selectTab(tabId);
    }
  }
</script>

<div class="tabs">
  <div class="tabs-list" role="tablist">
    {#each tabs as tab (tab.id)}
      <button
        class="tab"
        class:active={activeTab === tab.id}
        role="tab"
        aria-selected={activeTab === tab.id}
        tabindex={activeTab === tab.id ? 0 : -1}
        onclick={() => selectTab(tab.id)}
        onkeydown={(e) => handleKeyDown(e, tab.id)}
      >
        {tab.label}
      </button>
    {/each}
  </div>
  
  <div class="tabs-content" role="tabpanel">
    {@render children(activeTab)}
  </div>
</div>

