//! Minimal file logger.
//!
//! A release build of a shell replacement has `windows_subsystem = "windows"`, so
//! there is no console and `eprintln!` goes nowhere. Without a log, "the dock is
//! blank" is unfalsifiable guesswork — which is exactly the hole this plugs.
//!
//! Appends to `%LOCALAPPDATA%\macui\macui.log`, truncating at startup if the file has
//! grown past a megabyte so it can never fill a disk.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn log_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("macui").join("macui.log")
}

pub fn init(tag: &str) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *FILE.lock().unwrap() = Some(f);
    }
    write(&format!("--- start ({tag}, pid {}) ---", std::process::id()));
}

pub fn write(line: &str) {
    // Still useful under `cargo run`, where there IS a console.
    eprintln!("[macui] {line}");
    if let Ok(mut guard) = FILE.lock() {
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}
