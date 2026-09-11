# Overlay Reposition Hit-Test Bug

## Status

**Single-window experiment active. The native frame artifact is fixed by
`shadow: false`; edit mode now removes `WS_EX_LAYERED` on the main overlay HWND.
In-game validation pending.**

We tried multiple targeted fixes around `WS_EX_LAYERED`, WebView2 sizing,
native hit-testing, DWM frame suppression, clipping, and CSS opacity. None of
the single-overlay-window variants tested before `shadow: false` satisfied both
requirements:

- no white/native frame around the transparent overlay;
- reliable drag/input across the full game window in reposition mode.

The current implementation re-tests the single-window path with the newly found
`shadow: false` fix. If the original large-window hit-test cases fail again,
return to the two-window split.

## Symptom

When the edit chord is held over Diablo II, the overlay grid and drag ghosts
render across the full game window. Mouse hover and drag only work in the
upper-left portion of the window, roughly around an 800x600-ish region.

Outside that region:

- ghost hover does not activate;
- `mousedown` does not reach the ghost;
- clicks pass through to the game.

The exact working fraction varies with game size:

| Game mode | Game window | Working input region |
| --- | --- | --- |
| Windowed | ~1400x1080 | upper-left ~1/4 |
| Fullscreen | ~2560x1440 | upper-left ~1/6 |

## Confirmed

- The visual WebView2 layer is full-size. Diagnostic HUD/corner-marker testing
  showed `window.innerWidth`, `documentElement.clientWidth`, and `.edit-grid`
  all matching the game window size.
- CSS layout is not the blocker. The grid spans the full window, and no known
  `pointer-events: auto` element covers the dead area.
- Increasing ghost opacity did not expand the working input area.
- Returning `HTCLIENT` from `WM_NCHITTEST` did not expand the working input
  area.
- Calling Tauri `set_position`/`set_size` after native `MoveWindow` did not
  expand the working input area.
- Removing `WS_EX_LAYERED` in interactive mode makes input work. The earlier
  white/native frame artifact was caused by the undecorated Tauri window shadow;
  `shadow: false` removes it.
- Keeping `WS_EX_LAYERED` avoids the frame artifact, but input remains broken
  outside the upper-left area.

## Failed Experiments

### 1. Resize WebView2 Controller Bounds

Theory: WebView2's controller/host stayed at its initial size while the visual
surface stretched.

Attempt: force WebView2 controller bounds / parent window size after overlay
sync.

Result: no change. The visual viewport was already full-size, and input stayed
broken.

### 2. Remove Layered Style In Interactive Mode

Theory: `WS_EX_LAYERED` alpha hit-testing was making transparent pixels
click-through.

Attempt: in reposition mode remove both `WS_EX_TRANSPARENT` and
`WS_EX_LAYERED`.

Result: hit-testing improved, but a white/native frame appeared around the
overlay. Later testing showed this frame was the undecorated Tauri window shadow,
not a fundamental cost of removing `WS_EX_LAYERED`.

### 3. Suppress Or Hide The White Frame

Attempts:

- DWM border suppression via `DWMWA_BORDER_COLOR`;
- window region clipping via `SetWindowRgn`;
- outward overscan / resize tricks;
- removing first-show bump in interactive mode.

Result: the frame either remained or moved. These were symptoms, not a root fix;
the actual fix was disabling the Tauri window shadow.

### 4. Keep Layered, Remove Only Transparent

Theory: keep `WS_EX_LAYERED` for clean transparency, remove only
`WS_EX_TRANSPARENT` for input, and make ghosts more opaque so layered alpha
hit-testing sees them.

Attempt: interactive mode kept `WS_EX_LAYERED`, removed `WS_EX_TRANSPARENT`,
and increased ghost background opacity.

Result: old hit-test bug returned unchanged.

### 5. Override Native Hit-Test

Theory: `WM_NCHITTEST` could force Windows to treat transparent edit-mode
areas as client area.

