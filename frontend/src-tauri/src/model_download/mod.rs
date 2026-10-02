//! Shared model-download protocol for the Parakeet, Whisper and word-alignment
//! downloaders (openspec `harden-model-downloads`, design D1).
//!
//! - [`owners`]: per-model download ownership and cancellation. Lifted from
//!   upstream v0.4.1 `parakeet_engine/parakeet_engine.rs` `:147-173`
//!   (`ActiveDownload`, `ActiveDownloadState`, `DownloadCancelled`,
//!   `is_download_cancelled`, `CancelDownloadOutcome`,
//!   `CANCEL_DOWNLOAD_CLEANUP_TIMEOUT`), `:656-672` (`reserve_active_download`)
//!   and `:1196-1229` (`cancel_download_with_timeout`).
//! - [`transfer`]: the exact-size, resumable multi-file transfer. Lifted from
//!   upstream v0.4.1 `parakeet_engine.rs` `:190-230` (`parse_content_range`),
//!   `:681-810` (progress cap, request, response validators), `:849-1131`
//!   (`download_artifacts`) and, for tests, `:1257-1342` (loopback server).
//!
//! Upstream edits to those ranges must be re-applied here by hand.

pub mod owners;
pub mod transfer;

pub use owners::{
    is_download_cancelled, CancelDownloadOutcome, DownloadCancelled, DownloadOwner, DownloadOwners,
    OwnersGuard, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT,
};
