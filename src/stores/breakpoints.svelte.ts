import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { SpeedcalcTable } from '../lib/breakpoint-calc';

export interface BreakpointData {
  class: number;
  wclass: string;
  wsm: number;
  file_index: number;
  family_codes: string[];
  ias: number;
  fcr: number;
  fhr: number;
  fbr: number;
  skill_ias: number;
  skill_fhr: number;
  merc_type: number | null;
}

interface BreakpointsPayload {
  player: BreakpointData | null;
  merc: BreakpointData | null;
}

export interface WeaponBase {
  file_index: number;
  name: string;
  wclass: string;
  wsm: number;
  family_codes: string[];
}

/** Holds the last-known breakpoints/weapon-catalog/speedcalc data at module
 *  scope (outside the Breakpoints tab's component tree) so switching away
 *  from the tab and back shows the cached snapshot instantly instead of
 *  resetting to defaults and re-fetching/re-polling from scratch — same
 *  rationale as `statsStore`. */
class BreakpointsStore {
  player = $state<BreakpointData | null>(null);
  merc = $state<BreakpointData | null>(null);
  weaponBaseCatalog = $state<WeaponBase[]>([]);
  speedcalcTable = $state<SpeedcalcTable | null>(null);
  loadError = $state<string | null>(null);

  #unlisteners: UnlistenFn[] = [];
  #initialized = false;

  /** Subscribes + does the one-time catalog/speedcalc fetch once for the
   *  app's lifetime. Safe to call from every Breakpoints tab mount — later
   *  calls are no-ops. */
  async initListeners(): Promise<void> {
    if (this.#initialized) return;
    this.#initialized = true;

    invoke<SpeedcalcTable | null>('get_speedcalc_data').then((data) => {
      if (data && Object.keys(data).length > 0) {
        this.speedcalcTable = data;
        return;
      }
      invoke('refresh_speedcalc_data')
        .then(() => invoke<SpeedcalcTable | null>('get_speedcalc_data'))
        .then((freshData) => {
          if (freshData && Object.keys(freshData).length > 0) {
            this.speedcalcTable = freshData;
          } else {
            this.loadError = 'Failed to load breakpoint data';
          }
        })
        .catch((e) => {
          this.loadError = `Failed to fetch breakpoint data: ${e}`;
        });
    });

    invoke<WeaponBase[] | null>('get_weapon_base_catalog').then((data) => {
      if (data && data.length > 0) this.weaponBaseCatalog = data;
    });

    try {
      this.#unlisteners.push(
        await listen<BreakpointsPayload>('breakpoints-update', (event) => {
          this.player = event.payload.player;
          this.merc = event.payload.merc;
        }),
      );

      this.#unlisteners.push(
        await listen<WeaponBase[]>('weapon-base-catalog-updated', (event) => {
          if (event.payload && event.payload.length > 0) this.weaponBaseCatalog = event.payload;
        }),
      );
    } catch (err) {
      console.error('[Breakpoints] failed to subscribe:', err);
    }
  }

  /** Toggles the backend's per-stat polling loop — tied to Breakpoints tab
   *  visibility, unlike the listeners/catalog above which stay cached. */
  startPolling(): void {
    invoke('set_breakpoints_polling', { enabled: true });
  }

  stopPolling(): void {
    invoke('set_breakpoints_polling', { enabled: false });
  }
}

export const breakpointsStore = new BreakpointsStore();
