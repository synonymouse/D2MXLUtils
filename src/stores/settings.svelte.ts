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
  /** Enable sound effects for item drops */
  soundEnabled: boolean;
  /** Sound volume (0.0 - 1.0) */
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
  /** Hotkey configuration for toggling main window */
  toggleWindowHotkey: HotkeyConfig;
  /** Global loot filter mode: true = Show All (default_show_items), false = Hide All */
  defaultShowItems: boolean;
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

/** Default settings */
const DEFAULT_SETTINGS: AppSettings = {
  theme: 'dark',
  soundEnabled: true,
  soundVolume: 0.8,
  activeProfile: null,
  notificationDuration: 5000,
  notificationStackDirection: 'up',
  notificationFontSize: 14,
  notificationOpacity: 0.9,
  notificationX: 2.0,
  notificationY: 50.0,
  toggleWindowHotkey: DEFAULT_HOTKEY,
  defaultShowItems: true,
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

  /** Get sound enabled state */
  get soundEnabled(): boolean {
    return this._settings.soundEnabled;
  }

  /** Set sound enabled state */
  setSoundEnabled(enabled: boolean): void {
    this.set('soundEnabled', enabled);
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

