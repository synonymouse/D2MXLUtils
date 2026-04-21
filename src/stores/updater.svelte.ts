/**
 * Updater store for D2MXLUtils
 *
 * Reactive state machine for the auto-updater flow. Mirrors the Rust backend
 * in `src-tauri/src/updater.rs`.
 *
 * States:
 *   idle           — no check performed yet, or background check failed silently
 *   checking       — a check is in flight
 *   up_to_date     — latest stable release <= current version
 *   available      — newer stable release found; button shows "Обновление vX"
 *   downloading    — backend is streaming the new .exe (bytes counter)
 *   ready          — self_replace succeeded; button shows "Перезапустить"
 *   error          — only surfaced for MANUAL checks; auto checks fail to idle
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type UpdaterState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up_to_date'; current: string; checkedAt: Date }
  | { kind: 'available'; latest: string; current: string; assetUrl: string }
  | { kind: 'downloading'; latest: string; downloaded: number }
  | { kind: 'ready'; latest: string }
  | { kind: 'error'; message: string };

interface CheckResult {
  status: 'up_to_date' | 'available';
  latest_version: string | null;
  current_version: string;
  asset_url: string | null;
}

interface ProgressPayload {
  downloaded: number;
}

class UpdaterStore {
  private _state = $state<UpdaterState>({ kind: 'idle' });
  private _listenersAttached = false;
  private _unlisteners: UnlistenFn[] = [];

  get state(): UpdaterState {
    return this._state;
  }

  /**
   * Idempotently wire up backend event listeners. Call once from MainWindow
   * on mount; subsequent calls are no-ops.
   */
  async initListeners(): Promise<void> {
    if (this._listenersAttached) return;
    this._listenersAttached = true;

    const unProgress = await listen<ProgressPayload>('updater-progress', (e) => {
      const s = this._state;
      if (s.kind === 'downloading') {
        this._state = { ...s, downloaded: e.payload.downloaded };
      }
    });
    this._unlisteners.push(unProgress);

    const unReady = await listen('updater-ready', () => {
      const s = this._state;
      if (s.kind === 'downloading') {
        this._state = { kind: 'ready', latest: s.latest };
      }
    });
    this._unlisteners.push(unReady);

    const unError = await listen('updater-error', () => {
      // Silent revert: download/self_replace failed. Button disappears,
      // user can trigger a fresh check later.
      this._state = { kind: 'idle' };
    });
    this._unlisteners.push(unError);
  }

  destroyListeners(): void {
    for (const u of this._unlisteners) u();
    this._unlisteners = [];
    this._listenersAttached = false;
  }

  /**
   * Check GitHub for a newer release.
   *
   * @param manual  If true, surface errors to the UI (user clicked
   *                "Проверить обновления"). If false (automatic startup
   *                check), errors go to the log only and state stays `idle`.
   */
  async check(manual = false): Promise<void> {
    // Don't re-check while another check is in flight or while a download
    // is active/ready — those states own the UI.
    const k = this._state.kind;
    if (k === 'checking' || k === 'downloading' || k === 'ready') return;

    this._state = { kind: 'checking' };

    try {
      const result = await invoke<CheckResult>('check_for_updates', { manual });
      if (result.status === 'available' && result.latest_version && result.asset_url) {
        this._state = {
          kind: 'available',
          latest: result.latest_version,
          current: result.current_version,
          assetUrl: result.asset_url,
        };
      } else {
        this._state = {
          kind: 'up_to_date',
          current: result.current_version,
          checkedAt: new Date(),
        };
      }
    } catch (err) {
      const msg = typeof err === 'string' ? err : String(err);
      if (manual && msg !== 'silent') {
        this._state = { kind: 'error', message: msg };
      } else {
        // Automatic check: stay idle (button stays hidden).
        this._state = { kind: 'idle' };
      }
    }
  }

  /**
   * Kick off the download. Must be called from the `available` state —
   * other states either don't have an asset URL or already own the UI.
   */
  async install(): Promise<void> {
    const s = this._state;
    if (s.kind !== 'available') return;

    const latest = s.latest;
    const assetUrl = s.assetUrl;

    // Optimistically switch to downloading so the button reflects the click
    // instantly; progress events will fill in `downloaded` as they arrive.
    this._state = { kind: 'downloading', latest, downloaded: 0 };

    try {
      await invoke('start_update', { assetUrl });
    } catch (err) {
      console.error('[Updater] start_update failed:', err);
      // Revert — backend refused to start (e.g. already-downloading race).
      this._state = { kind: 'available', latest, current: '', assetUrl };
    }
  }

  /** Trigger the swap-exe-and-restart sequence on the backend. */
  async restart(): Promise<void> {
    if (this._state.kind !== 'ready') return;
    try {
      await invoke('restart_app');
    } catch (err) {
      console.error('[Updater] restart_app failed:', err);
    }
  }
}

export const updaterStore = new UpdaterStore();
