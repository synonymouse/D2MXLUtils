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
```

## Architecture

### Rust Backend (`src-tauri/src/`)

The backend handles all low-level Windows operations:

- **`main.rs`** — Tauri app setup, commands, scanner lifecycle, overlay window management, UAC elevation handling
- **`process.rs`** — D2 process attachment via WinAPI (`OpenProcess`, `ReadProcessMemory`)
- **`injection.rs`** — Remote thread injection into D2 process to call internal game functions (e.g. `GetStringById` to resolve localized names)
- **`notifier.rs`** — `DropScanner` that scans item unit lists and emits `item-drop` events; also builds `class_cache` over `items.txt` which backs both drop notifications and the editor autocomplete dictionary (`items_dictionary_snapshot`)
- **`rules/`** — Loot filter rule engine: DSL parsing (`dsl.rs`), rule matching (`matching.rs`)
- **`d2types.rs`** — `#[repr(C)]` structs for D2 memory structures (`UnitAny`, `ItemData`, etc.)
- **`offsets.rs`** — D2 memory offsets (DLL bases, unit lists, item data pointers, `items.txt` layout)
- **`logger.rs`** — File logger writing to `d2mxlutils.log` next to the exe
- **`settings.rs`** — App settings persistence
- **`profiles.rs`** — Loot filter profile management
- **`items_cache.rs`** — On-disk mirror of the items-dictionary snapshot (`items-cache.json` in `app_data_dir`) so editor autocomplete works in sessions without D2 attached. See `docs/autocomplete.md`.
- **`hotkeys.rs`** — Global hotkey handling

### Svelte Frontend (`src/`)

- **`App.svelte`** — Entry point, routes to `MainWindow` or `OverlayWindow` based on Tauri window label
- **`views/`** — Main window tabs (`GeneralTab`, `LootFilterTab`, `NotificationsTab`) and overlay
- **`components/`** — Reusable UI components (Button, Toggle, Tabs, etc.)
- **`editor/`** — CodeMirror-based loot filter rules editor: DSL language (`d2rules-language.ts`), linter (`d2rules-linter.ts`), theme (`d2rules-theme.ts`), autocomplete for item names inside quoted patterns (`d2rules-autocomplete.ts`)
- **`stores/`** — Svelte stores for settings, window state, and the items dictionary used by editor autocomplete (`items-dictionary.svelte.ts`)

### Communication

- **Tauri Commands**: Frontend calls Rust via `invoke()` (e.g., `set_filter_config`, `get_scanner_status`, `get_items_dictionary`)
- **Events**: Backend emits events to frontend via `app_handle.emit()` (e.g., `item-drop`, `scanner-status`, `items-dictionary-updated`)

## Important Conventions

### Git Commits

**Never run `git commit` without an explicit request from the user in the current turn.**
Staging, reviewing diffs, and writing commit messages are fine — but the actual
`git commit` must wait for the user to say "commit", "закоммить", or equivalent.
A previous approval does not carry over: each commit needs its own green light.

### Logging in Rust Backend

**Do NOT use `println!` / `eprintln!` in production code.** Use the logger module:
```rust
use crate::logger::{info as log_info, error as log_error};

log_info("Scanner started");
log_error(&format!("Failed to open process: {}", err));
```

Exception: `logger.rs` itself may use println/eprintln for stdout mirroring.

### Working with Legacy Code (`D2Stats.au3`)

The file is ~3000 lines. **Never load it fully** — it will overflow context.

1. First, check `docs/index_d2Stats.md` for section line ranges
2. Read only the needed section using `offset` and `limit` parameters
3. Use grep for specific searches instead of full file reads

### Documentation

- `docs/index_d2Stats.md` — Index of legacy AutoIt code sections
- `docs/filter_spec/` — Loot filter DSL specification
- `docs/autocomplete.md` — Editor autocomplete: data flow, cache lifecycle, extension points
