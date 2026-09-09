#[cfg(target_os = "windows")]
#[test]
fn wrappers_return_existing_errors_when_preparation_fails() {
    let injector = crate::injection::D2Injector::for_marker_test([73].into_iter());
    let process = crate::process::ProcessHandle {
        handle: windows::Win32::Foundation::HANDLE::default(),
        pid: 0,
    };
    let results = [
        injector.get_item_name(&process, 0),
        injector.get_item_stats(&process, 0),
        injector
            .get_unit_stat(&process, 0, 0)
            .map(|value| value.to_string()),
        injector.get_string(&process, 0, 1),
    ];
    assert!(results[..3].iter().all(|result| result
        .as_ref()
        .unwrap_err()
        .starts_with("WriteProcessMemory failed:")));
    assert!(results[3]
        .as_ref()
        .unwrap_err()
        .starts_with("CreateRemoteThread failed:"));
    assert_eq!(injector.new_automap_cell(&process), Ok(73));
    assert_eq!(injector.telemetry.snapshot().injector_attempts, [1; 5]);
}
use super::*;
use std::time::Duration;

#[test]
fn counters_separate_consumers_and_request_units() {
    let counters = StatTelemetryCounters::default();
    let stats = counters.consumer(StatConsumer::Stats);
    stats.direct_attempt(DirectKind::Bulk);
    stats.direct_completed(DirectKind::Bulk);
    stats.direct_values(
        DirectKind::Bulk,
        ValueCounts {
            found: 3,
            missing: 2,
        },
    );
    stats.semantic_reject(DirectKind::Bulk);
    stats.fallback_attempt();
    stats.fallback_ok();
    let level = counters.consumer(StatConsumer::Level);
    level.direct_attempt(DirectKind::Single);
    level.reader_error(DirectKind::Single);
    level.fallback_attempt();
    level.fallback_error();
    let snapshot = counters.snapshot();
    assert_eq!(snapshot.consumers[0].bulk, [1, 1, 0, 1, 3, 2]);
    assert_eq!(snapshot.consumers[0].single, [0; 6]);
    assert_eq!(snapshot.consumers[0].fallback, [1, 1, 0]);
    assert_eq!(snapshot.consumers[3].single, [1, 0, 1, 0, 0, 0]);
    assert_eq!(snapshot.consumers[3].fallback, [1, 0, 1]);
    assert_eq!(snapshot.consumers[4], ConsumerSnapshot::default());
    assert_eq!(
        StatTelemetryCounters::default().snapshot(),
        TelemetrySnapshot::default()
    );
}

#[test]
fn delta_saturates_when_attachment_counters_reset() {
    let counters = StatTelemetryCounters::default();
    counters.injector_attempt(InjectorCall::GetString);
    let previous = counters.snapshot();
    assert_eq!(
        TelemetrySnapshot::default().delta(&previous),
        TelemetrySnapshot::default()
    );
    assert_eq!(
        previous
            .delta(&TelemetrySnapshot::default())
            .injector_attempts,
        [1, 0, 0, 0, 0]
    );
    assert_eq!(rate_per_second(5, Duration::ZERO), None);
    assert_eq!(rate_per_second(5, Duration::from_millis(2500)), Some(2.0));
}

#[cfg(target_os = "windows")]
#[test]
fn memory_query_reports_current_process_and_invalid_handle() {
    use windows::Win32::{Foundation::HANDLE, System::Threading::GetCurrentProcess};
    // SAFETY: GetCurrentProcess returns a borrowed pseudo handle, not owned memory.
    let handle = unsafe { GetCurrentProcess() };
    let sample = memory::query_handle(handle).unwrap();
    assert!(sample.working_set_bytes.0 > 0);
    assert!(sample.private_commit_bytes.0 > 0);
    assert_eq!(memory::query_handle(HANDLE::default()), None);
}

#[cfg(target_os = "windows")]
#[test]
fn failed_cell_wrapper_counts_entry_and_new_injector_starts_empty() {
    let mut injector = crate::injection::D2Injector::for_marker_test(std::iter::empty());
    injector.marker_allocator = None;
    let process = crate::process::ProcessHandle {
        handle: windows::Win32::Foundation::HANDLE::default(),
        pid: 0,
    };
    assert!(injector
        .new_automap_cell(&process)
        .unwrap_err()
        .starts_with("CreateRemoteThread failed:"));
    assert_eq!(
        injector.telemetry.snapshot().injector_attempts,
        [0, 0, 0, 0, 1]
    );
    let next = crate::injection::D2Injector::for_marker_test(std::iter::empty());
    assert_eq!(next.telemetry.snapshot(), TelemetrySnapshot::default());
}
