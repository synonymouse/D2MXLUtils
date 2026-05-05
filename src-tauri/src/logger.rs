use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Local;

/// Simple file logger used by the backend.
///
/// Writes log lines to `d2mxlutils.log` inside the app-data directory
/// (`%APPDATA%\com.d2mxlutils.app` on Windows), matching the location used
/// by `settings.json`, `profiles/`, and `items-cache.json`. Falls back to the
/// directory next to the executable if `%APPDATA%` cannot be resolved.

const APP_DIR_NAME: &str = "com.d2mxlutils.app";
const LOG_FILE_NAME: &str = "d2mxlutils.log";
const ROTATED_LOG_FILE_NAME: &str = "d2mxlutils.log.1";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const THROTTLE_WINDOW: Duration = Duration::from_secs(60);

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static THROTTLE: OnceLock<Mutex<HashMap<(&'static str, u32), ThrottleEntry>>> = OnceLock::new();

#[derive(Default)]
struct ThrottleEntry {
    last_logged: Option<Instant>,
    suppressed: u64,
    last_msg: String,
}

fn throttle_map() -> &'static Mutex<HashMap<(&'static str, u32), ThrottleEntry>> {
    THROTTLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_log_path() -> PathBuf {
    LOG_PATH
        .get_or_init(|| {
            let dir = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|p| p.join(APP_DIR_NAME))
                .or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                })
                .unwrap_or_else(|| PathBuf::from("."));
            let _ = std::fs::create_dir_all(&dir);
            dir.join(LOG_FILE_NAME)
        })
        .clone()
}

fn rotate_if_needed(path: &Path) {
    let len = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    if len <= MAX_LOG_BYTES {
        return;
    }
    let rotated = path.with_file_name(ROTATED_LOG_FILE_NAME);
    let _ = std::fs::remove_file(&rotated);
    let _ = std::fs::rename(path, &rotated);
}

fn write_line(prefix: &str, msg: &str) {
    let path = get_log_path();
    let lock = WRITE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = match lock.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    rotate_if_needed(&path);

    let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}] {}{}", ts, prefix, msg);
    }
}

pub fn info(msg: &str) {
    // Mirror to stdout for convenient live debugging
    println!("{}", msg);
    write_line("[INFO] ", msg);
}

#[track_caller]
pub fn error(msg: &str) {
    let caller = Location::caller();
    let key = (caller.file(), caller.line());

    let mut guard = match throttle_map().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    let now = Instant::now();
    let entry = guard.entry(key).or_default();

    let last = match entry.last_logged {
        Some(t) => t,
        None => {
            entry.last_logged = Some(now);
            entry.last_msg = msg.to_string();
            drop(guard);
            emit_error(msg);
            return;
        }
    };

    let same_msg = entry.last_msg == msg;
    let within_window = now.duration_since(last) < THROTTLE_WINDOW;

    if same_msg && within_window {
        entry.suppressed += 1;
        return;
    }

    let suppressed = entry.suppressed;
    let elapsed_sec = now.duration_since(last).as_secs();
    entry.last_logged = Some(now);
    entry.suppressed = 0;
    entry.last_msg = msg.to_string();
    drop(guard);

    if suppressed > 0 {
        if same_msg {
            emit_error(&format!(
                "{} (repeated {} more times in last {}s)",
                msg, suppressed, elapsed_sec
            ));
        } else {
            emit_error(&format!(
                "(previous error suppressed {} more times before message changed)",
                suppressed
            ));
            emit_error(msg);
        }
    } else {
        emit_error(msg);
    }
}

fn emit_error(msg: &str) {
    // Mirror to stderr for convenient live debugging
    eprintln!("{}", msg);
    write_line("[ERROR] ", msg);
}
