## D2MXLUtils

**D2MXLUtils** is an overlay utility for *Diablo II: Median XL* that provides drop notifications and additional helper tools.

Technologies:
- **Frontend**: `Svelte` + `Vite`
- **Desktop shell**: `Tauri`
- **Backend**: `Rust`

### Development setup

Requirements:
- `Windows 10\11`
- `Node.js` (LTS recommended)
- `pnpm` (see `packageManager` in `package.json`)
- `Rust` toolchain and required native build tools for Tauri (see official Tauri documentation)

Install dependencies:

```bash
pnpm install
```

Run in development:

```bash
pnpm dev         # start Vite dev server (frontend only)
pnpm tauri dev   # start the Tauri desktop app in dev mode
```

Build:

```bash
pnpm build       # build frontend
pnpm tauri build # build Tauri desktop app
```

Tauri packaging/bundling is configured under `src-tauri`; refer to Tauri docs and project scripts when adding release builds.

### Release

To create a new release:

```bash
# Bump version (choose one):
pnpm version patch   # 0.1.0 → 0.1.1 (bugfixes)
pnpm version minor   # 0.1.0 → 0.2.0 (new features)
pnpm version major   # 0.1.0 → 1.0.0 (breaking changes)

# Push with tag:
git push --follow-tags
```

This will:
1. Update version in `package.json`, `Cargo.toml`, `Cargo.lock`, and `tauri.conf.json`
2. Create a commit and git tag (e.g. `v0.2.0`)
3. Trigger GitHub Actions pipeline that builds the app and creates a GitHub Release with downloadable binaries

### Project structure (short)

- `src/` — Svelte application and styles
- `src-tauri/` — Rust code and Tauri configuration
- `docs/` — notes, plans, and additional project documents