Attempt: subclass overlay HWND and return `HTCLIENT` for `WM_NCHITTEST` while
interactive.

Result: no change. Removed after testing. Either messages do not reach this
window in the dead area, or the relevant hit-test decision happens below/around
WebView2/DComp before normal WndProc handling can help.

### 6. Force Tauri Bounds After Native MoveWindow

Theory: raw `MoveWindow` updated the top-level HWND but did not notify
Tauri/WebView2 correctly, leaving a stale logical input surface.

Attempt: after each real `MoveWindow`, call `overlay_window.set_position(...)`
and `overlay_window.set_size(...)` with physical bounds.

Result: no change. The stale-input theory may still be directionally related,
but Tauri's public position/size API did not fix it.

## Current Conclusion

The reliable pattern is:

- `WS_EX_LAYERED` on: visual transparency is good, input is unreliable.
- `WS_EX_LAYERED` off: input is good. The frame artifact is avoidable with
  `shadow: false` on the undecorated Tauri window.

That points to a platform/architecture mismatch in using a single transparent
WebView2/Tauri window as both:

- a click-through overlay for notifications;
- a full-window interactive drag surface.

Single-window edit mode is now being re-tested specifically with `shadow: false`.
It still needs large-window/fullscreen hit-test validation before treating this
as the final architecture.

## Current Architecture: Single Window Experiment

Use one Tauri overlay window and switch its native styles by mode.

### 1. Normal Overlay Mode

- Existing transparent click-through window.
- Keeps `WS_EX_LAYERED + WS_EX_TRANSPARENT`.
- Shows normal notifications, DPS meter, indicators, and final widget
  positions.

### 2. Edit/Reposition Mode

- Same `overlay` HWND/window.
- Removes both `WS_EX_LAYERED` and `WS_EX_TRANSPARENT` while the edit chord is
  active.
- Keeps `shadow: false` so the undecorated window does not draw a 1px native
  border/shadow.
- Renders `OverlayEditGrid` inside `OverlayWindow.svelte` during edit mode.
- Restores the layered/click-through visual overlay mode when edit mode exits.

This tests whether the old hit-test failure was caused by layered WebView2 input
alone, while the frame artifact was independently caused by Tauri's default
window shadow.

## Implementation Notes

- `tauri.conf.json` defines only the main hidden `overlay` window; the temporary
  `overlay-edit` window was removed.
- The main `overlay` renders `OverlayEditGrid` while the edit chord is active.
- `set_overlay_edit_mode` now switches native styles on the main overlay HWND
  instead of showing/hiding a helper window.
- `shadow: false` in `tauri.conf.json` removes the 1px native border/shadow
  around the undecorated Windows overlay.
- Overlay sync hides the overlay when D2 is minimized (`IsIconic`) even if
  `GetForegroundWindow` still briefly reports the game HWND.
- Overlay sync strips leaked caption/chrome bits every tick, not only during
  edit-mode style transitions, so a native title bar cannot linger.
- The old 8px outward overscan workaround was only hiding the native edge and is
  no longer needed.
- Manual validation still needs to confirm drag works across the full D2 window
  in the large windowed/fullscreen cases that reproduced the original bug.

## Relevant Files

- `src-tauri/src/app/windows.rs`: overlay window sync, style toggling,
  `set_overlay_interactive`, `set_overlay_edit_mode`.
- `src-tauri/src/app/overlay_window_tests.rs`: existing style/focus policy checks.
- `src/views/OverlayWindow.svelte`: main visual overlay, loot-history
  interactivity, and edit-grid rendering.
- `src/components/OverlayEditGrid.svelte`: grid and ghost host.
- `src/components/DragGhost.svelte`: drag primitive.
- `src/lib/overlay-widgets.ts`: widget registry.
- `src/stores/widget-positions.svelte.ts`: persisted widget positions.

## Cleanup Notes

If the old dead input region returns in this single-window experiment, restore
the two-window architecture: layered/click-through visual overlay plus separate
non-layered edit/input window.
