use chrono::{DateTime, Local};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub const DEBUG: bool = true;

static LLM_ITERATION_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn reset_iteration_counter() {
    LLM_ITERATION_COUNTER.store(0, Ordering::SeqCst);
}

pub fn next_iteration() -> u64 {
    LLM_ITERATION_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Serialize)]
pub struct DebugLogEntry {
    pub start_timestamp: String,
    pub provider: String,
    pub model: String,
    pub request_json: serde_json::Value,
    pub iteration: u64,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DebugLogResult {
    Success {
        end_timestamp: String,
        elapsed_secs: f64,
        status_code: u16,
        response_body: String,
    },
    Error {
        end_timestamp: String,
        elapsed_secs: f64,
        error_message: String,
        partial_response: Option<String>,
    },
}

pub fn debug_log_path(folder: &Path, start_time: &DateTime<Local>, iteration: u64) -> PathBuf {
    let ts = start_time.format("%Y%m%d_%H%M%S");
    folder.join(format!("{ts}_it_{iteration}.log"))
}

pub fn write_debug_log(
    log_dir: &Path,
    entry: &DebugLogEntry,
    result: &DebugLogResult,
) {
    if !DEBUG {
        return;
    }

    let start_time: DateTime<Local> = match entry.start_timestamp.parse() {
        Ok(ts) => ts,
        Err(_) => return,
    };

    let log_payload = serde_json::json!({
        "request": entry,
        "response": result,
    });

    let file_path = debug_log_path(log_dir, &start_time, entry.iteration);

    if let Ok(json) = serde_json::to_string_pretty(&log_payload) {
        let _ = std::fs::write(&file_path, json);
    }
}

/// Convenience to build the elapsed time helper
pub fn elapsed_secs(start: &Instant) -> f64 {
    start.elapsed().as_secs_f64()
}

pub fn iso_timestamp() -> String {
    Local::now().to_rfc3339()
}
