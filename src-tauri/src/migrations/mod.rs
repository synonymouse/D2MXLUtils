//! Settings migrations applied at load time.
//!
//! Adding a migration:
//!   1. Create `migrations/v<version>_<topic>.rs` with a single
//!      `pub fn apply(raw: &Value, s: &mut AppSettings) -> bool`.
//!   2. Add a `mod` declaration and a call below.
//!
//! Each migration must be idempotent (gate on the new field's
//! presence). After `migrate()` returns true, `settings/mod.rs` re-saves
//! to disk, so legacy keys disappear on the next load.

mod v1_24_loot_history_alt_n;
mod v1_24_widget_positions;

use serde_json::Value;

use crate::settings::AppSettings;

pub fn migrate(raw: &Value, s: &mut AppSettings) -> bool {
    let mut changed = false;
    changed |= v1_24_widget_positions::apply(raw, s);
    changed |= v1_24_loot_history_alt_n::apply(raw, s);
    changed
}
