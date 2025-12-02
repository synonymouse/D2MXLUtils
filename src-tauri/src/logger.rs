use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Simple file logger used by the backend.
///
/// Writes log lines to `d2mxlutils.log` next to the executable.

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn get_log_path() -> PathBuf {
    LOG_PATH
        .get_or_init(|| {
            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
            let dir = exe_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            dir.join("d2mxlutils.log")
        })
        .clone()
}

fn write_line(prefix: &str, msg: &str) {
    let path = get_log_path();

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}{}", prefix, msg);
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



