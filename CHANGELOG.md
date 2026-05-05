# Changelog

## v1.21.0 — 2026-05-05

### Bug Fixes

- Fix(scanner): disable auto always-show-items after MXL patch broke offset (bc3f8df)
- Fix(loot-filter): widen hook masks to 16 bits and clear bits on item disappear (c9dc531)
- Fix(ci): checkout master branch before pushing changelog (303b637)

### Documentation

- Docs(breakpoints): add design spec and implementation plan (0c4e282)

### Features

- Feat(sounds): add dedicated Sounds tab with per-slot volume and custom files (99df603)
- Feat(breakpoints): add Breakpoints tab with live attack/cast/recovery FPA (3d0cba7)

### Other

- 1.21.0 (8930736)
- Gitignore (3aaac86)

## v1.20.1 — 2026-05-02

### Bug Fixes

- Fix(ci): generate changelog before build so binary embeds current version (8f8ea3a)

### Documentation

- Docs: update CHANGELOG.md (2c0fe6f)

### Other

- 1.20.1 (9937f78)

## v1.20.0 — 2026-05-02

### Bug Fixes

- Fix(hotkeys): suppress polling hotkeys when game is not foreground (6346dd6)
- Fix(scanner): stat fallback for data-table-only items (Cycles) (1f26e8a)

### Features

- Feat(ui): show changelog in-app instead of opening GitHub (82a4282)
- Feat(changelog): auto-generate CHANGELOG.md on release (6ffa849)

### Other

- 1.20.0 (5716b67)

### Performance

- Perf(scanner): map-marker pass on dedicated thread + TTL eviction (ed07bb1)

## v1.19.1 — 2026-05-01

### Bug Fixes

- Fix(loot-history): readable item names + theme-agnostic panel chrome (9d247a2)

### Other

- 1.19.1 (2cbe46e)

## v1.19.0 — 2026-05-01

### Features

- Feat(loot-history): session pickup tracker with overlay panel (0748c64)

### Other

- 1.19.0 (476d1b0)

## v1.18.0 — 2026-04-28

### Bug Fixes

- Fix(updater): surface install errors with manual-download fallback (acb6ba0)
- Fix(hotkeys): allow bare keys and prevent duplicate bindings (e6e3014)
- Fix(overlay): strip leaked window chrome on systems where decorations: false leaks (b70d0d0)
- Fix(overlay): stop focus war and edge flicker on alt-tab (b97d7ba)
- Fix(notifications): reverse stat order to match in-game tooltip (89d56a3)

### Features

- Feat(rules): add socket-count filter (sockets0..sockets6) (34188d1)
- Feat(editor): accept autocomplete with Tab (2afd3f1)

### Miscellaneous

- Chore(profiles): refine default new-profile template rules (62849d4)

### Other

- 1.18.0 (dc8d3d7)

## v1.17.0 — 2026-04-27

### Features

- Feat(loot-filter): category colors, shadow warnings, hover tooltips (3ad87b7)
- Feat(loot-filter): linter errors on misplaced name pattern (2ec2aee)

### Other

- 1.17.0 (731307f)

## v1.16.0 — 2026-04-27

### Bug Fixes

- Fix(notifications): show unique name for Tier0-base uniques (b437dbe)

### Features

- Feat(loot-filter): hold-to-reveal hotkey for hidden items (13f6941)

### Other

- 1.16.0 (678a9cc)

## v1.15.1 — 2026-04-26

### Bug Fixes

- Fix(profiles): orange map quest items and Cube Reagent rule (0057bf2)
- Fix(loot-filter): match name patterns against items.txt category prefix (949f450)

### Other

- 1.15.1 (cb813de)

## v1.15.0 — 2026-04-26

### Bug Fixes

- Fix: default profile (e0e181b)

### Documentation

- Docs(loot-filter): add user-facing gotchas guide (04bce3a)

### Features

- Feat(loot-filter): add sound7 flag (a94c7bd)
- Feat(scanner): auto-enable MXL always-show-items on game entry (92de094)

### Other

- 1.15.0 (10b0420)

## v1.14.1 — 2026-04-25

### Features

- Feat(loot-filter): rework starter template hide rules (429db28)

### Other

- 1.14.1 (231192f)

## v1.14.0 — 2026-04-24

### Features

- Add files via upload (c93650a)
- Add files via upload (9eb0269)
- Add files via upload (b2e51cc)
- Feat(loot-filter): notify on all eth sacred in starter template (88d0437)
- Add files via upload (8466c91)
- Add files via upload (80afb3c)
- Add files via upload (d386501)

### Miscellaneous

- Chore(public): downscale screenshots to ~620x460 (5d3f59a)

### Other

- 1.14.0 (488e768)

## v1.13.0 — 2026-04-24

### Bug Fixes

- Fix(editor): bump comment color contrast in both themes (ed04ad2)

### Other

- 1.13.0 (74e8225)

## v1.12.0 — 2026-04-24

### Bug Fixes

- Fix(loot-filter): respect group header flags in notify-independence lint (310fcd0)

### Features

- Feat(loot-filter): seed Default-starter profile with MXL-based rules (5e85df5)

