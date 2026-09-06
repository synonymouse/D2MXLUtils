import { execFileSync, execSync } from "node:child_process";
import { readFileSync } from "node:fs";

const arg = process.argv[2] ?? "patch";
const validBumps = ["patch", "minor", "major"];

if (!validBumps.includes(arg)) {
  console.error(`Unknown bump type "${arg}" — expected one of: ${validBumps.join(", ")}`);
  process.exit(1);
}

execFileSync("git", ["fetch", "origin", "master"], { stdio: "inherit" });

const head = execFileSync("git", ["rev-parse", "HEAD"], {
  encoding: "utf8",
}).trim();
const originMaster = execFileSync("git", ["rev-parse", "origin/master"], {
  encoding: "utf8",
}).trim();

if (head !== originMaster) {
  throw new Error(
    "Release must start at the current origin/master. Run `git fetch origin master` and check out that commit first.",
  );
}

// Delegates to `pnpm version`, which runs scripts/sync-version.mjs (syncs
// Cargo.toml/Cargo.lock/tauri.conf.json) and creates the commit + tag.
// It does not push; publishing remains a separate deliberate step.
execSync(`pnpm version ${arg}`, { stdio: "inherit" });

const packageJson = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);
console.log("\nRelease commit and tag created. Push only these refs:");
console.log(
  `  git push --atomic origin HEAD:master refs/tags/v${packageJson.version}:refs/tags/v${packageJson.version}`,
);
