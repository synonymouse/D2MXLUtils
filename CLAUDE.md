# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**D2MXLUtils** is an overlay utility for *Diablo II: Median XL* that provides drop notifications and loot filtering. It's a rewrite of a legacy AutoIt script (`D2Stats.au3`) using modern technologies.

## Tech Stack

- **Frontend**: Svelte 5 + TypeScript + vanilla CSS (variables + themes)
- **Desktop Shell**: Tauri v2
- **Backend**: Rust (using `windows` crate for WinAPI)
- **Package Manager**: pnpm

## Development Commands

```bash
pnpm install          # Install dependencies
pnpm tauri dev        # Run the app in dev mode (launches Vite + Tauri)
pnpm tauri build      # Build release version
pnpm check            # Type-check (svelte-check)
pnpm format:all       # Format TS/Svelte (prettier) + Rust (cargo fmt) — see Formatting below
```

## Starting new work

Before touching code for a new task, sync with the remote and branch off `master`
(this repo's default/main branch):

```bash
git checkout master
git pull
git checkout -b <type>/<short-description>
```

- Branch names follow the same prefixes as commits: `feat/`, `fix/`, `refactor/`,
  `docs/`, `chore/` (e.g. `feat/rules-editor-keyword-autocomplete`,
  `fix/map-marker-persist-across-acts`).
- Run `git status` before switching branches; stash or commit anything uncommitted
  first so it isn't carried onto (or lost from) the new branch.
- Never branch off a stale local `master` — always `git pull` right before
  `checkout -b`.

## Architecture

### Rust Backend (`src-tauri/src/`)

The backend handles all low-level Windows operations:

- **`main.rs`** — Tauri app setup, commands, scanner lifecycle, overlay window management, UAC elevation handling
- **`process.rs`** — D2 process attachment via WinAPI (`OpenProcess`, `ReadProcessMemory`)
- **`injection.rs`** — Remote thread injection into D2 process to call internal game functions (e.g. `GetStringById` to resolve localized names)
- **`notifier.rs`** — `DropScanner` that scans item unit lists and emits `item-drop` events; also builds `class_cache` over `items.txt` which backs both drop notifications and the editor autocomplete dictionary (`items_dictionary_snapshot`)
- **`scanner_state.rs`** — State shared by the items and marker scanner threads (`injector` and `recent_events` locks must never be held simultaneously)
- **`marker_scanner.rs`** / **`map_marker.rs`** — Automap markers for loot-filter matches: BFS over the room graph, reconciles the marker chain, allocates `AutomapCell`s via the injector
- **`dps_meter.rs`** / **`dps_hook/`** — Pure-data DPS accumulator fed by a trampoline hook on HP writes (`dps_hook/trampoline.rs`, `ring.rs`)
- **`hovered_item.rs`** — Captures the currently hovered in-game item (D2Sigma tooltip hook) for targeted MXL item search
- **`mxl_item_api.rs`** — Backend for the in-game Median XL item database search overlay (calls the public item API, normalizes/caches/rate-limits)
- **`loot_history.rs`** — Session-only loot history: items that fired a `notify` rule, with pickup state resolved against the local player's inventory
- **`sounds.rs`** — Drop-sound file management (`app_data_dir/sounds/slot-{N}.{ext}`)
- **`weapon_families.rs`** — On-disk catalog of every weapon record in `items.txt` with family chain and WSM, built once per game attach (backs the Breakpoints tab)
- **`unique_stats_db.rs`** / **`unique_stats_db_sync.rs`** — Local DB of unique/set item stat-roll templates; built offline by `scripts/generate-unique-stats-db.mjs` and kept in sync with a maintainer-published copy on GitHub rather than every client crawling the third-party API
- **`updater.rs`** — Auto-updater: checks GitHub releases, downloads the platform asset, atomically replaces the running executable, restarts
- **`keystroke_sim.rs`** — Cross-platform synthetic keystroke injection for the "autofill game create" hotkey
- **`rules/`** — Loot filter rule engine: DSL parsing (`dsl.rs`), rule matching (`matching.rs`), hover explanations (`explain.rs`)
- **`d2types.rs`** — `#[repr(C)]` structs for D2 memory structures (`UnitAny`, `ItemData`, etc.)
- **`offsets.rs`** — D2 memory offsets (DLL bases, unit lists, item data pointers, `items.txt` layout)
- **`logger.rs`** — File logger writing to `d2mxlutils.log` next to the exe
- **`settings.rs`** — App settings persistence
- **`profiles.rs`** — Loot filter profile management
- **`items_cache.rs`** — On-disk mirror of the items-dictionary snapshot (`items-cache.json` in `app_data_dir`) so editor autocomplete works in sessions without D2 attached
- **`hotkeys.rs`** — Global hotkey handling
- **`migrations/`** — Versioned migrations for settings/state shape changes (e.g. widget positions, loot-history alt-nemesis)

### Svelte Frontend (`src/`)

- **`App.svelte`** — Entry point, routes to `MainWindow` or `OverlayWindow` based on Tauri window label
- **`views/`** — Main window tabs (`GeneralTab`, `LootFilterTab`, `NotificationsTab`, `SoundsTab`, `BreakpointsTab`) plus `MainWindow` and `OverlayWindow`
- **`components/`** — Reusable UI components (Button, Toggle, Tabs, etc.)
- **`editor/`** — CodeMirror-based loot filter rules editor: DSL language (`d2rules-language.ts`), linter (`d2rules-linter.ts`), theme (`d2rules-theme.ts`), group-rule code folding (`d2rules-folding.ts`), line hover explanations (`d2rules-hover.ts`), autocomplete for both item names inside quoted patterns and bare DSL keywords (`d2rules-autocomplete.ts`)
- **`stores/`** — Svelte stores: `settingsStore`, `windowState`, `itemsDictionaryStore` (editor autocomplete), `updaterStore`, `lootHistoryStore`, `dpsMeterStore`, `uniqueStatsDbStore`

### Communication

- **Tauri Commands**: Frontend calls Rust via `invoke()` (e.g., `set_filter_config`, `get_scanner_status`, `get_items_dictionary`)
- **Events**: Backend emits events to frontend via `app_handle.emit()` (e.g., `item-drop`, `scanner-status`, `items-dictionary-updated`)

## Important Conventions

### Formatting

Never run `cargo fmt` directly, in any mode. This includes `cargo fmt`,
`cargo fmt --check`, `cargo fmt --manifest-path ...`, and any equivalent
command whose purpose is to invoke Rust formatting.

Do not run repository-wide auto-format write commands unless the user explicitly
asks for formatting in the current turn. This includes `pnpm format`,
`prettier --write`, and equivalent formatter write modes.

Formatter check commands are allowed when verifying work only if they do not
invoke `cargo fmt`.

Rust formatting is enforced by `.husky/pre-commit`, which runs
`pnpm format:all`. Do not bypass the hook unless the user explicitly asks.

### Git Commits

**Never run `git commit` without an explicit request from the user in the current turn.**
Staging, reviewing diffs, and writing commit messages are fine — but the actual
`git commit` must wait for the user to say "commit", "закоммить", or equivalent.
A previous approval does not carry over: each commit needs its own green light.

Prefer **Conventional Commits** (`feat:`, `fix:`, `refactor:`, `docs:`, `perf:`,
`test:`, `style:`, `build:`, `ci:`, `chore:`, optional `(scope)`, `!` or
`BREAKING CHANGE:` footer for breaks). Keyword fallbacks in `cliff.toml` still
catch unprefixed messages (`Add`/`Fix`/`Move`/…), but new commits should use the
convention.

### Release Notes

Release notes are generated by **git-cliff** (`cliff.toml` at repo root). The
`.github/workflows/release.yml` pipeline runs git-cliff on tag push (`v*.*.*`)
and feeds `CHANGES.md` into the GitHub Release body. To preview locally:

```bash
git-cliff --config cliff.toml --unreleased --strip header
```

Grouping is driven by `commit_parsers` in `cliff.toml` — edit there to adjust
sections or skip more noise.

### Pull Request Labels / Auto-Release

`.github/workflows/auto-release.yml` cuts a release (version bump, tag, build)
on **every** PR merged to `master`, unless the PR carries a label that changes
that:

- **`release:skip`** — merge does not bump the version or trigger a build.
  Apply this automatically to any PR that only touches documentation (e.g.
  `CLAUDE.md`, `AGENTS.md`, `README.md`, `docs/**`) with no source/build
  changes — no need to wait for the user to ask.
- **`release:minor`** / **`release:major`** — merge bumps minor/major instead
  of the default patch. Apply when the change warrants it.

### Logging in Rust Backend

**Do NOT use `println!` / `eprintln!` in production code.** Use the logger module:
```rust
use crate::logger::{info as log_info, error as log_error};

log_info("Scanner started");
log_error(&format!("Failed to open process: {}", err));
```

Exception: `logger.rs` itself may use println/eprintln for stdout mirroring.

### Documentation

- `docs/filter_spec/` — Loot filter DSL specification
- `docs/*.md` — Reverse-engineering notes and investigation write-ups for tricky
  subsystems (DPS meter, map markers, loot history, overlay hit-testing, MXL item
  search); check there before re-deriving offsets or behavior from scratch
