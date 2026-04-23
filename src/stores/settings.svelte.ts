/**
 * Settings store for D2MXLUtils
 * 
 * Manages application settings with persistence through Tauri backend.
 * Uses Svelte 5 runes for reactive state management.
 */

import { invoke } from '@tauri-apps/api/core';

/** Hotkey configuration interface */
export interface HotkeyConfig {
  /** Virtual key code (e.g., 0x4B for 'K') */
  keyCode: number;
  /** Modifier flags (Ctrl, Shift, Alt, Win) */
  modifiers: number;
  /** Human-readable representation (e.g., "Ctrl+K") */
  display: string;
}

/** Application settings interface */
export interface AppSettings {
  /** UI theme: "dark" or "light" */
  theme: string;
  /** Master volume for drop notification sounds (0.0 - 1.0, 0 = silent) */
  soundVolume: number;
  /** Active loot filter profile name */
  activeProfile: string | null;
  /** Notification display duration in milliseconds */
  notificationDuration: number;
  /** Notification stack direction: "up" or "down" */
  notificationStackDirection: string;
  /** Notification font size in pixels */
  notificationFontSize: number;
  /** Notification background opacity (0.0 - 1.0) */
  notificationOpacity: number;
  /** Notification position X offset from edge (percentage 0-100) */
  notificationX: number;
  /** Notification position Y offset from edge (percentage 0-100) */
  notificationY: number;
  /** When true, drop the unique/set name line for Set/TU/SU/SSU/SSSU items
   *  and show only the base type. Stat-flagged rules ignore this. */
  compactName: boolean;
  /** Hotkey configuration for toggling main window */
  toggleWindowHotkey: HotkeyConfig;
  /** Hotkey held to enter overlay edit mode (drag notification anchor) */
  editOverlayHotkey: HotkeyConfig;
  /** When true, scanner logs per-item filter decisions (noisy; opt-in debug). */
  verboseFilterLogging: boolean;
}

/** Window state interface */
export interface WindowState {
  x: number;
  y: number;
  width: number;
  height: number;
  maximized: boolean;
}

/** Default hotkey (Ctrl+K) */
const DEFAULT_HOTKEY: HotkeyConfig = {
  keyCode: 0x4B,     // 'K' key
  modifiers: 0x0002, // MOD_CONTROL
  display: 'Ctrl+K',
};

/** Default edit-overlay hotkey (Ctrl+Alt, modifier-only — keyCode 0) */
const DEFAULT_EDIT_OVERLAY_HOTKEY: HotkeyConfig = {
  keyCode: 0,
  modifiers: 0x0001 | 0x0002, // MOD_ALT | MOD_CONTROL
  display: 'Ctrl+Alt',
};

/** Default settings */
const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  soundVolume: 0.8,
  activeProfile: null,
  notificationDuration: 5000,
  notificationStackDirection: 'up',
  notificationFontSize: 14,
  notificationOpacity: 0.9,
  notificationX: 1.0,
  notificationY: 1.0,
  compactName: false,
  toggleWindowHotkey: DEFAULT_HOTKEY,
  editOverlayHotkey: DEFAULT_EDIT_OVERLAY_HOTKEY,
  verboseFilterLogging: false,
};

/** Settings store singleton */
class SettingsStore {
  private _settings = $state<AppSettings>({ ...DEFAULT_SETTINGS });
  private _isLoaded = $state(false);
  private _isLoading = $state(false);
  private _saveTimeout: ReturnType<typeof setTimeout> | null = null;

  /** Current settings (reactive) */
  get settings(): AppSettings {
    return this._settings;
  }

  /** Whether settings have been loaded from backend */
  get isLoaded(): boolean {
    return this._isLoaded;
  }

  /** Whether settings are currently loading */
  get isLoading(): boolean {
    return this._isLoading;
  }

  /** Load settings from backend */
  async load(): Promise<void> {
    if (this._isLoading) return;
    
    this._isLoading = true;
    
    try {
      const loaded = await invoke<AppSettings>('load_settings');
      this._settings = { ...DEFAULT_SETTINGS, ...loaded };
      this._isLoaded = true;

      // Apply theme immediately
      this.applyTheme(this._settings.theme);
    } catch (error) {
      console.error('[Settings] Failed to load:', error);
      // Use defaults on error
      this._settings = { ...DEFAULT_SETTINGS };
      this._isLoaded = true;
    } finally {
      this._isLoading = false;
    }
  }

