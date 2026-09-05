# Changelog

## v1.26.24 — 2026-09-05

### Features

- Feat: add toggle for "Items hidden" overlay indicator (dea8b8a)

### Other

- 1.26.24 (884d85d)

## v1.26.23 — 2026-09-05

### Bug Fixes

- Fix: relocate tooltip-item hook offset for MXL 2.14's D2Sigma.dll (4c63819)

### Other

- 1.26.23 (c6646d7)

## v1.26.22 — 2026-09-04

### Features

- Feat: add manual refresh button for game data caches (26775c3)

### Other

- 1.26.22 (1e8c6a3)

## v1.26.21 — 2026-09-04

### Features

- Feat(stats): cap displayed elemental/poison max resist at 90% (0d32d3e)

### Other

- 1.26.21 (a9e8017)

## v1.26.20 — 2026-09-04

### Bug Fixes

- Fix(ci): rename release AppImage with capital ".AppImage" extension (b62d5f2)
- Fix(updater): match the renamed AppImage asset case-insensitively (ccff37f)

### Other

- 1.26.20 (bc28665)

## v1.26.19 — 2026-09-04

### Bug Fixes

- Fix(stats): compute Charms locally instead of trusting the engine's counter (0337095)
- Fix(breakpoints): cache last-known data across tab switches (e703648)
- Fix(stats): show cached data instantly and stop per-tick flicker on read failures (240909d)
- Fix(stats): correct Life/Mana-per-point class factors, document Azurewrath (ba8cd3c)
- Fix(stats): correct Spell Focus formula and layout tweaks (b199eb7)

### Features

- Feat(branding): mark this build as a community fork (81b2ce8)
- Feat(stats): add full character Stats tab (2d188a5)

### Other

- 1.26.19 (2c5cdd8)

## v1.26.18 — 2026-08-31

### Features

- Feat(rules-editor): highlight and autocomplete class/level DSL keywords (a851751)

### Other

- 1.26.18 (5c17c60)

## v1.26.17 — 2026-08-31

### Bug Fixes

- Fix(rules-editor): persist group-rule fold state across tab switches and restarts (5ce20df)

### Documentation

- Docs: document release:skip convention for doc-only PRs (51b2827)
- Docs: consolidate AGENTS.md into CLAUDE.md (8fdd673)

### Other

- 1.26.17 (cd05ef8)

## v1.26.16 — 2026-08-31

### Features

- Feat(rules-editor): autocomplete bare DSL keywords (3a0e757)

### Other

- 1.26.16 (14af5a9)

## v1.26.15 — 2026-08-30

### Features

- Feat(loot-filter): add live "show matches" rule highlighting (aeb783f)

### Other

- 1.26.15 (5aef644)

## v1.26.14 — 2026-08-30

### Features

- Feat(rules-editor): add code folding for group rules (19c0ff8)

### Other

- 1.26.14 (d750232)

## v1.26.13 — 2026-08-30

### Bug Fixes

- Fix(unique-stats-db): pin gh CLI calls to the correct repo (aa460d6)

### Other

- 1.26.13 (14adda9)

## v1.26.12 — 2026-08-30

### Bug Fixes

- Fix(unique-stats-db): publish locally instead of via CI (blocked outright) (8cb794f)

### Other

- 1.26.12 (8fe07cf)

## v1.26.11 — 2026-08-30

### Features

- Feat(unique-stats-db): publish + auto-sync the roll-range template DB (580af1e)

### Other

- 1.26.11 (518cdf1)

## v1.26.10 — 2026-08-30

### Bug Fixes

- Fix(map-marker): clear persistent cache on real area/act change (ca36b63)

### Other

- 1.26.10 (f13eaa5)

## v1.26.9 — 2026-08-30

### Bug Fixes

- Fix(windows): close CreateRemoteThread's handle — was leaked on every injected call (9d012a9)

### Other

- 1.26.9 (e76557b)

## v1.26.8 — 2026-08-29

### Bug Fixes

- Fix(ci): discard stray Cargo.toml modification before rebasing changelog (e4cc2db)

### Other

- 1.26.8 (0078112)

## v1.26.7 — 2026-08-29

### Bug Fixes

- Fix(ci): rebase before pushing the changelog commit, don't just push and pray (ef72810)

### Other

- 1.26.7 (7e1cf88)

