// Builds a local database of unique/set item stat templates (with roll
// ranges, e.g. "+(6 to 10) to all Attributes") by crawling the same public
// MXL item API the in-app search overlay uses (see
// `src-tauri/src/mxl_item_api.rs`).
//
// Run locally via `pnpm unique-stats-db:publish` (scripts/publish-unique-
// stats-db.mjs), which calls this and then publishes the result to a
// GitHub release — clients download that automatically instead of every
// install crawling this same third-party API themselves (see
// unique_stats_db_sync.rs). NOT run from CI: the target API blocks
// GitHub Actions' runner IPs outright (confirmed live — 403 from Actions,
// 200 for the identical request from a normal dev IP), so this has to run
// from an actual maintainer machine.
//
// Usage: node scripts/generate-unique-stats-db.mjs [output-path]
// Default output path: ~/.local/share/com.d2mxlutils.app/unique-stats-db.json

import { writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import os from "node:os";

const API_URL = "https://tsw.vn.cz/stats/api_item.php";
// Categories that carry a fixed name + templated (possibly ranged) stats —
// the ones a "roll range" display is meaningful for. Magic/rare affixes are
// per-instance random and have no fixed name to look up by.
const RELEVANT_QUALITIES = new Set(["TU", "SU", "Set", "Sacred Set"]);
// Considerate pacing against a third-party community-run site — not an
// API-enforced limit, just being a good citizen.
const REQUEST_DELAY_MS = 1000;

function defaultOutputPath() {
  const appDataDir =
    process.platform === "win32"
      ? path.join(process.env.APPDATA ?? "", "com.d2mxlutils.app")
      : path.join(os.homedir(), ".local", "share", "com.d2mxlutils.app");
  return path.join(appDataDir, "unique-stats-db.json");
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fetchJson(url) {
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`${url} -> HTTP ${res.status}`);
  }
  return res.json();
}

async function main() {
  const outputPath = process.argv[2]
    ? path.resolve(process.argv[2])
    : defaultOutputPath();

  console.log("Fetching item index...");
  const index = await fetchJson(`${API_URL}?mode=index`);
  const names = index.items
    .filter((item) => RELEVANT_QUALITIES.has(item.quality))
    .map((item) => item.name);
  console.log(`${names.length} unique/set entries to crawl (of ${index.items.length} total).`);

  const entries = [];
  let done = 0;
  for (const name of names) {
    const url = `${API_URL}?q=${encodeURIComponent(name)}`;
    try {
      const result = await fetchJson(url);
      const match = (result.items ?? []).find((it) => it.name === name);
      if (match) {
        entries.push({
          name: match.name,
          quality: match.quality,
          stats: match.stats,
        });
      } else {
        console.warn(`  no exact match returned for "${name}"`);
      }
    } catch (err) {
      console.warn(`  failed "${name}": ${err.message}`);
    }
    done++;
    if (done % 25 === 0 || done === names.length) {
      console.log(`  ${done}/${names.length}`);
    }
    await sleep(REQUEST_DELAY_MS);
  }

  const db = {
    generatedAt: new Date().toISOString(),
    entries,
  };

  mkdirSync(path.dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, JSON.stringify(db, null, 2));
  console.log(`Wrote ${entries.length} entries to ${outputPath}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
