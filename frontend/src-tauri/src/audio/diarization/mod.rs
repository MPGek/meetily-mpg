//! Diarization: every speaker-identification concern for this app lives in
//! this tree (05-unified-diarization-engine).
//!
//! **`engine::DiarizationEngine` is the entry point other modules use.** It
//! owns the live session's state and wraps the offline pass, so nothing
//! outside this tree reaches into the stages below. The re-exports at the
//! bottom of this file exist for the types those calls exchange (and for the
//! Tauri commands `lib.rs` registers), not as an invitation to call a stage
//! directly.
//!
//! Layout:
//!
//! - `engine` — the facade: live session lifecycle, persistence, live
//!   corrections, telemetry snapshot, batch entries
//! - `commands` — the Tauri command surface, thin wrappers over `engine` and
//!   the stages
//! - `config` — the one resolved parameter surface (`DiarizationConfig`) and
//!   the stored clustering overrides
//! - `core/` — the shared stages: `units`, `cluster` (the `Clustering` seam),
//!   `segment` (dense embedding + the chunked core), `turns` (turn assembly),
//!   `timeline` (attribution and the token split), `factory` (pipeline
//!   construction), `fixtures` (test-only constructors)
//! - `batch/` — the offline pass over a saved recording: `guard` (one run at
//!   a time), `pcm` (streaming ffmpeg decode), `chunking`, `orchestrator`
//! - `streaming/` — the live path: `guard`, `units`, `engine` (per-channel
//!   Fast/Efficient engines), `processor`, and `reconcile` (the display-only
//!   word-level split)
//! - `identity/` — `matching` (cosine against enrolled voiceprints) and
//!   `prototypes` (the live prototype store)
//! - `persist/` — `clusters` (centroids, exemplar caches, recognition) and
//!   `offline_split` (the N-way transcript row rewrite)
//! - `telemetry` — batch progress events, the peak-memory sampler, and a live
//!   session's per-channel counters and status lines
//!
//! This file holds no logic.

pub mod batch;
pub mod commands;
pub mod config;
pub mod core;
pub mod engine;
pub mod identity;
pub mod persist;
pub mod streaming;
pub mod telemetry;

pub use batch::*;
pub use commands::*;
pub use config::*;
pub use core::cluster::*;
pub use core::factory::*;
pub use core::units::*;
pub use core::*;
pub use engine::DiarizationEngine;
pub use persist::*;
