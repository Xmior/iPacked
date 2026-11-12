//! Log of everything iPacked does with connected devices:
//! ~/Library/Logs/iPacked/device.log, plus a live `device-log` event for the UI.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

static APP: OnceLock<AppHandle> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

pub fn init(app: AppHandle) {
    let _ = APP.set(app);
}

pub fn path() -> PathBuf {
    let d = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Library/Logs/iPacked");
    let _ = std::fs::create_dir_all(&d);
    d.join("device.log")
}

/// Never log secrets: the SSH password is passed via env, and Apple ID
/// passwords never reach device commands.
pub fn log(line: impl AsRef<str>) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let text = line.as_ref().trim_end();
    if text.is_empty() {
        return;
    }
    let _g = LOCK.lock();
    let p = path();
    // Keep the file bounded (~5 MB): rotate to device.log.1.
    if std::fs::metadata(&p).map(|m| m.len() > 5_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        for l in text.lines() {
            let _ = writeln!(f, "{ts}  {l}");
        }
    }
    if let Some(app) = APP.get() {
        let _ = app.emit("device-log", serde_json::json!({ "t": ts, "line": text }));
    }
}

/// Last `n` lines of the log file.
pub fn tail(n: usize) -> Vec<String> {
    let s = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
}
