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
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outputPath = path.join(__dirname, "..", "unique-stats-db.json");
const TAG = "unique-stats-db";
// Keep publication independent from the checkout's configured git remotes.
const REPO = "synonymouse/D2MXLUtils";

console.log("Generating unique-stats-db.json ...");
execFileSync(
  "node",
  [path.join(__dirname, "generate-unique-stats-db.mjs"), outputPath],
  { stdio: "inherit" },
);

const database = JSON.parse(readFileSync(outputPath, "utf8"));
const validGeneratedAt =
  typeof database.generatedAt === "string" &&
  Number.isFinite(Date.parse(database.generatedAt));
const validEntries =
  Array.isArray(database.entries) &&
  database.entries.length >= 800 &&
  database.entries.every(
    (entry) =>
      typeof entry?.name === "string" &&
      entry.name.trim() !== "" &&
      typeof entry?.stats === "string" &&
      entry.stats.trim() !== "",
  );

if (!validGeneratedAt || !validEntries) {
  throw new Error(
    "Refusing to publish an invalid or partial Unique Stats DB (expected a timestamp and at least 800 complete entries)",
  );
}
console.log(`Validated ${database.entries.length} entries before publication.`);

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
      "--target",
      "master",
      "--latest=false",
      "--title",
      "Unique/Set Stats DB",
      "--notes",
      "Auto-generated unique/set item stat template database (roll ranges). Downloaded automatically by the app — not a versioned app release.",
    ],
    { stdio: "inherit" },
  );
}

console.log("Done.");
