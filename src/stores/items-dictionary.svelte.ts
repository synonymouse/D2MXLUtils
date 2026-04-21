import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

type Source = 'empty' | 'cache' | 'live';

export interface ItemsDictionary {
  base_types: string[];
  uniques_tu: string[];
  uniques_su: string[];
  uniques_ssu: string[];
  uniques_sssu: string[];
  set_items: string[];
}

export type AutocompleteKind = 'base' | 'set' | 'tu' | 'su' | 'ssu' | 'sssu';

export interface AutocompleteOption {
  label: string;
  kind: AutocompleteKind;
}

function emptyDict(): ItemsDictionary {
  return {
    base_types: [],
    uniques_tu: [],
    uniques_su: [],
    uniques_ssu: [],
    uniques_sssu: [],
    set_items: [],
  };
}

function isNonEmpty(dict: ItemsDictionary): boolean {
  return (
    dict.base_types.length > 0 ||
    dict.uniques_tu.length > 0 ||
    dict.uniques_su.length > 0 ||
    dict.uniques_ssu.length > 0 ||
    dict.uniques_sssu.length > 0 ||
    dict.set_items.length > 0
  );
}

class ItemsDictionaryStore {
  private _dict = $state<ItemsDictionary>(emptyDict());
  private _source = $state<Source>('empty');
  private _unlisten: UnlistenFn | null = null;
  private _initialized = false;

  get dict(): ItemsDictionary {
    return this._dict;
  }

  get source(): Source {
    return this._source;
  }

  get options(): AutocompleteOption[] {
    const out: AutocompleteOption[] = [];
    for (const label of this._dict.base_types) out.push({ label, kind: 'base' });
    for (const label of this._dict.set_items) out.push({ label, kind: 'set' });
    for (const label of this._dict.uniques_tu) out.push({ label, kind: 'tu' });
    for (const label of this._dict.uniques_su) out.push({ label, kind: 'su' });
    for (const label of this._dict.uniques_ssu) out.push({ label, kind: 'ssu' });
    for (const label of this._dict.uniques_sssu) out.push({ label, kind: 'sssu' });
    return out;
  }

  async init(): Promise<void> {
    if (this._initialized) return;
    this._initialized = true;

    try {
      const cached = await invoke<ItemsDictionary>('get_items_dictionary');
      if (cached && isNonEmpty(cached)) {
        this._dict = cached;
        this._source = 'cache';
      }
    } catch (error) {
      console.error('[ItemsDictionary] Failed to load from backend:', error);
    }

    try {
      this._unlisten = await listen<ItemsDictionary>('items-dictionary-updated', (event) => {
        const payload = event.payload;
        if (payload && typeof payload === 'object' && Array.isArray(payload.base_types)) {
          this._dict = payload;
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
