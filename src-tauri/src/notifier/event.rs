//! Notification payload preparation and on-demand stat enrichment.

use super::{strip_color_codes, DropScanner, ItemDropEvent};
use crate::d2types::ScannedItem;
use crate::injection::D2Injector;
use crate::logger::error as log_error;
use crate::offsets::{d2common, item_data, item_quality, items_txt, stat_list};
use crate::unit_stats_reader::{StatReadResult, UnitStatsReader};

impl DropScanner {
    pub(super) fn enrich_event_stats(&mut self, event: &mut ItemDropEvent, p_unit: u32) {
        if event.runtime_stats_loaded {
            return;
        }

        self.debug_get_item_stats_calls += 1;

        let activation_frequency =
            match UnitStatsReader::new(&self.state.ctx.process, self.state.ctx.d2_common, p_unit)
                .read_stat(u32::from(stat_list::STAT_ACTIVATION_FREQUENCY), 0)
            {
                Ok(StatReadResult::Found(value)) if value != 0 => Some(value),
                _ => None,
            };

        let injector = self.state.injector.lock().unwrap();
        match injector.get_item_stats(&self.state.ctx.process, p_unit) {
            Ok(raw_stats) => {
                let cleaned = strip_color_codes(&raw_stats);
                if !cleaned.trim().is_empty() {
                    let reversed: Vec<&str> = cleaned.lines().rev().collect();
                    let mut stats = Self::format_event_stats(event.sockets, reversed.join("\n"));
                    if event.quality == "Unique" || event.quality == "Set" {
                        stats = crate::unique_stats_db::annotate_with_roll_ranges(
                            &self.state.unique_stats_db,
                            &event.name,
                            event.tier,
                            &stats,
                        );
                    }
                    event.stats = stats;
                    event.runtime_stats_loaded = true;
                }
            }
            Err(e) => {
                if self.verbose_filter_logging {
                    log_error(&format!("get_item_stats failed for unit {}: {}", p_unit, e));
                }
            }
        }

        if !event.runtime_stats_loaded {
            if let Some(text) = self.read_item_desc_from_txt(&injector, event.class) {
                event.stats = Self::format_event_stats(event.sockets, text);
                event.runtime_stats_loaded = true;
            }
        }

        if let Some(value) = activation_frequency {
            let already_formatted = event
                .stats
                .lines()
                .any(|line| line.starts_with("Activation Frequency "));
            if !already_formatted {
                if !event.stats.is_empty() {
                    event.stats.push('\n');
                }
                event
                    .stats
                    .push_str(&format!("Activation Frequency {value:+}%"));
            }
            event.runtime_stats_loaded = true;
        }
    }

    /// Convert a scanned item into an event payload for the frontend.
    pub(super) fn to_event(&self, scanned: ScannedItem) -> ItemDropEvent {
        let class = scanned.class;
        let quality = scanned.quality_name().to_string();
        let base_name = self.class_base_name(class);
        let mut name = self.static_display_name(&scanned, &base_name);
        let runtime_stats_loaded = scanned.stats.is_some();
        let unique_kind = if scanned.quality == item_quality::UNIQUE {
            self.unique_kind(scanned.file_index, class)
        } else {
            None
        };
        if let Some(kind) = unique_kind {
            name.push(' ');
            name.push_str(kind.label());
        }
        let raw_stats = scanned.stats.unwrap_or_default();
        let stats = Self::format_event_stats(scanned.sockets, raw_stats);
        // Read dwSeed at item_data + 0x14 — stable per-item across area
        // unload/reload, used by loot-history dedup.
        let seed = if scanned.p_unit_data != 0 {
            self.state
                .ctx
                .process
                .read_memory::<u32>(scanned.p_unit_data as usize + item_data::SEED)
                .unwrap_or(0)
        } else {
            0
        };
        ItemDropEvent {
            unit_id: scanned.unit_id,
            class,
            quality,
            base_name,
            category: self.class_category(class),
            name,
            stats,
            name_is_runtime: false,
            runtime_stats_loaded,
            is_ethereal: scanned.is_ethereal,
            is_identified: scanned.is_identified,
            p_unit_data: scanned.p_unit_data,
            seed,
            history_pushed: false,
            tier: self.class_tier(class),
            unique_kind,
            sockets: scanned.sockets,
            clvl: self.char_level,
            ilvl: scanned.item_level,
            player_class: self.player_class,
            filter: None,
        }
    }

    fn format_event_stats(sockets: u8, raw_stats: String) -> String {
        if sockets > 0 {
            if raw_stats.is_empty() {
                format!("Socketed ({})", sockets)
            } else {
                format!("Socketed ({})\n{}", sockets, raw_stats)
            }
        } else {
            raw_stats
        }
    }

    /// Read item bonus description from the items.txt string table.
    ///
    /// Items like Median XL Cycles store their property description as a
    /// string-table ID in items.txt at record offset +0xB6 (u16).  The
    /// string contains the full tooltip in bottom-to-top line order.
    fn read_item_desc_from_txt(&self, injector: &D2Injector, class: u32) -> Option<String> {
        let count: u32 = self
            .state
            .ctx
            .process
            .read_memory(self.state.ctx.d2_common + d2common::ITEMS_TXT_COUNT)
            .ok()?;
        let base_ptr: u32 = self
            .state
            .ctx
            .process
            .read_memory(self.state.ctx.d2_common + d2common::ITEMS_TXT)
            .ok()?;
        if class >= count || base_ptr == 0 {
            return None;
        }
        let record = base_ptr as usize + class as usize * items_txt::RECORD_SIZE;
        let sid: u16 = self
            .state
            .ctx
            .process
            .read_memory(record + items_txt::DESC_STR_ID)
            .ok()?;
        if sid == 0 || sid == 0xFFFF {
            return None;
        }
        let raw = injector
            .get_string(&self.state.ctx.process, sid, 500)
            .ok()?;
        let clean = strip_color_codes(&raw);
        if clean.trim().is_empty() {
            return None;
        }

        let stat_section = clean.splitn(2, "\n\n").next().unwrap_or(&clean);
        let lines: Vec<&str> = stat_section
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with("Cube ")
            })
            .rev()
            .collect();
        if lines.is_empty() {
            return None;
        }
        Some(lines.join("\n"))
    }
}
