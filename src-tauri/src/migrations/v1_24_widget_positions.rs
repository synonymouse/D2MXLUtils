//! v1.23 → v1.24: top-level `notificationX/Y` (percent) moved into
//! `widget_positions["notifications"]`. Done as part of the unified
//! overlay-widget-repositioning module — see
//! docs/superpowers/specs/2026-05-09-overlay-widget-repositioning-design.md.

use serde::Deserialize;
use serde_json::Value;

use crate::settings::{AppSettings, WidgetPosition};

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")] // matches AppSettings's wire format
struct LegacyKeys {
    #[serde(default)]
    notification_x: Option<f64>,
    #[serde(default)]
    notification_y: Option<f64>,
}

pub fn apply(raw: &Value, s: &mut AppSettings) -> bool {
    if s.widget_positions.contains_key("notifications") {
        return false;
    }
    let legacy: LegacyKeys = serde_json::from_value(raw.clone()).unwrap_or_default();
    // Skip when neither legacy key was present (fresh install): the
    // helper's spec default kicks in and we avoid writing a noisy
    // pre-populated entry to settings.
    if legacy.notification_x.is_none() && legacy.notification_y.is_none() {
        return false;
    }
    let x = legacy.notification_x.unwrap_or(1.0);
    let y = legacy.notification_y.unwrap_or(1.0);
    s.widget_positions
        .insert("notifications".into(), WidgetPosition { x, y });
    true
}

#[cfg(test)]
#[path = "v1_24_widget_positions_tests.rs"]
mod tests;