## v1.26.6 — 2026-08-29

### Bug Fixes

- Fix(ci): explicitly dispatch the release build instead of relying on the tag-push cascade (1b20767)

### Other

- 1.26.6 (ae3d6fd)

## v1.26.5 — 2026-08-29

### Features

- Feat(ci): auto-release on merge to master; build Windows/Linux in parallel (2dd3923)

### Other

- 1.26.5 (156a061)

## v1.26.4 — 2026-08-29

### Bug Fixes

- Fix(map-marker): attach markers as pObjects leaves, not root swaps (6b82a5f)

### Other

- 1.26.4 (ce3f69f)

## v1.26.3 — 2026-08-29

### Bug Fixes

- Fix(updater): add real Linux AppImage self-update support (c1fdf23)

### Other

- 1.26.3 (56ef1fc)

## v1.26.2 — 2026-08-29

### Other

- 1.26.2 (96c607d)

## v1.26.1 — 2026-08-29

### Bug Fixes

- Fix(updater): point self-updater and releases link at this fork (13c73cc)
- Fix(ui): dark-mode select popups and tab-content overflow containment (c28a9be)
- Fix(linux): unconditionally disable WebKitGTK's DMA-BUF renderer (6261605)
- Fix(overlay): fix focus-stealing prevention after alt-tab on Linux (c769a48)
- Fix(overlay): explicitly refocus D2 instead of trusting WM implicit behavior (73bdb57)

### Features

- Feat(release): add pnpm release shortcut, defaulting to patch (9161fb7)
- Feat(dev): add F12 devtools shortcut (557f00f)
- Feat(loot-filter): add quest filter keyword (c6ed195)
- Feat: unique/set roll-range annotations and create-game autofill (87c3c43)
- Feat(loot-filter): add clvl/ilvl and character-class filter keywords (b8358b1)

### Other

- 1.26.1 (0a4d575)

### Refactor

- Refactor(ui): custom Select component replacing native <select> (661cc32)

## manual-19 — 2026-08-24

### Bug Fixes

- Fix(overlay): dedupe notification toasts by unit_id (140b7eb)
- Fix(sounds): route Linux audio through rodio directly, never <audio> (30c9cbd)

## manual-14 — 2026-08-24

### Bug Fixes

- Fix(ci): stand in an empty gdk-pixbuf loaders dir for linuxdeploy's gtk plugin (6158548)
- Fix(ci): install gdk-pixbuf2 for linuxdeploy's gtk plugin (331ea55)
- Fix(ci): install fuse2 for linuxdeploy's AppImage runtime (b352dac)

### CI

- Ci(debug): use --verbose on the real build instead of standalone probes (258e880)
- Ci(debug): test env-var-only invocation and the appimage output plugin (22eb1ae)
- Ci(debug): run linuxdeploy directly to surface its real error (1ac0763)
- Ci: fix linuxdeploy under container (FUSE) + let manual runs pick a platform (3b904fc)

## manual-7 — 2026-08-24

### Bug Fixes

- Fix(ci): build the Linux AppImage inside an Arch container (d276344)

## manual-6 — 2026-08-24

### Bug Fixes

- Fix(ci): install libasound2-dev for rodio's alsa-sys build (ea623af)

## manual-5 — 2026-08-24

### Bug Fixes

- Fix(ci): unescaped # in release name expression broke YAML parsing (49ea451)

### CI

- Ci: fix release step for workflow_dispatch (needs a real tag) (7071a7f)
- Ci: allow manual test runs via workflow_dispatch (629d125)
- Ci: add Linux AppImage build/release job (1a4566c)

### Features

- Feat(linux): native Linux port (process attach, injection, overlay, hotkeys) (aeff831)

## v1.26.0 — 2026-05-17

### Bug Fixes

- Fix(notifier): use lazy static item enrichment (0772ab4)

### Features

- Feat(item-search): add indexed typeahead search (59e4cbb)
- Feat(overlay): add draggable overlay windows (61c8e53)
- Feat(overlay): polish loot history panel UX (e37e438)
- Feat(item-search): add in-game MXL item lookup (ae50e7c)

### Miscellaneous

- Chore: ignore worktree directories (4f468cc)

### Other

- 1.26.0 (bd34525)

## v1.25.0 — 2026-05-13

### Bug Fixes

