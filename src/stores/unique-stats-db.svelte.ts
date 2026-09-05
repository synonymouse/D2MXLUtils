/**
 * Sync state for the unique/set roll-range template DB (see
 * `unique_stats_db_sync.rs`). Much simpler than `updaterStore` — this is a
 * single small JSON file, not an executable, so no byte-progress streaming
 * or restart/self-replace dance, just check -> download -> done.
 *
 * A completed download does not apply to the currently running scanner
 * (it loads the DB once at startup into a plain field) — it takes effect
 * next launch. The 'downloaded' state's copy reflects that.
 */

import { invoke } from '@tauri-apps/api/core';

export type UniqueStatsDbState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'not_downloaded' }
  | { kind: 'up_to_date' }
  | { kind: 'available' }
  | { kind: 'downloading' }
  | { kind: 'downloaded' }
  | { kind: 'error'; message: string };

interface CheckResult {
  status: 'not_downloaded' | 'up_to_date' | 'available';
  asset_updated_at: string | null;
}

class UniqueStatsDbStore {
  private _state = $state<UniqueStatsDbState>({ kind: 'idle' });

  get state(): UniqueStatsDbState {
    return this._state;
  }

  async check(): Promise<void> {
    if (this._state.kind === 'checking' || this._state.kind === 'downloading') return;
    this._state = { kind: 'checking' };
    try {
      const result = await invoke<CheckResult>('check_unique_stats_db_update');
      this._state = { kind: result.status };
    } catch (err) {
      this._state = { kind: 'error', message: String(err) };
    }
  }

  async download(): Promise<void> {
    if (this._state.kind === 'downloading') return;
    this._state = { kind: 'downloading' };
    try {
      await invoke('download_unique_stats_db');
      this._state = { kind: 'downloaded' };
    } catch (err) {
      this._state = { kind: 'error', message: String(err) };
    }
  }
}

export const uniqueStatsDbStore = new UniqueStatsDbStore();
