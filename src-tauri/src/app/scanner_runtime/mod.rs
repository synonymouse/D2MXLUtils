//! Game discovery and auto-start; worker owns attachment through shutdown.

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod readouts;
mod worker;

use worker::start_scanner_internal;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::AppHandle;

use crate::loot_history::LootHistory;
use crate::notifier::ItemsDictionary;
use crate::{breakpoints, rules};

// Windows-only imports for scanner window discovery
#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

/// Check if Diablo II window exists
#[cfg(target_os = "windows")]
fn is_diablo2_running() -> bool {
    let class_wide: Vec<u16> = OsStr::new("Diablo II")
        .encode_wide()
        .chain(Some(0))
        .collect();

    let hwnd = unsafe { FindWindowW(PCWSTR(class_wide.as_ptr()), PCWSTR::null()) };
    hwnd.is_ok() && !hwnd.unwrap().0.is_null()
}

/// X11 window lookup (works regardless of which Wine/Proton prefix the game
/// is running in — see `process/`'s Linux `D2Context`).
#[cfg(target_os = "linux")]
pub(super) fn is_diablo2_running() -> bool {
    crate::process::open_process_by_window_class(crate::process::LINUX_WINDOW_TITLE).is_ok()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn is_diablo2_running() -> bool {
    false
}

/// Spawn background thread that monitors for Diablo II and auto-starts scanner
pub(crate) fn spawn_auto_scanner(
    is_scanning: Arc<AtomicBool>,
    should_auto_scan: Arc<AtomicBool>,
    filter_config: Arc<RwLock<Option<rules::FilterConfig>>>,
    verbose_filter_logging: Arc<AtomicBool>,
    live_match_highlight: Arc<AtomicBool>,
    auto_always_show_items: Arc<AtomicBool>,
    auto_no_pickup: Arc<AtomicBool>,
    reveal_hidden_active: Arc<AtomicBool>,
    filter_config_generation: Arc<AtomicU64>,
    scanner_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    game_status: Arc<AtomicU8>,
    items_dictionary: Arc<RwLock<Option<ItemsDictionary>>>,
    loot_history: Arc<RwLock<LootHistory>>,
    breakpoints_polling: Arc<AtomicBool>,
    stats_polling: Arc<AtomicBool>,
    weapon_base_catalog: Arc<RwLock<Option<breakpoints::WeaponBaseCatalog>>>,
    dps_reset_pending: Arc<AtomicBool>,
    #[cfg(any(target_os = "windows", target_os = "linux"))] scanner_shared_state: Arc<
        RwLock<Option<Arc<crate::scanner_state::SharedScannerState>>>,
    >,
    #[cfg(any(target_os = "windows", target_os = "linux"))] attach_failure_streak: Arc<AtomicU32>,
    app_handle: AppHandle,
) {
    thread::spawn(move || {
        while should_auto_scan.load(Ordering::SeqCst) {
            // If not currently scanning, check if D2 is running
            if !is_scanning.load(Ordering::SeqCst) && is_diablo2_running() {
                start_scanner_internal(
                    is_scanning.clone(),
                    filter_config.clone(),
                    verbose_filter_logging.clone(),
                    live_match_highlight.clone(),
                    auto_always_show_items.clone(),
                    auto_no_pickup.clone(),
                    reveal_hidden_active.clone(),
                    filter_config_generation.clone(),
                    scanner_thread.clone(),
                    game_status.clone(),
                    items_dictionary.clone(),
                    loot_history.clone(),
                    breakpoints_polling.clone(),
                    stats_polling.clone(),
                    weapon_base_catalog.clone(),
                    dps_reset_pending.clone(),
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    scanner_shared_state.clone(),
                    #[cfg(any(target_os = "windows", target_os = "linux"))]
                    attach_failure_streak.clone(),
                    app_handle.clone(),
                );
            }

            // Check for the game launching frequently — this poll was the
            // single biggest fixed delay before the scanner even started
            // attaching (up to 2s of the reported 5-10s "time to ready").
            // `is_diablo2_running()` is cheap (a single X11 property read
            // over the shared connection on Linux, `FindWindowW` on
            // Windows), so polling this often is not a real cost. But a
            // *failed* attach is not cheap on Linux — `D2Injector::new`
            // hijacks a live thread via ptrace, and retrying that at this
            // same 300ms cadence against an actively-playing game (where
            // every thread is usually mid-syscall, so the attach keeps
            // failing for real, non-transient reasons) hammered the game
            // process with 100+ ptrace attach/detach cycles in under a
            // minute in one observed session — the leading suspect for a
            // crash that immediately followed. Back off exponentially on
            // consecutive failures instead of retrying at full speed.
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            let poll_ms = match attach_failure_streak.load(Ordering::Relaxed) {
                0 => 300,
                n => (300u64 * 2u64.pow(n.min(6))).min(10_000),
            };
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            let poll_ms = 300u64;
            thread::sleep(Duration::from_millis(poll_ms));
        }
    });
}
