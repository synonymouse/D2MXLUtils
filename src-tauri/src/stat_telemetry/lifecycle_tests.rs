use super::*;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[test]
fn periodic_when_due_uses_elapsed_without_catchup() {
    let start = Instant::now();
    let counters = StatTelemetryCounters::default();
    let mut session = TelemetrySession::default();
    let marker = session.poll(start, || Some(counters.snapshot())).unwrap();
    assert_eq!(marker.render(None)["event"], "start");
    counters.injector_attempt(InjectorCall::GetUnitStat);
    assert!(session
        .poll(start + Duration::from_secs(29), || panic!("early snapshot"))
        .is_none());
    let event = session
        .poll(start + Duration::from_secs(30), || {
            Some(counters.snapshot())
        })
        .unwrap();
    let fields = event.render(None);
    assert_eq!(fields["elapsed_seconds"], 30.0);
    assert_eq!(fields["injector_attempts"]["delta"][3], 1);
    assert_eq!(fields["injector_attempts"]["per_second"][3], 1.0 / 30.0);
    let delayed = session
        .poll(start + Duration::from_secs(121), || {
            Some(counters.snapshot())
        })
        .unwrap();
    assert_eq!(delayed.render(None)["elapsed_seconds"], 91.0);
    assert!(session
        .poll(start + Duration::from_secs(150), || panic!(
            "catchup snapshot"
        ))
        .is_none());
    assert!(session
        .poll(start + Duration::from_secs(151), || Some(
            counters.snapshot()
        ))
        .is_some());
}

#[test]
fn contention_when_due_keeps_baseline_and_releases_guard() {
    let start = Instant::now();
    let counters = Mutex::new(StatTelemetryCounters::default());
    let snapshot = || try_snapshot(&counters, StatTelemetryCounters::snapshot);
    let mut session = TelemetrySession::default();
    session.poll(start, snapshot).unwrap();
    let guard = counters.lock().unwrap();
    guard.consumer(StatConsumer::Level).fallback_attempt();
    assert!(session
        .poll(start + Duration::from_secs(30), snapshot)
        .is_none());
    drop(guard);
    let event = session
        .poll(start + Duration::from_secs(45), snapshot)
        .unwrap();
    assert!(counters.try_lock().is_ok());
    let fields = event.render(None);
    assert_eq!(fields["elapsed_seconds"], 45.0);
    assert_eq!(
        fields["consumers"]["level"]["fallback"]["delta"],
        serde_json::json!([1, 0, 0])
    );
}

#[test]
fn final_when_idle_or_zero_elapsed_emits_nothing() {
    let start = Instant::now();
    let counters = StatTelemetryCounters::default();
    let mut idle = TelemetrySession::default();
    idle.poll(start, || Some(counters.snapshot())).unwrap();
    assert!(idle
        .finish(start + Duration::from_secs(1), || Some(counters.snapshot()))
        .is_none());
    let mut zero = TelemetrySession::default();
    zero.poll(start, || Some(counters.snapshot())).unwrap();
    counters.injector_attempt(InjectorCall::GetString);
    assert!(zero.finish(start, || Some(counters.snapshot())).is_none());
}

#[test]
fn attach_when_snapshot_unavailable_defers_baseline_without_false_delta() {
    let start = Instant::now();
    let counters = StatTelemetryCounters::default();
    let mut session = TelemetrySession::default();
    assert!(session.poll(start, || None).is_none());
    counters.injector_attempt(InjectorCall::GetString);
    let marker = session
        .poll(start + Duration::from_secs(2), || Some(counters.snapshot()))
        .unwrap();
    assert_eq!(marker.render(None)["injector_attempts"]["cumulative"][0], 1);
    assert!(session
        .poll(start + Duration::from_secs(31), || panic!("early snapshot"))
        .is_none());
    let event = session
        .poll(start + Duration::from_secs(32), || {
            Some(counters.snapshot())
        })
        .unwrap();
    assert_eq!(event.render(None)["injector_attempts"]["delta"][0], 0);
    let mut next = TelemetrySession::default();
    let marker = next
        .poll(start, || Some(StatTelemetryCounters::default().snapshot()))
        .unwrap();
    assert_eq!(marker.render(None)["injector_attempts"]["cumulative"][0], 0);
}

#[test]
fn poisoned_snapshot_when_due_or_final_is_skipped() {
    let start = Instant::now();
    let counters = Mutex::new(StatTelemetryCounters::default());
    let snapshot = || try_snapshot(&counters, StatTelemetryCounters::snapshot);
    let mut session = TelemetrySession::default();
    session.poll(start, snapshot).unwrap();
    let result = std::panic::catch_unwind(|| {
        let _guard = counters.lock().unwrap();
        panic!("poison fixture");
    });
    assert!(result.is_err());
    assert!(session
        .poll(start + Duration::from_secs(30), snapshot)
        .is_none());
    assert!(session
        .finish(start + Duration::from_secs(31), snapshot)
        .is_none());
}
