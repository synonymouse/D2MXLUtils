import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

type Source = 'empty' | 'cache' | 'live';

class ItemsDictionaryStore {
  private _items = $state<string[]>([]);
  private _source = $state<Source>('empty');
  private _unlisten: UnlistenFn | null = null;
  private _initialized = false;

  get items(): string[] {
    return this._items;
  }

  get source(): Source {
    return this._source;
  }

  async init(): Promise<void> {
    if (this._initialized) return;
    this._initialized = true;

    try {
      const cached = await invoke<string[]>('get_items_dictionary');
      if (cached.length > 0) {
        this._items = cached;
        this._source = 'cache';
      }
    } catch (error) {
      console.error('[ItemsDictionary] Failed to load from backend:', error);
    }

    try {
      this._unlisten = await listen<string[]>('items-dictionary-updated', (event) => {
        if (Array.isArray(event.payload)) {
          this._items = event.payload;
          this._source = 'live';
        }
      });
    } catch (error) {
      console.error('[ItemsDictionary] Failed to subscribe to updates:', error);
    }
  }

  destroy(): void {
    this._unlisten?.();
    this._unlisten = null;
    this._initialized = false;
  }
}

export const itemsDictionaryStore = new ItemsDictionaryStore();
