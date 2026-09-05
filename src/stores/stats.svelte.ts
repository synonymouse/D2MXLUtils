import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface DamageStats {
  physMin1h: number;
  physMax1h: number;
  physMin2h: number;
  physMax2h: number;
  fireMin: number;
  fireMax: number;
  coldMin: number;
  coldMax: number;
  lightningMin: number;
  lightningMax: number;
  magicMin: number;
  magicMax: number;
  poisonMinPerSec: number;
  poisonMaxPerSec: number;
  strDamageBonusPct: number;
  dexDamageBonusPct: number;
}

export interface UnitStats {
  class: number;
  stats: Record<string, number>;
  baseStats: Record<string, number>;
  damage: DamageStats | null;
}

interface StatsPayload {
  player: UnitStats | null;
  merc: UnitStats | null;
}

/** Holds the last-known player/merc stats at module scope (outside any tab's
 *  component tree) so switching away from the Stats tab and back shows the
 *  cached snapshot instantly instead of resetting to a blank/loading state
 *  and waiting for a fresh poll cycle — polling itself still stops while the
 *  tab isn't visible (see `startPolling`/`stopPolling`), only the display
 *  data survives the tab unmounting. */
class StatsStore {
  player = $state<UnitStats | null>(null);
  merc = $state<UnitStats | null>(null);
  gameStatus = $state<'unknown' | 'ingame' | 'menu'>('unknown');
  /** Distinguishes "no stats-update has arrived yet this game session" (show
   *  a loading skeleton) from "one arrived and this entity genuinely has no
   *  data" (e.g. no mercenary hired). */
  receivedFirstPayload = $state(false);

  #unlisteners: UnlistenFn[] = [];
  #initialized = false;

  /** Subscribes to `stats-update`/`game-status` once for the app's lifetime.
   *  Safe to call from every Stats tab mount — later calls are no-ops. */
  async initListeners(): Promise<void> {
    if (this.#initialized) return;
    this.#initialized = true;

    try {
      const status = await invoke<unknown>('get_game_status');
      if (status === 'ingame' || status === 'menu') {
        this.gameStatus = status;
      }

      this.#unlisteners.push(
        await listen<StatsPayload>('stats-update', (event) => {
          this.player = event.payload.player;
          this.merc = event.payload.merc;
          this.receivedFirstPayload = true;
        }),
      );

      this.#unlisteners.push(
        await listen<string>('game-status', (event) => {
          this.gameStatus = event.payload as typeof this.gameStatus;
          if (this.gameStatus !== 'ingame') {
            this.player = null;
            this.merc = null;
            this.receivedFirstPayload = false;
          }
        }),
      );
    } catch (err) {
      console.error('[Stats] failed to subscribe:', err);
    }
  }

  /** Toggles the backend's expensive per-stat polling loop — tied to Stats
   *  tab visibility, unlike the listeners above which stay registered. */
  startPolling(): void {
    invoke('set_stats_polling', { enabled: true });
  }

  stopPolling(): void {
    invoke('set_stats_polling', { enabled: false });
  }
}

export const statsStore = new StatsStore();
