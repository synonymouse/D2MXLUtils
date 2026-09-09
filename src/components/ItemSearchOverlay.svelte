<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import { clickOutside } from '../actions/click-outside';
  import { dragWindow, type WindowPosition } from '../actions/drag-window';
  import {
    itemQualityColor,
    type MxlItemDetail,
    type MxlItemEntry,
    type MxlItemSearchMode,
    type MxlItemSearchResult,
    type OpenItemSearchPayload,
  } from '../lib/mxl-item-search';
  import { setWidgetPosition, widgetPosition } from '../stores/widget-positions.svelte';

  type TooltipLine = {
    text: string;
    kind: 'base' | 'stat' | 'socket';
  };

  let { onActiveChange } = $props<{ onActiveChange: (active: boolean) => void | Promise<void> }>();

  const TYPEAHEAD_DELAY_MS = 180;
  const MIN_TYPEAHEAD_CHARS = 2;

  let open = $state(false);
  let inputValue = $state('');
  let entries = $state<MxlItemEntry[]>([]);
  let message = $state('');
  let loading = $state(false);
  let loadingName = $state<string | null>(null);
  let tooltip = $state<MxlItemDetail | null>(null);
  let inputEl: HTMLInputElement | null = $state(null);
  let layoutEl: HTMLDivElement | null = $state(null);
  let searchRequestId = 0;
  let detailRequestId = 0;
  let autoOpenSingleDetailedResult = false;
  let typeaheadTimer: number | null = null;
  let skipTypeaheadValue: string | null = null;

  const active = $derived(open || tooltip !== null);
  let pos = $derived(widgetPosition('item-search'));
  let viewportOffset = $state({ x: 0, y: 0 });

  $effect(() => {
    onActiveChange(active);
  });

  // The drag handle clamps the search panel alone into the viewport at
  // drag-start time (see `drag-window.ts`), before the tooltip panel
  // exists. The tooltip renders as a flex sibling to the right, so once it
  // opens (or the results list grows tall) the *rendered* layout can
  // extend past the viewport edge even though the drag clamp was
  // satisfied. Re-clamp the actual rendered box on every content change so
  // a tooltip never silently renders off-screen.
  $effect(() => {
    // Track dependencies that change the rendered size/position.
    void open;
    void tooltip;
    void entries.length;
    void message;
    void pos.x;
    void pos.y;
    void clampViewportOffset();
  });

  async function clampViewportOffset() {
    await tick();
    if (!layoutEl) return;
    const margin = 8;
    // `getBoundingClientRect` already reflects any offset applied by a
    // previous call (via the CSS transform below), so back it out first —
    // otherwise repeated calls would compound the correction instead of
    // recomputing it fresh against the panel's natural drag position.
    const rect = layoutEl.getBoundingClientRect();
    const naturalLeft = rect.left - viewportOffset.x;
    const naturalRight = rect.right - viewportOffset.x;
    const naturalTop = rect.top - viewportOffset.y;
    const naturalBottom = rect.bottom - viewportOffset.y;

    let dx = 0;
    let dy = 0;
    if (naturalRight > window.innerWidth - margin) {
      dx = window.innerWidth - margin - naturalRight;
    }
    if (naturalLeft + dx < margin) {
      dx = margin - naturalLeft;
    }
    if (naturalBottom > window.innerHeight - margin) {
      dy = window.innerHeight - margin - naturalBottom;
    }
    if (naturalTop + dy < margin) {
      dy = margin - naturalTop;
    }
    viewportOffset = { x: dx, y: dy };
  }

  $effect(() => {
    const value = inputValue;
    if (!open) return;

    if (typeaheadTimer !== null) {
      window.clearTimeout(typeaheadTimer);
      typeaheadTimer = null;
    }

    const q = value.trim();
    if (!q) {
      searchRequestId += 1;
      detailRequestId += 1;
      entries = [];
      tooltip = null;
      loading = false;
      loadingName = null;
      message = '';
      skipTypeaheadValue = null;
      return;
    }

    if (skipTypeaheadValue && q === skipTypeaheadValue) {
      return;
    }
    skipTypeaheadValue = null;

    if (q.length < MIN_TYPEAHEAD_CHARS) {
      searchRequestId += 1;
      detailRequestId += 1;
      entries = [];
      tooltip = null;
      loading = false;
      loadingName = null;
      message = 'Type at least 2 characters to search.';
      return;
    }

    searchRequestId += 1;
    detailRequestId += 1;
    entries = [];
    tooltip = null;
    loading = false;
    loadingName = null;
    message = '';

    typeaheadTimer = window.setTimeout(() => {
      typeaheadTimer = null;
      void runSearch(q, 'index');
    }, TYPEAHEAD_DELAY_MS);

    return () => {
      if (typeaheadTimer !== null) {
        window.clearTimeout(typeaheadTimer);
        typeaheadTimer = null;
      }
    };
  });

  function focusInput() {
    void tick().then(() => {
      inputEl?.focus();
      window.setTimeout(() => inputEl?.focus(), 60);
    });
  }

  async function openSearch(query: string | null) {
    open = true;
    tooltip = null;
    inputValue = query ?? '';
    message = '';
    autoOpenSingleDetailedResult = Boolean(query && query.trim());
    skipTypeaheadValue = query?.trim() || null;
    await onActiveChange(true);
    focusInput();
    if (query && query.trim()) {
      void runSearch(query, 'detail', autoOpenSingleDetailedResult);
      autoOpenSingleDetailedResult = false;
    }
  }

  function closeAll() {
    searchRequestId += 1;
    detailRequestId += 1;
    if (typeaheadTimer !== null) {
      window.clearTimeout(typeaheadTimer);
      typeaheadTimer = null;
    }
    open = false;
    tooltip = null;
    loading = false;
    loadingName = null;
    autoOpenSingleDetailedResult = false;
    message = '';
    skipTypeaheadValue = null;
    viewportOffset = { x: 0, y: 0 };
  }

  function moveWindow(position: WindowPosition) {
    setWidgetPosition('item-search', position.x, position.y);
  }

  async function runSearch(
    query = inputValue,
    mode: MxlItemSearchMode = 'index',
    shouldAutoOpenSingleDetailedResult = false,
  ) {
    const q = query.trim();
    if (!q) return;
    const requestId = ++searchRequestId;
    detailRequestId += 1;
    loading = true;
    loadingName = null;
    tooltip = null;
    message = '';
    try {
      const result = await invoke<MxlItemSearchResult>('search_mxl_items', { query: q, mode });
      if (requestId !== searchRequestId) return;
      applyResult(result, shouldAutoOpenSingleDetailedResult);
    } catch (err) {
      if (requestId !== searchRequestId) return;
      console.error('[ItemSearch] search failed:', err);
      entries = [];
      message = 'Search failed. Check connection and try again.';
    } finally {
      if (requestId === searchRequestId) loading = false;
    }
  }

  function applyResult(result: MxlItemSearchResult, shouldAutoOpenSingleDetailedResult = false) {
    switch (result.kind) {
      case 'results':
        entries = result.entries;
        message = result.message ?? '';
        const onlyEntry = result.entries.length === 1 ? result.entries[0] : null;
        if (shouldAutoOpenSingleDetailedResult && onlyEntry?.detail) {
          tooltip = onlyEntry.detail;
        }
        break;
      case 'notFound':
      case 'rateLimited':
      case 'error':
        entries = [];
        tooltip = null;
        message = result.message;
        break;
    }
  }

  function detailFromResult(
    result: MxlItemSearchResult,
    requestedName: string,
  ): MxlItemDetail | null {
    if (result.kind !== 'results') {
      message = result.message;
      return null;
    }
    const exact = result.entries.find(
      (entry) => entry.detail && entry.name.toLowerCase() === requestedName.toLowerCase(),
    );
    return exact?.detail ?? result.entries.find((entry) => entry.detail)?.detail ?? null;
  }

  async function openTooltip(entry: MxlItemEntry) {
    const requestId = ++detailRequestId;
    if (entry.detail) {
      loadingName = null;
      tooltip = entry.detail;
      return;
    }

    loadingName = entry.name;
    message = '';
    try {
      const result = await invoke<MxlItemSearchResult>('search_mxl_items', {
        query: entry.name,
        mode: 'detail',
      });
      if (requestId !== detailRequestId) return;
      const detail = detailFromResult(result, entry.name);
      if (detail) tooltip = detail;
    } catch (err) {
      if (requestId !== detailRequestId) return;
      console.error('[ItemSearch] detail lookup failed:', err);
      message = 'Search failed. Check connection and try again.';
    } finally {
      if (requestId === detailRequestId) loadingName = null;
    }
  }

  function tooltipBaseLine(detail: MxlItemDetail): string {
    const name = cleanDisplayName(detail.nameDisplay || detail.name).toLowerCase();
    const base = cleanDisplayName(detail.typeName || detail.class);
    if (!base || isDuplicateItemLabel(name, base.toLowerCase())) {
      return detail.quality;
    }
    return [base, detail.quality].filter(Boolean).join(' ');
  }

  function cleanDisplayName(name: string): string {
    return name
      .replace(/<br\s*\/?\s*>/gi, ' ')
      .replace(/<[^>]*>/g, '')
      .replace(/\s+/g, ' ')
      .trim();
  }

  function isDuplicateItemLabel(name: string, label: string): boolean {
    const cleanName = name.toLowerCase().trim();
    const cleanLabel = label.toLowerCase().trim();
    return (
      cleanName === cleanLabel ||
      cleanName.startsWith(`${cleanLabel} `) ||
      cleanName.startsWith(`${cleanLabel} (`) ||
      cleanLabel.startsWith(`${cleanName} `) ||
      cleanLabel.startsWith(`${cleanName} (`)
    );
  }

  function entryTypeLine(entry: MxlItemEntry): string {
    const typeName = cleanDisplayName(entry.typeName);
    if (!typeName || isDuplicateItemLabel(cleanDisplayName(entry.name), typeName)) {
      return '';
    }
    return typeName;
  }

  function statLines(detail: MxlItemDetail): TooltipLine[] {
    const lines = detail.stats
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
    const baseEndIndex = lines.findIndex((line) => line.toLowerCase().startsWith('item level'));
    const requiredLevelIndex = lines.findIndex((line) =>
      line.toLowerCase().startsWith('required level'),
    );
    const lastBaseIndex = baseEndIndex >= 0 ? baseEndIndex : requiredLevelIndex;

    return lines.map((line, index) => {
      const lower = line.toLowerCase();
      if (lower.startsWith('socketed')) {
        return { text: line, kind: 'socket' };
      }

      if (lastBaseIndex >= 0 && index <= lastBaseIndex) {
        return { text: line, kind: 'base' };
      }

      return { text: line, kind: 'stat' };
    });
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!active) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      closeAll();
    }
  }

  onMount(() => {
    const unlisteners: Array<() => void> = [];
    let disposed = false;

    listen<OpenItemSearchPayload>('open-item-search', (event) => {
      void openSearch(event.payload?.query ?? null);
    })
      .then((u) => {
        if (disposed) {
          u();
        } else {
          unlisteners.push(u);
        }
      })
      .catch((err) => console.error('[ItemSearch] failed to listen for open-item-search:', err));

    window.addEventListener('keydown', handleKeydown);

    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
      window.removeEventListener('keydown', handleKeydown);
    };
  });
