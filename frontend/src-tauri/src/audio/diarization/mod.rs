//! Diarization: every speaker-identification concern for this app lives in
//! this tree (05-unified-diarization-engine).
//!
//! Layout:
//!   - `config`     — the one resolved parameter surface (`DiarizationConfig`)
//!   - `core/`      — units, clustering, segmentation and dense embedding,
//!                    turn assembly, and speaker attribution over a timeline
//!   - `batch/`     — the offline pass over a saved recording: single-run
//!                    guard, streaming PCM decode, chunking, orchestration
//!   - `streaming/` — live diarization: online processor, engines, and the
//!                    display-only word-level reconcile stage
//!   - `identity/`  — cosine matching against enrolled voiceprints, live
//!                    prototype store
//!   - `persist/`   — cluster centroids, exemplar caches, auto-recognition
//!   - `telemetry`  — progress events and the peak-memory sampler
//!   - `commands`   — the Tauri command surface
//!
//! This file holds no logic: public items are re-exported here so callers
//! outside the tree keep their existing import paths.

pub mod batch;
pub mod commands;
pub mod config;
pub mod core;
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
pub use persist::*;
