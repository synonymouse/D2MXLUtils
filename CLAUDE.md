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

The backend combines Windows/Linux native operations with portable feature logic:

- **`main.rs` / `app/`** — Main owns Tauri composition, AppState, initial filter loading and the close/join watchdog. App holds window/edit controls, window policy/tests, platform preparation and application commands. Private `scanner_runtime/` keeps discovery/auto-start at its entry, attach/coordination/shutdown in `worker.rs`, and borrowed readout/DPS sampling in `readouts.rs`.
- **`notifier/`** — DropScanner owns item discovery, processing, pickup and catalog/event preparation; private dictionary mirroring sits beside its producer. Private `visibility/` co-locates controls/reveal, the cohesive native `hook/` and pure `tracker/`; their internal exports reach only notifier. The dictionary remains loadable without a game.
- **`map_markers/`** — Private `scanner.rs` coordinates BFS; `manager/` remains the sole marker-chain owner with child-detach/diagnostics and nearby tests. Windows fixture consumers use `map_markers::test_support::{Fixture,calls}`. Logical/native manager groups deliberately stay together.
- **`dps/` / `item_search/`** — DPS co-locates accumulator, native hook/ring/trampoline and reset watcher. Item search co-locates private API state/transport/response/index, cohesive tooltip `capture/`, and search watcher; manual API queries are independent of capture. Keep portable entries/tests outside native enclosing gates.
- **`loot_history/`** — Pilot pattern: entry DTOs/cap/timestamp, private indexed state and complete transitions in `history.rs`, neighboring interface tests; complete toggle watcher in `hotkey.rs`.
- **`breakpoints/` / `damage_stats/` / `stats_panel/`** — Distinct readouts with neighboring Windows acquisition tests. Breakpoints contains weapon helpers (also consumed by the other readouts), weapon-family catalog/cache and speed-calculator loader/cache. Character-sheet aggregate/scaling/recovery stays over private inventory/base reads; Damage stays cohesive.
- **`unit_stats_reader/` / `stat_telemetry/`** — Shared acquisition keeps private snapshot/adjustment, production-used `fallback`, and nearby tests/fixtures. Telemetry keeps counters, lifecycle, rendering and memory sampling with nearby tests. Readouts/notifier retain acquisition/fallback decisions; injector records attempts and runtime samples telemetry.
- **`process/` / `injection/`** — Shared process/context, platform I/O, X11 and ptrace serve scanners/readouts/hooks/input; injector keeps resource fields, platform execution, game calls and ordered installation. Windows test interception/allocator support retain their existing seams; seven Linux live probes in `process/live_probe.rs` remain ignored.
- **`hotkeys/` / `game_create/`** — Shared input owns config and chord/focus predicates over private platform mechanics. Features own complete watchers/state/commands; game-create owns menu-gated autofill and its sole-use `input.rs`.
- **`tick_clock.rs` / `remote_io.rs` / `scanner_state.rs`** — DPS and hover share clock and borrowed process I/O; hook allocation/protection stays with each owner. Scanner state joins notifier/markers and DPS/search/annotation; never hold `injector` and `recent_events` locks simultaneously.
- **`rules/`** — Entry types/compiled state, private decisions and DSL parsing/attributes/tokens/validation; cohesive matching/explanation files. Neighboring tests retain their original logical owners, including explicit relative test paths.
- **`settings/` / `profiles/` / `migrations/`** — Settings persistence/commands over private schema/defaults; profile workflow over exact private starter text; ordered application persistence migrations with nearby widget-position tests.
- **`unique_stats_db/` / `updater/` / `sounds/`** — Separate workflows: annotation plus private storage/sync (download does not refresh the attached scanner DB); updater lifecycle plus release/install/progress; compact sound files/playback with neighboring tests where they exist.
- **`d2types/` / `offsets.rs` / `logger.rs`** — Shared layouts/data catalog with nearby layout test, searchable offsets, and file logging with caller tracking.

#### Backend organization

- **Business-feature entry:** group related owners behind one `<feature>/mod.rs` with private children; remove obsolete physical entries. Expose required symbols explicitly and update internal callers when grouping. Truly shared infrastructure may remain a separate module.
- **Private implementation:** keep focused children private and grant only the narrow internal access needed. Keep related state and invariants with their existing owner.
- **Cohesion:** split by responsibility, never arbitrary file size. Compact cohesive modules may stay flat; when only tests need another file, production may remain together in the entry.
- **Nearby tests:** place existing tests and fixture builders in separate neighboring files. Declare them beneath the owner when private access is needed instead of widening production visibility.
- **Interface seam:** test through the existing useful interface and preserve scenarios, assertions, names and target/ignore gates. Add tests only for a concrete uncovered invariant; file moves do not justify automatic test growth.
- **Locality, leverage, depth:** co-location makes related behavior easier to find; leverage means one change reaches its consumers; depth means an interface hides useful knowledge. A file split alone improves locality, not necessarily depth.
- **Deletion test:** before adding an abstraction, ask whether deleting it loses hidden knowledge or merely removes forwarding. Retain abstractions that hide useful complexity.
- **Adapter seams:** one adapter is a hypothetical seam; two actually used adapters justify a real one. Introduce an adapter interface only for demonstrated needs.
- **Relocation contracts:** preserve serialization, command/event paths, cfg gates, lock/drop scopes and operation order. Check source-relative assets, generated command registration and logger call-site tracking when moving their owners.
- **Verification:** capture baseline discovery/results, compare names on the same target after moving tests, run focused checks and a connected Windows-target build, then the full backend suite per feature stage. Record platform gaps explicitly.

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

`.github/workflows/auto-release.yml` cuts a release only when a PR merged to
`master` has exactly one explicit release label:

- **`release:patch`** — bugfix release.
- **`release:minor`** — feature release.
- **`release:major`** — breaking release.
- **`release:skip`** — explicit documentation that the PR must not release.

An unlabeled PR does not release. Never combine `release:skip` with a release
label or apply more than one release label; the workflow rejects ambiguous
combinations. Directly pushing a semver tag remains the manual release path.

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
