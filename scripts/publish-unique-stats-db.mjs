// Regenerates the unique/set roll-range template DB and publishes it to
// the `unique-stats-db` GitHub release (which the app's
// unique_stats_db_sync.rs downloads from). Run this locally — NOT from
// CI. Confirmed live via an actual workflow run: the third-party API
// generate-unique-stats-db.mjs crawls returns HTTP 403 for every request
// from a GitHub Actions runner, but 200 for the exact same request from a
// normal residential/dev IP (tested side by side, no header differences
// mattered) — the site is blocking Actions' published IP ranges, almost
// certainly deliberately given how much automated scraping comes from
// there. There's no fix on our end for that short of a self-hosted
// runner, which isn't worth the setup for something run this rarely.
//
// Requires the `gh` CLI, already authenticated.
//
// Usage: node scripts/publish-unique-stats-db.mjs

import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outputPath = path.join(__dirname, "..", "unique-stats-db.json");
const TAG = "unique-stats-db";
// Explicit, not inferred from the current directory's git remotes: this
// repo typically has both `origin` (the read-only upstream,
// synonymouse/D2MXLUtils) and a personal fork remote, and `gh` picking the
// wrong one fails with a 404 rather than anything obviously
// repo-related — confirmed live the first time this ran.
const REPO = "pertinate/D2MXLUtils";

console.log("Generating unique-stats-db.json ...");
execFileSync(
  "node",
  [path.join(__dirname, "generate-unique-stats-db.mjs"), outputPath],
  { stdio: "inherit" },
);

function releaseExists() {
  try {
    execFileSync("gh", ["release", "view", TAG, "--repo", REPO], {
      stdio: "ignore",
    });
    return true;
  } catch {
    return false;
  }
}

if (releaseExists()) {
  console.log(`Uploading to existing "${TAG}" release ...`);
  execFileSync(
    "gh",
    ["release", "upload", TAG, outputPath, "--repo", REPO, "--clobber"],
    { stdio: "inherit" },
  );
} else {
  console.log(`Creating "${TAG}" release ...`);
  execFileSync(
    "gh",
    [
      "release",
      "create",
      TAG,
      outputPath,
      "--repo",
      REPO,
      "--title",
      "Unique/Set Stats DB",
      "--notes",
      "Auto-generated unique/set item stat template database (roll ranges). Downloaded automatically by the app — not a versioned app release.",
    ],
    { stdio: "inherit" },
  );
}

console.log("Done.");