### Other

- 1.12.0 (fd5b875)

## v1.11.0 — 2026-04-23

### Bug Fixes

- Fix(notifier): re-notify item dropped after pickup (b3235ec)
- Fix(settings): preserve cross-window changes when saving (ef33de8)

### Features

- Feat(loot-filter): auto-save profiles on idle typing (b01f9fe)
- Feat(loot-filter): seed a real Default profile on first run (f1e6151)

### Miscellaneous

- Chore: remove legacy AutoIt source and obsolete docs (9670c44)
- Chore: untrack .vscode .claude (81a9b9b)

### Other

- 1.11.0 (f7b7d0a)

### Performance

- Perf(notifier): split pPaths scan from map-marker BFS pass (9f1d776)

## v1.10.0 — 2026-04-23

### Bug Fixes

- Fix(loot-filter): resolve outstanding review bugs (8139e96)

### Features

- Feat(loot-filter): support multiple {regex} stat patterns per rule (AND) (188b1cc)
- Feat(ui): desktop-feel polish and debug logging toggle (65210d1)
- Feat(loot-filter): add map flag for in-game automap markers (ebf6ea0)

### Other

- 1.10.0 (7f9ad43)

## v1.9.0 — 2026-04-21

### Features

- Feat(notifications): honor rule-level color flag for item names (5573ff0)

### Other

- 1.9.0 (46989df)

## v1.8.1 — 2026-04-21

### Bug Fixes

- Fix(updater): send Accept: octet-stream to fetch the binary asset (e4d9cfc)

### Other

- 1.8.1 (66a8e2e)

## v1.8.0 — 2026-04-21

### Features

- Feat(app): store log in app data dir; add "Open folder" button (abab594)

### Other

- 1.8.0 (879e51e)

### Refactor

- Refactor(ui): tidy section headers and toolbar alignment (5a94ab1)

## v1.7.0 — 2026-04-21

### Bug Fixes

- Fix(filter): auto-load active profile on startup (0e5b82e)
- Fix(notifier): label low-wLvl TU uniques (e.g. Razordisk) correctly (d9b6904)

### Features

- Feat(updater): add GitHub Releases auto-updater (b872d31)
- Feat(filter): highlight matched stat line in drop notifications (5b198b3)
- Feat(sound): play drop notification sounds with master volume (b821569)
- Feat(notifications): improve drop rendering and settings preview (de19ced)

### Other

- 1.7.0 (bf38bce)

## v1.6.0 — 2026-04-21

### Bug Fixes

- Fix(ui): sync header version with package.json at build time (79741a8)
- Eliminate loot-filter label flicker on fresh drops (bff0c0d)
- Fix reattach bug (37374c0)
- Survive project restart without re-launching Diablo II (4f3ed6d)

### CI

- Ci(release): grant pull-requests:read so git-cliff can query GitHub API (d967eae)
- Ci(release): generate release notes from commits with git-cliff (6ea2f12)

### Changes

- Move drop notifications to top-left and hide overlay window border (c0be59c)
- Redesign drop notification layout and drop the `name` filter flag (b67fd52)
- Normalize autocomplete dictionary and version the items cache (12ae0fb)

### Features

- Add hold-hotkey overlay editor to reposition drop notifications (7a645a8)
- Add uniques and set items to autocomplete dictionary (863a50f)
- Add items autocomplete to loot-filter rules editor (00c7341)

### Other

- 1.6.0 (f5dadfc)

## v1.5.0 — 2026-04-19

### Bug Fixes

- Fix loot filter not syncing to scanner after profile load/save (d8dacab)

### Changes

- Removed docs (313fcfb)

### Features

- Support multi-quality/tier OR-matching and base_name regex (915f96a)
- Implement MedianXL tier detection (C2 fix) (bea784d)
- Add force-show filter mode and reduce scanner overhead (a476716)
- Implement full loot filter trampoline and clean up scanner loop (575216a)
- Add loot filter hook, rule matching enhancements, and documentation (b33bd93)
- Add ThemeToggle component and refactor GeneralTab layout (16b0b0c)

### Other

- Sync pnpm-lock.yaml with package.json (7d43ef3)
- 1.5.0 (33a5355)
- New spec (6c9b497)
- - Integrated ProfileSelector component for managing user profiles within the LootFilterTab. (5742608)
- - Updated the `NotificationsTab` and `OverlayWindow` components to support customizable notification settings, including duration, font size, and opacity. (a2e2c89)
- Enhance layout and styling for improved UI responsiveness (8d64d6a)
- Enhance CodeMirror integration and validation features (833f1e1)

### Refactor

- Refactor loot filter engine to match new DSL specification (412a8e3)
- Refactor UI styles and improve editor functionality (5dc04e9)

## v1.2.1 — 2025-12-05

### Features

