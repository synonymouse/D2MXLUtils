<script lang="ts">
  interface Props {
    /** Anchor x position as percentage of overlay width (0-100). */
    x: number;
    /** Anchor y position as percentage of overlay height (0-100). */
    y: number;
    onchange: (x: number, y: number) => void;
  }

  let { x, y, onchange }: Props = $props();

  const MARKER_WIDTH_PX = 300;
  const MARKER_HEIGHT_PX = 80;

  let dragging = $state(false);
  let dragOffsetX = 0;
  let dragOffsetY = 0;

  function clamp(v: number, lo: number, hi: number): number {
    return Math.min(Math.max(v, lo), hi);
  }

  function onMarkerMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    dragOffsetX = e.clientX - rect.left;
    dragOffsetY = e.clientY - rect.top;
    dragging = true;
  }

  function onWindowMouseMove(e: MouseEvent) {
    if (!dragging) return;
    const w = window.innerWidth;
    const h = window.innerHeight;
    if (w === 0 || h === 0) return;

    const pxX = e.clientX - dragOffsetX;
    const pxY = e.clientY - dragOffsetY;

    const maxXPct = 100 - (MARKER_WIDTH_PX / w) * 100;
    const maxYPct = 100 - (MARKER_HEIGHT_PX / h) * 100;

    const nx = clamp((pxX / w) * 100, 0, Math.max(0, maxXPct));
    const ny = clamp((pxY / h) * 100, 0, Math.max(0, maxYPct));

    onchange(nx, ny);
  }

  function onWindowMouseUp() {
    dragging = false;
  }
</script>

<svelte:window onmousemove={onWindowMouseMove} onmouseup={onWindowMouseUp} />

<div class="edit-grid" class:dragging>
  <div
    class="marker"
    style="top: {y}%; left: {x}%; width: {MARKER_WIDTH_PX}px; height: {MARKER_HEIGHT_PX}px;"
    onmousedown={onMarkerMouseDown}
    role="button"
    tabindex="-1"
    aria-label="Drag to reposition notification anchor"
  >
    <span class="marker-label">Notifications appear here — drag to move</span>
  </div>
</div>

<style>
  .edit-grid {
    position: fixed;
    inset: 0;
    pointer-events: auto;
    background-image:
      linear-gradient(to right, rgba(180, 180, 255, 0.12) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(180, 180, 255, 0.12) 1px, transparent 1px),
      linear-gradient(to right, rgba(180, 180, 255, 0.22) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(180, 180, 255, 0.22) 1px, transparent 1px);
    background-size:
      25px 25px,
      25px 25px,
      100px 100px,
      100px 100px;
    background-color: rgba(0, 0, 0, 0.25);
    z-index: 10000;
    cursor: crosshair;
  }

  .edit-grid.dragging {
    cursor: grabbing;
  }

  .marker {
    position: absolute;
    box-sizing: border-box;
    border: 2px dashed var(--accent-primary, #6aa3ff);
    background: rgba(106, 163, 255, 0.15);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-primary, #e0e0e0);
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    text-align: center;
    padding: var(--space-2, 8px);
    cursor: grab;
    user-select: none;
    pointer-events: auto;
    transition: background 120ms ease;
  }

  .marker:hover {
    background: rgba(106, 163, 255, 0.25);
  }

  .edit-grid.dragging .marker {
    cursor: grabbing;
    background: rgba(106, 163, 255, 0.35);
  }

  .marker-label {
    pointer-events: none;
  }
</style>
