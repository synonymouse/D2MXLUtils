/**
 * Drop-notification sound player.
 *
 * Resolves slot indices (1-based) to either the bundled MP3s under
 * `/sounds/N.mp3` (for `Default` sources) or a custom file in
 * `app_data_dir/sounds/` (for `Custom` sources). `Empty` slots and
 * out-of-range indices are silent no-ops.
 *
 * Final played gain is `master * slot.volume`. Overlapping plays use
 * `cloneNode` so a new drop never cuts off a previous one.
 */

import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import { settingsStore, type SoundSlot } from '../stores/settings.svelte';

let appDataDirPath: string | null = null;
let appDataDirPromise: Promise<string> | null = null;

// Checked once and cached — see `should_use_native_audio`'s doc comment
// (sounds.rs) for why platforms it returns true for skip the `<audio>`
// element entirely rather than trying it first and falling back on
// failure: on some Linux/WebKitGTK builds the failure mode isn't a
// rejected play() promise (which the catch handler below could recover
// from) but the whole WebProcess aborting outright, taking the page down
// with it before any JS fallback gets a chance to run.
let useNativeAudioPromise: Promise<boolean> | null = null;
function useNativeAudio(): Promise<boolean> {
  if (!useNativeAudioPromise) {
    useNativeAudioPromise = invoke<boolean>('should_use_native_audio').catch(() => false);
  }
  return useNativeAudioPromise;
}

// Cache one Audio per (slot index, resolved URL). Invalidated when the
// resolved URL for an index changes (e.g. slot replaced or reset).
interface CacheEntry {
  url: string;
  audio: HTMLAudioElement;
}
const cache: Map<number, CacheEntry> = new Map();

function getAppDataDir(): Promise<string> {
  if (appDataDirPath !== null) return Promise.resolve(appDataDirPath);
  if (!appDataDirPromise) {
    appDataDirPromise = appDataDir().then((p) => {
      appDataDirPath = p.replace(/[\\/]+$/, '');
      return appDataDirPath;
    });
  }
  return appDataDirPromise;
}

function urlForSlot(slot: SoundSlot, index1: number, dir: string): string | null {
  switch (slot.source.kind) {
    case 'default':
      return `/sounds/${index1}.mp3`;
    case 'custom':
      return convertFileSrc(`${dir}/sounds/${slot.source.fileName}`);
    case 'empty':
      return null;
  }
}

/**
 * Native-playback path: fetches the same bytes the `<audio>` element
 * would have used and hands them to the Rust backend (`rodio`, talking to
 * ALSA/PulseAudio/PipeWire directly) instead. Used proactively on
 * platforms `useNativeAudio()` covers (see its comment), and as a
 * same-tick fallback elsewhere if `<audio>`/`Audio.play()` itself rejects.
 */
async function playViaNativeFallback(url: string, gain: number): Promise<void> {
  try {
    const resp = await fetch(url);
    const buf = await resp.arrayBuffer();
    await invoke('play_audio_bytes_native', {
      bytes: Array.from(new Uint8Array(buf)),
      volume: gain,
    });
  } catch (err) {
    console.warn(`[sound-player] native fallback failed for ${url}:`, err);
  }
}

/**
 * Play the audio for the given 1-based slot index at `master * slot.volume`.
 * Empty slots, missing slots, or zero gain are silent no-ops.
 */
export async function playSound(index1: number, masterVolume: number): Promise<void> {
  if (!Number.isInteger(index1) || index1 < 1) return;
  const slots = settingsStore.settings.sounds;
  const slot = slots[index1 - 1];
  if (!slot || slot.source.kind === 'empty') return;

  const gain = Math.max(0, Math.min(1, masterVolume * slot.volume));
  if (gain <= 0) return;

  const dir = await getAppDataDir();
  const url = urlForSlot(slot, index1, dir);
  if (!url) return;

  if (await useNativeAudio()) {
    void playViaNativeFallback(url, gain);
    return;
  }

  let entry = cache.get(index1);
  if (!entry || entry.url !== url) {
    const audio = new Audio(url);
    audio.preload = 'auto';
    entry = { url, audio };
    cache.set(index1, entry);
  }
  const node = entry.audio.cloneNode(true) as HTMLAudioElement;
  node.volume = gain;
  void node.play().catch((err) => {
    console.warn(`[sound-player] playback failed for slot ${index1}, falling back to native:`, err);
    void playViaNativeFallback(url, gain);
  });
}