  /** Save settings to backend (debounced) */
  async save(): Promise<void> {
    // Clear any pending save
    if (this._saveTimeout) {
      clearTimeout(this._saveTimeout);
    }

    // Debounce saves by 500ms
    this._saveTimeout = setTimeout(async () => {
      try {
        await invoke('save_settings', { settings: this._settings });
      } catch (error) {
        console.error('[Settings] Failed to save:', error);
      }
    }, 500);
  }

  /** Update a single setting */
  set<K extends keyof AppSettings>(key: K, value: AppSettings[K]): void {
    this._settings = { ...this._settings, [key]: value };
    
    // Special handling for theme changes
    if (key === 'theme') {
      this.applyTheme(value as string);
    }
    
    // Auto-save after change
    this.save();
  }

  /** Update multiple settings at once */
  update(partial: Partial<AppSettings>): void {
    this._settings = { ...this._settings, ...partial };
    
    // Special handling for theme changes
    if ('theme' in partial) {
      this.applyTheme(partial.theme as string);
    }
    
    // Auto-save after change
    this.save();
  }

  /** Apply theme to the document */
  private applyTheme(theme: string): void {
    document.documentElement.setAttribute('data-theme', theme);
  }

  /** Get current theme */
  get theme(): string {
    return this._settings.theme;
  }

  /** Set theme */
  setTheme(theme: 'dark' | 'light'): void {
    this.set('theme', theme);
  }

  /** Toggle theme between dark and light */
  toggleTheme(): void {
    const newTheme = this._settings.theme === 'dark' ? 'light' : 'dark';
    this.setTheme(newTheme);
  }

  /** Get master sound volume (0.0 - 1.0) */
  get soundVolume(): number {
    return this._settings.soundVolume;
  }

  /** Set master sound volume (clamped to 0.0 - 1.0) */
  setSoundVolume(volume: number): void {
    const clamped = Math.max(0, Math.min(1, volume));
    this.set('soundVolume', clamped);
  }

  /** Get toggle window hotkey */
  get toggleWindowHotkey(): HotkeyConfig {
    return this._settings.toggleWindowHotkey;
  }

  /** Set toggle window hotkey */
  async setToggleWindowHotkey(hotkey: HotkeyConfig): Promise<void> {
    this.set('toggleWindowHotkey', hotkey);
    // Also update the backend hotkey listener
    try {
      await invoke('update_hotkey', { hotkey });
    } catch (error) {
      console.error('[Settings] Failed to update hotkey:', error);
    }
  }

  /** Get overlay-edit-mode hotkey */
  get editOverlayHotkey(): HotkeyConfig {
    return this._settings.editOverlayHotkey;
  }

  /** Set overlay-edit-mode hotkey */
  async setEditOverlayHotkey(hotkey: HotkeyConfig): Promise<void> {
    this.set('editOverlayHotkey', hotkey);
    try {
      await invoke('update_edit_mode_hotkey', { hotkey });
    } catch (error) {
      console.error('[Settings] Failed to update edit-mode hotkey:', error);
    }
  }

  /** Set notification anchor position (percentages 0-100) */
  setNotificationPosition(x: number, y: number): void {
    this.update({ notificationX: x, notificationY: y });
  }

  /** Enable/disable verbose per-item filter logging. Persists and flips the
   *  scanner-side atomic immediately (saved settings only seed on next
   *  startup, so we also push the change through a dedicated command). */
  async setVerboseFilterLogging(enabled: boolean): Promise<void> {
    this.set('verboseFilterLogging', enabled);
    try {
      await invoke('set_verbose_filter_logging', { enabled });
    } catch (error) {
      console.error('[Settings] Failed to update verbose filter logging:', error);
    }
  }
}

/** Global settings store instance */
export const settingsStore = new SettingsStore();

/**
 * Window state management utilities
 */
export const windowState = {
  /** Load window state from backend */
  async load(windowLabel: string): Promise<WindowState | null> {
    try {
      const state = await invoke<WindowState | null>('get_window_state', { windowLabel });
      return state;
    } catch (error) {
      console.error(`[WindowState] Failed to load for ${windowLabel}:`, error);
      return null;
    }
  },

  /** Save window state to backend */
  async save(windowLabel: string, state: WindowState): Promise<void> {
    try {
      await invoke('save_window_state', { windowLabel, state });
    } catch (error) {
      console.error(`[WindowState] Failed to save for ${windowLabel}:`, error);
    }
  },
};

