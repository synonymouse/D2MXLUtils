use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use chrono::Local;

/// Simple file logger used by the backend.
///
/// Writes log lines to `d2mxlutils.log` inside the app-data directory
/// (`%APPDATA%\com.d2mxlutils.app` on Windows), matching the location used
/// by `settings.json`, `profiles/`, and `items-cache.json`. Falls back to the
/// directory next to the executable if `%APPDATA%` cannot be resolved.

const APP_DIR_NAME: &str = "com.d2mxlutils.app";
const LOG_FILE_NAME: &str = "d2mxlutils.log";

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

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

fn write_line(prefix: &str, msg: &str) {
    let path = get_log_path();

    // Prepend a simple timestamp to every log line.
    let now = Local::now();
    let ts = now.format("%Y-%m-%d %H:%M:%S");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {}{}", ts, prefix, msg);
    }
}

pub fn info(msg: &str) {
    // Mirror to stdout for convenient live debugging
    println!("{}", msg);
    write_line("[INFO] ", msg);
}

pub fn error(msg: &str) {
    // Mirror to stderr for convenient live debugging
    eprintln!("{}", msg);
    write_line("[ERROR] ", msg);
}
