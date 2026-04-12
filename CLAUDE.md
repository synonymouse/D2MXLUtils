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
- **`injection.rs`** — Remote thread injection into D2 process to call internal game functions
- **`notifier.rs`** — `DropScanner` that scans item unit lists and emits `item-drop` events
- **`rules/`** — Loot filter rule engine: DSL parsing (`dsl.rs`), rule matching (`matching.rs`)
- **`d2types.rs`** — `#[repr(C)]` structs for D2 memory structures (`UnitAny`, `ItemData`, etc.)
- **`offsets.rs`** — D2 memory offsets (DLL bases, unit lists, item data pointers)
- **`logger.rs`** — File logger writing to `d2mxlutils.log` next to the exe
- **`settings.rs`** — App settings persistence
- **`profiles.rs`** — Loot filter profile management
- **`hotkeys.rs`** — Global hotkey handling

### Svelte Frontend (`src/`)

- **`App.svelte`** — Entry point, routes to `MainWindow` or `OverlayWindow` based on Tauri window label
- **`views/`** — Main window tabs (`GeneralTab`, `LootFilterTab`, `NotificationsTab`) and overlay
- **`components/`** — Reusable UI components (Button, Toggle, Tabs, etc.)
- **`editor/`** — CodeMirror-based loot filter rules editor
- **`stores/`** — Svelte stores for settings and window state

### Communication

- **Tauri Commands**: Frontend calls Rust via `invoke()` (e.g., `start_scanner`, `stop_scanner`)
- **Events**: Backend emits events to frontend via `app_handle.emit()` (e.g., `item-drop`, `scanner-status`)

## Important Conventions

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
- `docs/d2mxlutils-refactoring.plan.md` — Refactoring plan and progress log
- `docs/loot-filter-spec.md`, `docs/loot-filter-dsl.md` — Loot filter DSL specification

## Version Bumping

```bash
pnpm version patch    # 0.1.0 → 0.1.1
pnpm version minor    # 0.1.0 → 0.2.0
pnpm version major    # 0.1.0 → 1.0.0
git push --follow-tags  # Triggers GitHub Actions release
```

This syncs version across `package.json`, `Cargo.toml`, `Cargo.lock`, and `tauri.conf.json`.
