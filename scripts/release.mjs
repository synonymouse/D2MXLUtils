import { execSync } from "node:child_process";

const arg = process.argv[2] ?? "patch";
const validBumps = ["patch", "minor", "major"];

if (!validBumps.includes(arg)) {
  console.error(`Unknown bump type "${arg}" — expected one of: ${validBumps.join(", ")}`);
  process.exit(1);
}

// Delegates to `pnpm version`, which runs scripts/sync-version.mjs (syncs
// Cargo.toml/Cargo.lock/tauri.conf.json) and creates the commit + tag.
// Does NOT push — `git push --follow-tags` is a separate, deliberate step
// (pushing the tag is what triggers the real CI release build).
execSync(`pnpm version ${arg}`, { stdio: "inherit" });
