//! Word-alignment settings holder (task 6.1).
//!
//! The backend needs `wordAlignmentEnabled` (default **on**) and
//! `alignmentModelId` at three call sites: the live transcription task, the
//! stop-time finalize repair, and offline re-diarization. Rather than thread
//! them through every entry point, they live in process globals set by the
//! frontend (which persists them in the settings store) via the
//! `set_word_alignment_settings` command, mirroring `LANGUAGE_PREFERENCE`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use super::catalog::DEFAULT_ALIGNMENT_MODEL_ID;
use super::refine::AlignmentSettings;

static ENABLED: AtomicBool = AtomicBool::new(true); // default on (D7)
static MODEL_ID: Mutex<Option<String>> = Mutex::new(None);
static MODELS_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Set the models root (parent of `alignment/`). Idempotent: first call wins,
/// matching the Parakeet/Whisper `set_models_directory` pattern.
pub fn set_models_root(path: PathBuf) {
    let _ = MODELS_ROOT.set(path);
}

fn models_root() -> PathBuf {
    MODELS_ROOT
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("models"))
}

/// Update the effective settings from the frontend store.
pub fn set_settings(enabled: bool, model_id: Option<String>) {
    ENABLED.store(enabled, Ordering::SeqCst);
    *MODEL_ID.lock().unwrap() = model_id;
    log::info!(
        "Word alignment settings updated: enabled={}, model={:?}",
        enabled,
        MODEL_ID.lock().unwrap().as_deref()
    );
}

/// Snapshot of the current settings for `refine_segment_tokens`.
pub fn current() -> AlignmentSettings {
    let model_id = MODEL_ID
        .lock()
        .unwrap()
        .clone()
        .or_else(|| Some(DEFAULT_ALIGNMENT_MODEL_ID.to_string()));
    AlignmentSettings {
        enabled: ENABLED.load(Ordering::SeqCst),
        model_id,
        models_root: models_root(),
    }
}

/// Cheap enabled check for the worker hot path (skip queueing when off).
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Build settings with an explicit models root (tests / repair callers that
/// already resolved the directory).
pub fn with_models_root(root: PathBuf) -> AlignmentSettings {
    AlignmentSettings {
        enabled: ENABLED.load(Ordering::SeqCst),
        model_id: MODEL_ID
            .lock()
            .unwrap()
            .clone()
            .or_else(|| Some(DEFAULT_ALIGNMENT_MODEL_ID.to_string())),
        models_root: root,
    }
}
