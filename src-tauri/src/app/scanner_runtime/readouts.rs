//! Complete readout/sampling operations; counters and last-good values stay in worker.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter};

use crate::logger::{error as log_error, info as log_info};
use crate::scanner_state::SharedScannerState;
use crate::{breakpoints, damage_stats, offsets, stats_panel};

pub(super) fn sample_breakpoints(
    shared_state: &SharedScannerState,
    app_handle: &AppHandle,
    last_player_bp: &mut Option<breakpoints::BreakpointData>,
    last_merc_bp: &mut Option<breakpoints::BreakpointData>,
) {
    let injector = shared_state.injector.lock().unwrap();
    let player_result = breakpoints::read_unit_breakpoint_data(
        &shared_state.ctx,
        &injector,
        offsets::d2client::PLAYER_UNIT,
    );
    let merc_result = breakpoints::read_unit_breakpoint_data(
        &shared_state.ctx,
        &injector,
        offsets::d2client::MERCENARY_UNIT,
    );
    drop(injector);

    let player_data = match player_result {
        Ok(data) => {
            *last_player_bp = data.clone();
            data
        }
        Err(()) => last_player_bp.clone(),
    };
    let merc_data = match merc_result {
        Ok(data) => {
            *last_merc_bp = data.clone();
            data
        }
        Err(()) => last_merc_bp.clone(),
    };

    #[derive(serde::Serialize)]
    struct BreakpointsPayload {
        player: Option<breakpoints::BreakpointData>,
        merc: Option<breakpoints::BreakpointData>,
    }
    let payload = BreakpointsPayload {
        player: player_data,
        merc: merc_data,
    };
    if let Err(e) = app_handle.emit("breakpoints-update", &payload) {
        log_error(&format!("Failed to emit breakpoints-update: {}", e));
    }
}

pub(super) fn sample_stats(
    shared_state: &SharedScannerState,
    app_handle: &AppHandle,
    last_player_damage: &mut Option<damage_stats::DamageStats>,
    last_merc_damage: &mut Option<damage_stats::DamageStats>,
    last_player_stats: &mut Option<stats_panel::CharacterStats>,
    last_merc_stats: &mut Option<stats_panel::CharacterStats>,
) {
    let injector = shared_state.injector.lock().unwrap();

    let player_damage = match damage_stats::read_unit_damage_stats(
        &shared_state.ctx,
        &injector,
        offsets::d2client::PLAYER_UNIT,
    ) {
        Ok(data) => {
            *last_player_damage = data.clone();
            data
        }
        Err(()) => last_player_damage.clone(),
    };
    let merc_damage = match damage_stats::read_unit_damage_stats(
        &shared_state.ctx,
        &injector,
        offsets::d2client::MERCENARY_UNIT,
    ) {
        Ok(data) => {
            *last_merc_damage = data.clone();
            data
        }
        Err(()) => last_merc_damage.clone(),
    };

    let player_stats = stats_panel::read_unit_character_stats(
        &shared_state.ctx,
        &injector,
        offsets::d2client::PLAYER_UNIT,
        last_player_stats.as_ref(),
    );
    *last_player_stats = player_stats.clone();
    let merc_stats = stats_panel::read_unit_character_stats(
        &shared_state.ctx,
        &injector,
        offsets::d2client::MERCENARY_UNIT,
        last_merc_stats.as_ref(),
    );
    *last_merc_stats = merc_stats.clone();
    drop(injector);

    #[derive(serde::Serialize)]
    struct UnitStatsPayload {
        class: u32,
        stats: std::collections::BTreeMap<u32, i32>,
        #[serde(rename = "baseStats")]
        base_stats: std::collections::BTreeMap<u32, i32>,
        damage: Option<damage_stats::DamageStats>,
    }
    #[derive(serde::Serialize)]
    struct StatsPayload {
        player: Option<UnitStatsPayload>,
        merc: Option<UnitStatsPayload>,
    }
    let payload = StatsPayload {
        player: player_stats.map(|s| UnitStatsPayload {
            class: s.class,
            stats: s.stats,
            base_stats: s.base_stats,
            damage: player_damage,
        }),
        merc: merc_stats.map(|s| UnitStatsPayload {
            class: s.class,
            stats: s.stats,
            base_stats: s.base_stats,
            damage: merc_damage,
        }),
    };
    if let Err(e) = app_handle.emit("stats-update", &payload) {
        log_error(&format!("Failed to emit stats-update: {}", e));
    }
}

pub(super) fn sample_dps(
    shared_state: &SharedScannerState,
    app_handle: &AppHandle,
    dps_reset_pending: &AtomicBool,
    dps_area_tick_counter: &mut u32,
) {
    const AREA_CHECK_EVERY: u32 = 5;
    let events = shared_state.dps_hook.drain();
    let manual_reset = dps_reset_pending.swap(false, Ordering::SeqCst);

    *dps_area_tick_counter = dps_area_tick_counter.wrapping_add(1);
    let area_token = if *dps_area_tick_counter % AREA_CHECK_EVERY == 0 {
        shared_state.read_current_area_token()
    } else {
        None
    };
    let area_change = match area_token {
        Some(token) => {
            // Sentinel -1 means first observation: record
            // without resetting.
            let prev = shared_state
                .last_area_token
                .swap(token as i64, Ordering::Relaxed);
            prev >= 0 && prev != token as i64
        }
        None => false,
    };

    if let Ok(mut meter) = shared_state.dps_meter.write() {
        if manual_reset || area_change {
            if area_change {
                log_info(&format!(
                    "DPS meter: area change → token 0x{:08X} (auto-reset)",
                    area_token.unwrap_or(0)
                ));
            }
            meter.reset();
        }
        for ev in &events {
            meter.ingest(ev.ts_ms, ev.delta_raw, ev.max_hp, ev.monster_level);
        }
        let snap = meter.snapshot(crate::tick_clock::now_ms());
        drop(meter);
        if let Err(e) = app_handle.emit("dps-update", &snap) {
            log_error(&format!("Failed to emit dps-update: {}", e));
        }
    }
}