</script>

{#if open || tooltip}
  <div
    class="item-search-layout"
    bind:this={layoutEl}
    use:clickOutside={closeAll}
    style:top="{pos.y}%"
    style:left="{pos.x}%"
    style:transform="translate({viewportOffset.x}px, {viewportOffset.y}px)"
  >
    {#if open}
      <section class="item-search" role="dialog" aria-label="MXL item search">
        <header
          class="search-header"
          use:dragWindow={{ target: () => layoutEl, onMove: moveWindow }}
        >
          <div>
            <h2>Item Search</h2>
            <p>Median XL database</p>
          </div>
          <button type="button" class="close" aria-label="Close item search" onclick={closeAll}
            >&times;</button
          >
        </header>

        <form
          class="search-form"
          onsubmit={(event) => {
            event.preventDefault();
            if (typeaheadTimer !== null) {
              window.clearTimeout(typeaheadTimer);
              typeaheadTimer = null;
            }
            skipTypeaheadValue = null;
            void runSearch(inputValue, 'index');
          }}
        >
          <input
            bind:this={inputEl}
            bind:value={inputValue}
            autocomplete="off"
            spellcheck="false"
            placeholder="Type at least 2 characters"
          />
        </form>

        {#if message}
          <div class="message">{message}</div>
        {/if}

        <div class="results" aria-live="polite">
          {#if loading}
            <div class="empty">Searching...</div>
          {:else if entries.length === 0}
            <div class="empty">No previous results.</div>
          {:else}
            {#each entries as entry (entry.name)}
              <button type="button" class="result-row" onclick={() => openTooltip(entry)}>
                <span class="result-name" style:color={itemQualityColor(entry.quality)}
                  >{cleanDisplayName(entry.name)}</span
                >
                <span class="result-meta">{entry.quality} &middot; {entry.class}</span>
                {#if entryTypeLine(entry)}
                  <span class="result-type">{entryTypeLine(entry)}</span>
                {/if}
                {#if loadingName === entry.name}
                  <span class="row-loading">Loading stats...</span>
                {/if}
              </button>
            {/each}
          {/if}
        </div>
      </section>
    {/if}

    {#if tooltip}
      <section class="item-tooltip" role="dialog" aria-label="Item stats">
        <button
          type="button"
          class="tooltip-close"
          aria-label="Close item tooltip"
          onclick={() => (tooltip = null)}>&times;</button
        >
        <h3 style:color={itemQualityColor(tooltip.quality)}>
          {cleanDisplayName(tooltip.nameDisplay || tooltip.name)}
        </h3>
        {#if tooltipBaseLine(tooltip)}
          <div class="tooltip-type" style:color={itemQualityColor(tooltip.quality)}>
            {tooltipBaseLine(tooltip)}
          </div>
        {/if}
        <div class="tooltip-stats">
          {#each statLines(tooltip) as line}
            <div
              class:base-line={line.kind === 'base'}
              class:stat-line={line.kind === 'stat'}
              class:socket-line={line.kind === 'socket'}
            >
              {line.text}
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>
{/if}

<style>
  .item-search-layout {
    position: fixed;
    width: max-content;
    max-width: calc(100vw - 16px);
    display: flex;
    align-items: flex-start;
    justify-content: flex-start;
    gap: 12px;
    z-index: 60;
  }

  .item-search {
    position: relative;
    width: min(560px, calc(100vw - 32px));
    pointer-events: auto;
    color: #f3f0df;
    background: linear-gradient(180deg, rgba(12, 10, 8, 0.96), rgba(0, 0, 0, 0.92));
    border: 1px solid rgba(199, 179, 119, 0.5);
    box-shadow:
      0 12px 40px rgba(0, 0, 0, 0.75),
      inset 0 0 18px rgba(199, 179, 119, 0.08);
    font-family: var(--font-mono, Consolas, monospace);
    flex: 0 0 min(560px, calc(100vw - 32px));
  }

  .search-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    border-bottom: 1px solid rgba(199, 179, 119, 0.28);
    cursor: grab;
    user-select: none;
    touch-action: none;
  }

  h2,
  p,
  h3 {
    margin: 0;
  }

  h2 {
    font-size: 15px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  p,
  .result-meta,
  .result-type,
  .tooltip-type,
  .empty,
  .message,
  .row-loading {
    color: rgba(243, 240, 223, 0.62);
    font-size: 12px;
  }

  .close,
  .tooltip-close {
    color: #f3f0df;
    background: transparent;
    border: 0;
    cursor: pointer;
    font-size: 22px;
    line-height: 1;
  }

  .search-form {
    padding: 12px;
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 9px 10px;
    color: #ffffff;
    background: rgba(0, 0, 0, 0.55);
    border: 1px solid rgba(199, 179, 119, 0.38);
    outline: none;
    font: inherit;
  }

  input:focus {
    border-color: rgba(240, 205, 140, 0.9);
  }

  .message {
    padding: 0 12px 10px;
  }

  .results {
    max-height: min(360px, 46vh);
    overflow-y: auto;
    padding: 0 8px 10px;
  }

  .results,
  .item-tooltip {
    scrollbar-color: rgba(214, 192, 132, 0.78) rgba(8, 7, 5, 0.72);
    scrollbar-width: thin;
  }

  .results::-webkit-scrollbar,
  .item-tooltip::-webkit-scrollbar {
    width: 7px;
  }

  .results::-webkit-scrollbar-track,
  .item-tooltip::-webkit-scrollbar-track {
    background: rgba(8, 7, 5, 0.72);
    border-left: 1px solid rgba(199, 179, 119, 0.18);
  }

  .results::-webkit-scrollbar-thumb,
  .item-tooltip::-webkit-scrollbar-thumb {
    background: linear-gradient(180deg, rgba(231, 211, 150, 0.92), rgba(150, 126, 71, 0.9));
    border: 1px solid rgba(0, 0, 0, 0.62);
    border-radius: 0;
  }

  .results::-webkit-scrollbar-thumb:hover,
  .item-tooltip::-webkit-scrollbar-thumb:hover {
    background: linear-gradient(180deg, rgba(246, 228, 169, 0.98), rgba(178, 150, 87, 0.96));
  }

  .result-row {
    width: 100%;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px 12px;
    padding: 8px;
    text-align: left;
    color: inherit;
    background: transparent;
    border: 1px solid transparent;
    cursor: pointer;
  }

  .result-row:hover,
  .result-row:focus-visible {
    border-color: rgba(199, 179, 119, 0.45);
    background: rgba(255, 255, 255, 0.06);
    outline: none;
  }

  .result-name {
    font-size: 14px;
    font-weight: 700;
  }

  .result-type,
  .row-loading {
    grid-column: 1 / -1;
  }

  .empty {
    padding: 20px 8px;
    text-align: center;
  }

  .item-tooltip {
    position: relative;
    width: max-content;
    min-width: min(280px, calc(100vw - 32px));
    max-width: min(720px, calc(100vw - 32px));
    max-height: 82vh;
    overflow-y: auto;
    pointer-events: auto;
    padding: 8px 13px 9px;
    color: #ffffff;
    background: rgba(0, 0, 0, 0.82);
    border: 1px solid rgba(160, 160, 160, 0.28);
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.6),
      0 1px 5px rgba(0, 0, 0, 0.35);
    font-family: var(--font-mono, Consolas, monospace);
    letter-spacing: 0.035em;
    text-align: center;
    flex: 0 1 auto;
  }

  .tooltip-close {
    position: absolute;
    top: 3px;
    right: 5px;
    opacity: 0.3;
    font-size: 16px;
  }

  .tooltip-close:hover,
  .tooltip-close:focus-visible {
    opacity: 0.75;
  }

  h3 {
    padding: 0 18px;
    font-size: 17px;
    font-weight: 700;
  }

  .tooltip-type {
    margin-top: 1px;
    color: #ffffff;
    font-size: 14px;
  }

  .tooltip-stats {
    margin-top: 5px;
    display: grid;
    gap: 2px;
    white-space: pre-wrap;
    font-size: 14px;
    line-height: 1.24;
  }

  .base-line {
    color: #ffffff;
  }

  .stat-line {
    color: #8787ff;
  }

  .socket-line {
    color: #b8b8b8;
  }
</style>
