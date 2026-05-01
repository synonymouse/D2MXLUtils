import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type PickupState = 'pending' | 'picked_up' | 'lost';

export interface LootHistoryEntry {
  unit_id: number;
  seed: number;
  timestamp_ms: number;
  name: string;
  color: string | null;
  pickup: PickupState;
}

interface LootHistoryUpdate {
  unit_id: number;
  seed: number;
  pickup: PickupState;
}

class LootHistoryStore {
  // Insertion-ordered list of entries; renderable directly.
  entries = $state<LootHistoryEntry[]>([]);
  // Stable identity index. `seed` is preserved across the engine's
  // teleport-away/return cycle (where `unit_id` is reassigned), so we
  // key by it. `seed === 0` means the backend failed to read it — fall
  // back to `unit_id` for those rows (they're effectively per-session
  // ephemeral and won't survive a rekey, but that's the best we can do).
  #index = new Map<string, number>();
  #unlisteners: UnlistenFn[] = [];
  #initialized = false;

  async initialize(): Promise<void> {
    if (this.#initialized) return;
    this.#initialized = true;

    try {
      const initial = await invoke<LootHistoryEntry[]>('get_loot_history');
      this.#replaceAll(initial);
    } catch (err) {
      console.error('[LootHistory] failed to load initial snapshot:', err);
    }

    try {
      this.#unlisteners.push(
        await listen<LootHistoryEntry>('loot-history-entry', (event) => {
          this.#append(event.payload);
        }),
        await listen<LootHistoryUpdate>('loot-history-update', (event) => {
          this.#applyUpdate(event.payload);
        }),
        await listen<null>('loot-history-cleared', () => {
          this.#replaceAll([]);
        }),
      );
    } catch (err) {
      console.error('[LootHistory] failed to subscribe to events:', err);
    }
  }

  destroy(): void {
    for (const u of this.#unlisteners) u();
    this.#unlisteners = [];
    this.#initialized = false;
  }

  async clear(): Promise<void> {
    await invoke('clear_loot_history');
  }

  #keyFor(entry: { seed: number; unit_id: number }): string {
    return entry.seed !== 0 ? `s:${entry.seed}` : `u:${entry.unit_id}`;
  }

  #replaceAll(items: LootHistoryEntry[]) {
    this.entries = items;
    this.#index.clear();
    items.forEach((e, i) => this.#index.set(this.#keyFor(e), i));
  }

  #append(entry: LootHistoryEntry) {
    const key = this.#keyFor(entry);
    if (this.#index.has(key)) return;
    this.#index.set(key, this.entries.length);
    this.entries = [...this.entries, entry];
  }

  #applyUpdate(update: LootHistoryUpdate) {
    const idx = this.#index.get(this.#keyFor(update));
    if (idx === undefined) return;
    const next = this.entries.slice();
    next[idx] = { ...next[idx], pickup: update.pickup };
    this.entries = next;
  }
}

export const lootHistoryStore = new LootHistoryStore();
