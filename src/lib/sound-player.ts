/**
 * Drop-notification sound player.
 *
 * The loot filter DSL lets rules specify `sound1`..`sound6` (see
 * `docs/filter_spec/loot-filter-dsl.md`). This module resolves those indices
 * to the bundled MP3s under `/sounds/` and plays them at the given volume.
 *
 * Overlapping plays are supported via `cloneNode` so a new drop never cuts off
 * a previous one.
 */

const TOTAL_SOUNDS = 6;

let cache: HTMLAudioElement[] | null = null;

function ensureCache(): HTMLAudioElement[] {
  if (cache) return cache;
  cache = Array.from({ length: TOTAL_SOUNDS }, (_, i) => {
    const audio = new Audio(`/sounds/${i + 1}.mp3`);
    audio.preload = 'auto';
    return audio;
  });
  return cache;
}

export function playSound(index: number, volume: number): void {
  if (!Number.isInteger(index) || index < 1 || index > TOTAL_SOUNDS) return;
  if (!(volume > 0)) return;

  const source = ensureCache()[index - 1];
  const node = source.cloneNode(true) as HTMLAudioElement;
  node.volume = Math.max(0, Math.min(1, volume));
  void node.play().catch(() => {
    // Autoplay / decode errors are non-fatal - a missed blip is fine.
  });
}
