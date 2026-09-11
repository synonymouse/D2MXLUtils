use super::*;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct LogDirectory(PathBuf);
impl Drop for LogDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove isolated log directory");
    }
}

#[test]
fn production_logger_when_isolated_records_real_memory_and_final() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = LogDirectory(
        std::env::temp_dir().join(format!("stat-telemetry-{}-{nonce}", std::process::id())),
    );
    std::fs::create_dir(&directory.0).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "stat_telemetry::log_tests::logger_driver_child",
            "--nocapture",
        ])
        .env("D2MXL_TELEMETRY_DRIVER", "1")
        .env("APPDATA", &directory.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log =
        std::fs::read_to_string(directory.0.join("com.d2mxlutils.app/d2mxlutils.log")).unwrap();
    let records: Vec<serde_json::Value> = log
        .lines()
        .map(|line| serde_json::from_str(line.split_once("[INFO] ").unwrap().1).unwrap())
        .collect();
    assert_eq!(records.len(), 5);
    assert_eq!(records[0]["event"], "start");
    assert_eq!(records[0]["schema"], 1);
    assert_eq!(records[0]["package_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(records[0]["observational"], true);
    let periodic = &records[1];
    assert_eq!(periodic["event"], "periodic");
    assert_eq!(periodic["elapsed_seconds"], 45.0);
    assert_eq!(periodic["injector_attempts"]["delta"][3], 1);
    assert_eq!(
        periodic["consumers"]["stats"]["bulk"]["delta"],
        serde_json::json!([1, 1, 0, 0, 3, 2])
    );
    assert!(periodic["memory"]["working_set_bytes"].as_u64().unwrap() > 0);
    assert!(periodic["memory"]["private_commit_bytes"].as_u64().unwrap() > 0);
    assert_eq!(records[2]["event"], "final");
    assert_eq!(records[2]["elapsed_seconds"], 0.5);
    assert_eq!(records[2]["memory"], "unavailable");
    assert_eq!(records[2]["injector_attempts"]["cumulative"][3], 2);
    assert_eq!(records[2]["injector_attempts"]["per_second"][3], 2.0);
    assert_eq!(records[3]["event"], "start");
    assert_eq!(records[3]["injector_attempts"]["cumulative"][3], 0);
    assert_eq!(records[4]["event"], "final");
    assert_eq!(records[4]["elapsed_seconds"], 0.25);
    assert!(log.len() < 24_000);
    println!("{log}");
}

#[test]
fn logger_driver_child() {
    if std::env::var_os("D2MXL_TELEMETRY_DRIVER").is_none() {
        return;
    }
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };
    let pid = std::process::id();
    // SAFETY: OpenProcess returns an owned handle for this live process; ProcessHandle closes it.
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }.unwrap();
    let process = crate::process::ProcessHandle { handle, pid };
    let inaccessible = crate::process::ProcessHandle {
        handle: windows::Win32::Foundation::HANDLE::default(),
        pid: 0,
    };
    let injector = Mutex::new(crate::injection::D2Injector::for_marker_test(
        std::iter::empty(),
    ));
    let snapshot = || try_snapshot(&injector, |value| value.telemetry.snapshot());
    let start = Instant::now();
    let mut session = TelemetrySession::default();
    session
        .poll(start, snapshot)
        .unwrap()
        .log(|| memory::sample(&process));
    let guard = injector.lock().unwrap();
    guard.telemetry.injector_attempt(InjectorCall::GetUnitStat);
    let stats = guard.telemetry.consumer(StatConsumer::Stats);
    stats.direct_attempt(DirectKind::Bulk);
    stats.direct_completed(DirectKind::Bulk);
    stats.direct_values(
        DirectKind::Bulk,
        ValueCounts {
            found: 3,
            missing: 2,
        },
    );
    assert!(session
        .poll(start + Duration::from_secs(30), snapshot)
        .is_none());
    drop(guard);
    session
        .poll(start + Duration::from_secs(45), snapshot)
        .unwrap()
        .log(|| {
            assert!(injector.try_lock().is_ok());
            memory::sample(&process)
        });
    injector
        .lock()
        .unwrap()
        .telemetry
        .injector_attempt(InjectorCall::GetUnitStat);
    session
        .finish(start + Duration::from_millis(45_500), snapshot)
        .unwrap()
        .log(|| {
            assert!(injector.try_lock().is_ok());
            memory::sample(&inaccessible)
        });
    let counters = StatTelemetryCounters::default();
    let mut next = TelemetrySession::default();
    next.poll(start, || Some(counters.snapshot()))
        .unwrap()
        .log(|| memory::sample(&process));
    counters.consumer(StatConsumer::Level).fallback_attempt();
    next.finish(start + Duration::from_millis(250), || {
        Some(counters.snapshot())
    })
    .unwrap()
    .log(|| memory::sample(&inaccessible));
}
