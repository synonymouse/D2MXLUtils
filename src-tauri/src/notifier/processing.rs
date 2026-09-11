//! New-item acquisition, filter decisions and notification/history publication.

use super::visibility::visibility_mask_ops;
use super::{DropScanner, ItemDropEvent};
use crate::d2types::{ItemData, ScannedItem, UnitAny};
use crate::logger::{error as log_error, info as log_info};
use crate::offsets::{stat_list, unit_type};
use crate::rules::{MatchContext, PartialFilterDecision, Visibility};
use crate::scanner_state::CachedFilterDecision;
use crate::stat_telemetry::StatConsumer;
use crate::unit_stats_reader::fallback::StatReadContext;
use std::sync::atomic::Ordering;

impl DropScanner {
    pub(super) fn process_scanned_item(
        &mut self,
        scanned: ScannedItem,
        events: &mut Vec<ItemDropEvent>,
    ) {
        let p_unit = scanned.p_unit;
        let mut event = self.to_event(scanned);
        let unit_id = event.unit_id;

        let mut should_emit = true;
        let mut hook_bits_may_exist = false;
        let mut cached_filter_decision = None;
        let filter_snapshot = {
            let guard = self.state.filter_config.read().unwrap();
            guard.as_ref().map(|filter_arc| {
                (
                    filter_arc.clone(),
                    self.state.filter_generation.load(Ordering::SeqCst),
                )
            })
        };
        if let Some((filter_arc, filter_generation)) = filter_snapshot {
            if let Ok(filter) = filter_arc.read() {
                let decision = loop {
                    let ctx = MatchContext::new(&event);
                    match filter.decide_partial(&ctx) {
                        PartialFilterDecision::Ready(decision) => break decision,
                        PartialFilterDecision::Needs(needs) => {
                            let before_stats = event.runtime_stats_loaded;

                            if needs.runtime_stats {
                                self.enrich_event_stats(&mut event, p_unit);
                            }

                            if before_stats == event.runtime_stats_loaded {
                                let ctx = MatchContext::new(&event);
                                break filter.decide(&ctx);
                            }
                        }
                    }
                };

                if self.live_match_highlight {
                    if let Some(line) = decision.matched_line {
                        self.pending_matched_lines.push(line);
                    }
                }

                cached_filter_decision = Some(CachedFilterDecision::from_decision(
                    filter_generation,
                    &decision,
                ));

                if self.verbose_filter_logging {
                    let ctx = MatchContext::new(&event);
                    let winner = filter.rules.iter().rev().find(|r| ctx.matches(r));
                    let reason = match winner {
                        Some(r) => {
                            format!("winner={}", r.name_pattern.as_deref().unwrap_or("<any>"))
                        }
                        None => {
                            format!("no rule matched (hide_all={})", filter.hide_all)
                        }
                    };
                    let vis_label = match decision.visibility {
                        Visibility::Show => "SHOW",
                        Visibility::Hide => "HIDE",
                        Visibility::Default => "DEFAULT",
                    };
                    let category_label = event
                        .category
                        .as_deref()
                        .map(|c| format!(" [{}]", c.replace('\n', "|")))
                        .unwrap_or_default();
                    log_info(&format!(
                        "[Filter] \"{} {}\"{} ({}, class={}) -> {} notify={} | {}",
                        event.name,
                        event.base_name,
                        category_label,
                        event.quality,
                        event.class,
                        vis_label,
                        decision.notification.is_some(),
                        reason
                    ));
                }

                if decision
                    .notification
                    .as_ref()
                    .map(|n| n.display_stats)
                    .unwrap_or(false)
                {
                    self.enrich_event_stats(&mut event, p_unit);
                }

                if self.loot_hook.is_injected() {
                    hook_bits_may_exist = true;
                    let failed_ops = self.apply_visibility_mask_ops(
                        event.unit_id,
                        visibility_mask_ops(decision.visibility),
                    );
                    self.pending_visibility_ops
                        .record_failed(event.unit_id, failed_ops);
                }

                match decision.notification {
                    Some(n) => event.filter = Some(n),
                    None => should_emit = false,
                }
            }
        }

        // Cache enriched event for the map-marker pass.
        self.state
            .recent_events
            .write()
            .unwrap()
            .insert(event.unit_id, event.clone());
        if let Some(decision) = cached_filter_decision {
            self.state
                .recent_filter_decisions
                .write()
                .unwrap()
                .insert(event.unit_id, decision);
        } else {
            self.state
                .recent_filter_decisions
                .write()
                .unwrap()
                .remove(&event.unit_id);
        }

        if should_emit {
            // Push to session history (only filter-matched items — same gate
            // as overlay notifications).
            if event.filter.is_some() {
                let color = event
                    .filter
                    .as_ref()
                    .and_then(|n| n.color.as_ref())
                    .map(|c| c.lowercase_name().to_string());
                let entry = crate::loot_history::LootEntry {
                    unit_id: event.unit_id,
                    timestamp_ms: crate::loot_history::now_ms(),
                    name: event.name.clone(),
                    quality: event.quality.clone(),
                    color,
                    pickup: crate::loot_history::PickupState::Pending,
                    seed: event.seed,
                };
                // Only fresh inserts emit `loot-history-entry`; dedup-merges
                // silently update the existing row (frontend keys by `seed`).
                let outcome = if let Ok(mut hist) = self.loot_history.write() {
                    hist.push(entry)
                } else {
                    crate::loot_history::PushOutcome::Duplicate
                };
                event.history_pushed =
                    matches!(outcome, crate::loot_history::PushOutcome::Inserted);
            }
            events.push(event);
        }

        // Keep after show/hide writes: inspected releases the trampoline gate.
        if self.loot_hook.is_injected() {
            hook_bits_may_exist = true;
            if let Err(e) = self
                .loot_hook
                .add_inspected_unit_id(&self.state.ctx, unit_id)
            {
                log_error(&format!("Failed to mark item {} inspected: {}", unit_id, e));
            }
        }
        if hook_bits_may_exist {
            self.hook_bits.mark_written(unit_id);
        }
    }

    /// Process a single unit, returning a fully scanned item if it's a new item.
    pub(super) fn scan_unit(&mut self, p_unit: u32, unit: &UnitAny) -> Option<ScannedItem> {
        // Only process items (unit_type == 4)
        if unit.unit_type != unit_type::ITEM {
            return None;
        }

        // Skip if we've already seen this item
        if self.seen_items.contains(&unit.unit_id) {
            return None;
        }

        // Read ItemData
        if unit.p_unit_data == 0 {
            return None;
        }

        let item_data: ItemData = self
            .state
            .ctx
            .process
            .read_memory(unit.p_unit_data as usize)
            .ok()?;

        // Create scanned item and keep the existing socket-count enrichment only.
        let mut scanned = ScannedItem::from_unit(unit, &item_data, p_unit);

        {
            let injector = self.state.injector.lock().unwrap();
            if item_data.is_socketed() {
                if let Ok(value) =
                    StatReadContext::new(&self.state.ctx, &injector, StatConsumer::Sockets)
                        .read_stat(p_unit, u32::from(stat_list::STAT_SOCKETS))
                {
                    scanned.sockets = u32::from_ne_bytes(value.to_ne_bytes()).min(6) as u8;
                }
            }
        }

        // Mark as seen
        self.seen_items.insert(unit.unit_id);

        Some(scanned)
    }
}
