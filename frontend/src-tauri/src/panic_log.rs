//! Process-wide panic hook that appends each panic (message, location,
//! thread, backtrace) to `<app data>/com.meetily.ai/logs/panic.log`.
//!
//! The release build has no console on Windows, so without this the panic
//! message and backtrace are lost. Recording is best-effort: it never panics
//! and the previously installed hook still runs.

use std::any::Any;
use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::Location;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Must match `identifier` in `tauri.conf.json` (pinned by a unit test), so the
/// log lands next to the DB under Tauri's `app_data_dir()`.
const APP_IDENTIFIER: &str = "com.meetily.ai";

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the panic hook. Repeated calls are no-ops.
pub fn install() {
    install_at(default_log_path());
}

fn install_at(path: PathBuf) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        let entry = format_entry(
            &chrono::Local::now().to_rfc3339(),
            std::thread::current().name(),
            &payload_message(info.payload()),
            info.location(),
            &Backtrace::force_capture().to_string(),
        );
        let _ = append_entry(&path, &entry);
    }));
}

fn default_log_path() -> PathBuf {
    match dirs::data_dir() {
        Some(dir) => dir.join(APP_IDENTIFIER).join("logs").join("panic.log"),
        None => std::env::temp_dir().join("meetily-panic").join("panic.log"),
    }
}

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload; type unavailable>".to_string()
    }
}

fn format_location(location: Option<&Location<'_>>) -> String {
    match location {
        Some(l) => format!("{}:{}:{}", l.file(), l.line(), l.column()),
        None => "unknown".to_string(),
    }
}

fn format_entry(
    timestamp: &str,
    thread: Option<&str>,
    message: &str,
    location: Option<&Location<'_>>,
    backtrace: &str,
) -> String {
    let mut entry = format!(
        "==== panic @ {} ====\nversion: {}\npid: {}\nthread: {}\nlocation: {}\nmessage: {}\nbacktrace:\n",
        timestamp,
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        thread.unwrap_or("<unnamed>"),
        format_location(location),
        message,
    );
    for line in backtrace.lines() {
        entry.push_str("    ");
        entry.push_str(line);
        entry.push('\n');
    }
    entry.push('\n');
    entry
}

/// Append one entry, creating the parent directory on demand.
fn append_entry(path: &Path, entry: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(entry.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_message_handles_str_string_and_other() {
        assert_eq!(payload_message(&"boom"), "boom");
        assert_eq!(payload_message(&String::from("owned")), "owned");
        assert_eq!(
            payload_message(&42_u32),
            "<non-string panic payload; type unavailable>"
        );
    }

    #[test]
    fn format_entry_fills_fallbacks() {
        let entry = format_entry("ts", None, "msg", None, "frame a\nframe b");
        assert!(entry.starts_with("==== panic @ ts ====\n"));
        assert!(entry.contains("thread: <unnamed>\n"));
        assert!(entry.contains("location: unknown\n"));
        assert!(entry.contains("message: msg\n"));
        assert!(entry.contains("    frame a\n    frame b\n"));
        assert!(entry.contains(&format!("version: {}\n", env!("CARGO_PKG_VERSION"))));
        assert!(entry.contains(&format!("pid: {}\n", std::process::id())));
    }

    #[test]
    fn append_entry_creates_dir_and_keeps_earlier_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("panic.log");
        let first = format_entry("t1", Some("main"), "first", None, "bt1");
        let second = format_entry("t2", Some("worker"), "second", None, "bt2");

        append_entry(&path, &first).unwrap();
        append_entry(&path, &second).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with(&first));
        assert!(content.ends_with(&second));
    }

    #[test]
    fn app_identifier_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"].as_str(), Some(APP_IDENTIFIER));
    }

    #[test]
    fn default_log_path_ends_with_logs_panic_log() {
        let path = default_log_path();
        assert!(path.ends_with(Path::new("logs").join("panic.log")));
    }

    /// The only test that installs the global hook (installation is
    /// process-wide and one-shot), so it covers both idempotent install and
    /// the real-panic recording path. Routed to a temp file so tests never
    /// write into the real app data directory.
    #[test]
    fn installed_hook_records_real_panic_and_propagates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("panic.log");
        install_at(path.clone());
        install_at(dir.path().join("ignored.log"));

        let panic_line = line!() + 1;
        let result = std::panic::catch_unwind(|| panic!("panic-log test marker"));
        assert!(result.is_err(), "panic must still propagate");

        let content = fs::read_to_string(&path).unwrap();
        assert!(!dir.path().join("ignored.log").exists());
        assert!(content.contains("message: panic-log test marker\n"));
        assert!(content.contains(&format!("panic_log.rs:{}:", panic_line)));
        let backtrace = content.split("backtrace:\n").nth(1).unwrap();
        assert!(backtrace.lines().any(|l| !l.trim().is_empty()));
    }
}
