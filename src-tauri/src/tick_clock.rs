/// Must share the trampoline's time base (`GetTickCount`); otherwise
/// `snapshot(now) - event.ts_ms` is meaningless.
#[cfg(target_os = "windows")]
pub fn now_ms() -> u32 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    unsafe { GetTickCount() }
}

/// Wine's `GetTickCount` (like real Windows') is system-wide, not
/// per-process — implemented on top of the host's monotonic clock, the
/// same one every process (including this one) can read directly. So,
/// same as the Windows arm above avoiding a call into the remote process
/// entirely, we can too: `CLOCK_MONOTONIC` read locally lines up with
/// what the trampoline's remote `GetTickCount()` call embeds, without
/// needing a ptrace round-trip on every tick.
#[cfg(target_os = "linux")]
pub fn now_ms() -> u32 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ((ts.tv_sec as u64 * 1000) + (ts.tv_nsec as u64 / 1_000_000)) as u32
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn now_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}
