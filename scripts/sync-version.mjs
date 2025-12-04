import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { execSync } from "node:child_process";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const pkgPath = path.join(rootDir, "package.json");
const cargoPath = path.join(rootDir, "src-tauri", "Cargo.toml");
const tauriConfPath = path.join(rootDir, "src-tauri", "tauri.conf.json");

/** Load package.json and take its version as the single source of truth */
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
const version = pkg.version;

if (!/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`Unexpected version format in package.json: "${version}"`);
}

/** Sync version in Cargo.toml (Rust backend) */
{
  const content = readFileSync(cargoPath, "utf8");
  const next = content.replace(
    /^version\s*=\s*"\d+\.\d+\.\d+"$/m,
    `version = "${version}"`
  );
  if (content === next) {
    console.warn("No version line replaced in Cargo.toml – check the file format.");
  }
  writeFileSync(cargoPath, next, "utf8");
}

/** Sync version in tauri.conf.json (desktop bundle config) */
{
  const content = readFileSync(tauriConfPath, "utf8");
  const next = content.replace(
    /"version"\s*:\s*"\d+\.\d+\.\d+"/,
    `"version": "${version}"`
  );
  if (content === next) {
    console.warn("No version field replaced in tauri.conf.json – check the file format.");
  }
  writeFileSync(tauriConfPath, next, "utf8");
}

// Stage synced files so that `pnpm version` includes them into its commit
try {
  execSync(`git add "${cargoPath}" "${tauriConfPath}"`, { stdio: "inherit" });
  console.log("Staged Cargo.toml and tauri.conf.json for commit");
} catch (err) {
  console.warn(
    "Failed to stage synced files automatically. Please run: git add src-tauri/Cargo.toml src-tauri/tauri.conf.json"
  );
}

console.log(`Synced version to ${version} in Cargo.toml and tauri.conf.json`);