- Fix(hotkeys): support auxiliary mouse buttons (ceaab6a)
- Fix(ci): keep release automation out of notes (adae543)
- Fix(notifications): support larger preview sizes (924000a)
- Fix(editor): restore line numbers and socket reference (2588f48)
- Fix(loot-filter): stabilize automap marker reconciliation (20c6c96)
- Fix(scanner): cap item memory walks (d767201)

### Features

- Feat(settings): add auto /nopickup toggle (ec02674)
- Feat(notifications): boost text contrast at low opacity (153b229)
- Feat(notifications): show matched stat lines (dce1845)

### Other

- 1.25.0 (ea3aa46)

## v1.24.0 — 2026-05-11

### Bug Fixes

- Fix(loot-filter): process BFS-only item candidates (f634595)
- Fix(dps-meter): show meter only in active games (83151b8)
- Fix(loot-filter): harden hook mask cleanup lifecycle (ff4e2b5)
- Fix(dps-meter): restore hook lifecycle across restarts (d7f9b36)
- Fix(overlay): hide chrome when game is minimized (aedd7ff)
- Fix(overlay): restore single-window reposition mode (0a3db83)

### Documentation

- Docs: update formatting and loot notes (1be0b23)
- Docs: drop landed plans and specs for past features (59a457d)

### Features

- Feat(overlay): unified widget repositioning module (6929406)
- Feat(dps-meter): add live DPS overlay via inline hook on Ord10887 (a613a50)

### Miscellaneous

- Chore(format): enforce project formatters (549bcde)

### Other

- 1.24.0 (3eeac3b)

### Performance

- Perf(loot-filter): reduce large-filter matching latency (3394422)

### Refactor

- Refactor(loot-filter): remove unused filter toggle (40c3e4a)

## v1.23.0 — 2026-05-06

### Bug Fixes

- Fix(loot-filter): skip runtime-name match for rare items (34c2ae3)

### Features

- Feat(sounds): add goblin alert with selectable sound (a231acf)

### Other

- 1.23.0 (e839707)

## v1.22.0 — 2026-05-05

### Bug Fixes

- Fix(loot-filter): scan full D2Sigma.dll instead of hardcoded 2MB (79bbe1f)
- Fix(scanner): AOB-resolve always-show-items struct after MXL patch (f9f5721)

### Features

- Feat(logger): throttle errors per call site and rotate log file (c69d8c9)

### Other

- 1.22.0 (357ecda)
- Revert "fix(scanner): disable auto always-show-items after MXL patch broke offset" (ac3fc4c)

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
- Fix(ui): sync header version with package.json at build time (79741a8)
- Eliminate loot-filter label flicker on fresh drops (bff0c0d)
- Fix reattach bug (37374c0)
- Survive project restart without re-launching Diablo II (4f3ed6d)
- Fix loot filter not syncing to scanner after profile load/save (d8dacab)

### CI

- Ci(release): grant pull-requests:read so git-cliff can query GitHub API (d967eae)
- Ci(release): generate release notes from commits with git-cliff (6ea2f12)

### Changes

- Move drop notifications to top-left and hide overlay window border (c0be59c)
- Redesign drop notification layout and drop the `name` filter flag (b67fd52)
- Normalize autocomplete dictionary and version the items cache (12ae0fb)
- Removed docs (313fcfb)

### Features

- Feat(updater): add GitHub Releases auto-updater (b872d31)
- Feat(filter): highlight matched stat line in drop notifications (5b198b3)
- Feat(sound): play drop notification sounds with master volume (b821569)
- Feat(notifications): improve drop rendering and settings preview (de19ced)
- Add hold-hotkey overlay editor to reposition drop notifications (7a645a8)
- Add uniques and set items to autocomplete dictionary (863a50f)
- Add items autocomplete to loot-filter rules editor (00c7341)
- Support multi-quality/tier OR-matching and base_name regex (915f96a)
- Implement MedianXL tier detection (C2 fix) (bea784d)
- Add force-show filter mode and reduce scanner overhead (a476716)
- Implement full loot filter trampoline and clean up scanner loop (575216a)
- Add loot filter hook, rule matching enhancements, and documentation (b33bd93)
- Add ThemeToggle component and refactor GeneralTab layout (16b0b0c)

### Other

- 1.7.0 (bf38bce)
- 1.6.0 (f5dadfc)
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