- Add CodeMirror dependencies and implement Loot Filter Editor (01fb59c)
- Add known bugs documentation and improve memory management in injection process (92f459e)
- Implement global hotkey functionality for toggling the main window in D2MXLUtils. Add HotkeyInput component for user configuration, integrate hotkey management in Tauri backend, and update settings store to persist hotkey preferences. Enhance UI to reflect hotkey settings in the General tab. (80bf329)
- Implement settings management with persistence using Tauri plugin store. Add settings store and window state management, allowing users to save and load application settings. Update UI components to reflect settings changes, including theme and sound preferences. Adjust styles for overlay backgrounds. (d384fe9)

### Other

- 1.2.1 (6ca888a)

### Refactor

- Refactor rule management and integrate DSL parser for item filtering (f3a3bfd)

## v1.2.0 — 2025-12-04

### Changes

- Update GitHub Actions workflow to build 32-bit Tauri app for Windows and adjust release asset path. (1cb5afd)

### Other

- 1.2.0 (dfa783b)

## v1.1.0 — 2025-12-04

### Other

- 1.1.0 (f73583e)
- Enhance release process in README and update GitHub Actions workflow. Added instructions for version bumping and release creation in README. Updated release job to use softprops/action-gh-release for asset uploads and set permissions for GitHub Actions. (965797d)

## v1.0.4 — 2025-12-04

### Changes

- Remove pnpm version specification from GitHub Actions workflow (422d92e)

### Other

- 1.0.4 (494f198)

## v1.0.3 — 2025-12-04

### Changes

- Update GitHub Actions workflow to set up pnpm version 10 and remove corepack enable step. (6e571b7)

### Other

- 1.0.3 (3fc9d01)

## v1.0.2 — 2025-12-04

### Other

- 1.0.2 (048bb04)
- Sync version in Cargo.lock for d2mxlutils to 1.0.1 and update staging in sync-version script. (d4ffbd4)

## v1.0.1 — 2025-12-04

### Changes

- Update GitHub Actions workflow to enable corepack for pnpm instead of using the pnpm setup action. (47065a9)
- Update version to 1.0.0 (58a45d9)

### Other

- 1.0.1 (7ae2f71)

## v1.0.0 — 2025-12-04

### Changes

- Update dependencies and enhance logging in D2MXLUtils. Add chrono for timestamping log entries, update Cargo.toml and Cargo.lock to include new dependencies, and refactor logger to prepend timestamps to log messages for better traceability. (bc32ab7)
- Remove unused print_string function and related injection logic from D2Injector. Update documentation to reflect the changes in available methods for item handling. (9e53e1b)
- Update overlay documentation for fullscreen behavior in D2MXLUtils. Clarify functionality on native Windows and virtualized environments, detailing limitations and recommended user settings. Enhance user guidance for optimal overlay performance in various game modes. (31a8fb7)
- Update package.json to use ES modules, enhance Cargo.toml with Windows dependencies, and implement process handling in Rust. Add icon file and improve memory management for process interactions. (9838623)

### Features

- Add husky dependency to pnpm-lock.yaml (c5c2fa2)
- Add version synchronization script and GitHub release workflow for D2MXLUtils. (5a621ba)
- Add README and restructure UI components for D2MXLUtils. (7205f5f)
- Implement access privilege fixes for D2MXLUtils, including a custom Windows manifest for administrator rights, enabling SeDebugPrivilege for the current process, and configuring WebView2 user data folder for UAC-elevated scenarios. Update related documentation and enhance process handling in main.rs. (f3337e8)
- Implement overlay window for D2MXLUtils with transparent, click-through functionality. Enhance synchronization with Diablo II's window position and size. Introduce a logging module for better debugging and document access issues related to process elevation. Update Svelte UI to support overlay-specific layouts and improve user experience. (a065cd9)

### Other

- 1.0.0 (3f0726d)
- 0.2.0 (e54bad3)
- Enhance scanner functionality in D2MXLUtils by adding overlay visibility management. Implement logic to show and hide the overlay window based on game status and scanner state, along with error handling for overlay operations. Update documentation to reflect these changes. (bbe138d)
- Enhance D2MXLUtils with improved item scanning and UI updates. Refactor item handling in the DropScanner, implement logging for debugging, and optimize memory management in the injection layer. Update Svelte UI for better user experience and integrate event handling for game status and item drops. (20ab328)
- Initialize D2MXLUtils project with Tauri, Rust, Svelte, and Tailwind. Add core files including package.json, configuration files, and initial source structure. Implement basic functionality for the Drop Notifier overlay, including event handling and UI components. (0d47793)

### Refactor

- Refactor D2MXLUtils UI by removing Tailwind CSS and implementing a custom CSS architecture. Introduce a dark theme and restructure components for better maintainability. Update package.json to reflect the removal of Tailwind dependencies and enhance the overall styling with new CSS variables and components. (4e16f82)
- Refactor logging in Rust backend to use a unified logging layer instead of direct `println!`/`eprintln!` calls. Update documentation to reflect logging practices and ensure messages are mirrored to stdout/stderr for debugging. Enhance `CLAUDE.md` with logging guidelines and update related files for consistency. (8d0dbe7)
- Refactor D2MXLUtils project by completing several Rust modules, enhancing the scanner functionality, and removing the outdated index documentation. Update Cargo.toml to include additional Windows features and improve thread management for the item scanner. (4f3eb75)

