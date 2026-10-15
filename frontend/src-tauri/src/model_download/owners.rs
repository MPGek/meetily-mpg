//! Per-model download ownership and cancellation (design D1).
//!
//! A download reserves an owner for its model key. Only that download's own
//! worker releases the owner, and only after its cleanup has finished, so a
//! cancel followed by an immediate retry can never produce two writers.

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex, MutexGuard};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// How long a cancel request waits for the worker's cleanup before it
/// reports [`CancelDownloadOutcome::Pending`].
pub const CANCEL_DOWNLOAD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

/// The error a worker returns when its owner was cancelled.
#[derive(Debug, thiserror::Error)]
#[error("Download cancelled by user")]
pub struct DownloadCancelled;

pub fn is_download_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<DownloadCancelled>().is_some()
}

/// Result of a cancel request, serialized as `"cancelled"` / `"pending"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CancelDownloadOutcome {
    /// The download stopped and released its claim (or there was none).
    Cancelled,
    /// The download was told to stop but has not finished cleanup yet.
    Pending,
}

/// One in-flight download's claim on a model key.
pub struct DownloadOwner {
    cancellation: CancellationToken,
    completion: watch::Sender<bool>,
    progress: AtomicU8,
}

impl DownloadOwner {
    fn new() -> Self {
        let (completion, _) = watch::channel(false);
        Self {
            cancellation: CancellationToken::new(),
            completion,
            progress: AtomicU8::new(0),
        }
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub fn set_progress(&self, progress: u8) {
        self.progress.store(progress, Ordering::Relaxed);
    }

    pub fn progress(&self) -> u8 {
        self.progress.load(Ordering::Relaxed)
    }

    /// Wake every waiting cancel request. Call after the owner was released.
    pub fn signal_done(&self) {
        self.completion.send_replace(true);
    }
}

#[derive(Default)]
struct OwnersState {
    downloads: HashMap<String, Arc<DownloadOwner>>,
    revision: u64,
}

/// The registry of download owners for one engine.
#[derive(Default)]
pub struct DownloadOwners {
    state: Mutex<OwnersState>,
}

/// Exclusive access to the owner registry.
pub struct OwnersGuard<'a> {
    state: MutexGuard<'a, OwnersState>,
}

impl OwnersGuard<'_> {
    /// Whether `owner` is still the registered owner of `key`.
    pub fn is_owner(&self, key: &str, owner: &Arc<DownloadOwner>) -> bool {
        matches!(self.state.downloads.get(key), Some(current) if Arc::ptr_eq(current, owner))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.state.downloads.contains_key(key)
    }

    pub fn owner(&self, key: &str) -> Option<Arc<DownloadOwner>> {
        self.state.downloads.get(key).cloned()
    }

    /// Remove the owner of `key` and bump the revision.
    pub fn release(&mut self, key: &str) {
        self.state.downloads.remove(key);
        self.state.revision = self.state.revision.wrapping_add(1);
    }

    /// Changes whenever an owner is reserved or released.
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
}

impl DownloadOwners {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim `key` for a new download.
    pub async fn reserve(&self, key: &str) -> Result<Arc<DownloadOwner>> {
        let mut state = self.state.lock().await;
        if state.downloads.contains_key(key) {
            return Err(anyhow!("Download already in progress for model: {}", key));
        }
        let owner = Arc::new(DownloadOwner::new());
        state.downloads.insert(key.to_string(), Arc::clone(&owner));
        state.revision = state.revision.wrapping_add(1);
        Ok(owner)
    }

    pub async fn lock(&self) -> OwnersGuard<'_> {
        OwnersGuard {
            state: self.state.lock().await,
        }
    }

    /// Cancel the download owning `key` and wait up to `cleanup_timeout` for
    /// its worker to release it. A key with no owner reports `Cancelled`.
    pub async fn cancel_with_timeout(
        &self,
        key: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        let owner = {
            let state = self.state.lock().await;
            let Some(owner) = state.downloads.get(key).cloned() else {
                return Ok(CancelDownloadOutcome::Cancelled);
            };
            owner.cancellation.cancel();
            owner
        };

        let mut completion = owner.completion.subscribe();
        if !*completion.borrow() {
            match timeout(cleanup_timeout, completion.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Download worker ended before completing cancellation cleanup"
                    ));
                }
                Err(_) => return Ok(CancelDownloadOutcome::Pending),
            }
        }

        Ok(CancelDownloadOutcome::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reserve_rejects_a_second_owner() {
        let owners = DownloadOwners::new();
        let _first = owners.reserve("model").await.expect("first reservation");
        let error = owners
            .reserve("model")
            .await
            .err()
            .expect("second reservation must fail");
        assert!(error
            .to_string()
            .contains("Download already in progress for model: model"));
        assert!(owners.reserve("other").await.is_ok());
    }

    #[tokio::test]
    async fn cancel_without_owner_reports_cancelled() {
        let owners = DownloadOwners::new();
        assert_eq!(
            owners
                .cancel_with_timeout("model", Duration::from_millis(1))
                .await
                .expect("cancel"),
            CancelDownloadOutcome::Cancelled
        );
    }

    #[tokio::test]
    async fn cancel_times_out_to_pending_and_keeps_owner() {
        let owners = DownloadOwners::new();
        let owner = owners.reserve("model").await.expect("reserve");
        assert_eq!(
            owners
                .cancel_with_timeout("model", Duration::from_millis(1))
                .await
                .expect("cancel"),
            CancelDownloadOutcome::Pending
        );
        assert!(owner.cancellation().is_cancelled());
        let guard = owners.lock().await;
        assert!(guard.is_owner("model", &owner));
        drop(guard);
        assert!(owners.reserve("model").await.is_err());
    }

    #[tokio::test]
    async fn release_bumps_revision_and_signals_completion() {
        let owners = Arc::new(DownloadOwners::new());
        let owner = owners.reserve("model").await.expect("reserve");
        let revision = owners.lock().await.revision();

        let cancel_owners = Arc::clone(&owners);
        let cancel = tokio::spawn(async move {
            cancel_owners
                .cancel_with_timeout("model", Duration::from_secs(5))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), owner.cancellation().cancelled())
            .await
            .expect("cancel request reaches the owner");

        let mut guard = owners.lock().await;
        guard.release("model");
        assert_ne!(guard.revision(), revision);
        assert!(!guard.contains("model"));
        assert!(guard.owner("model").is_none());
        drop(guard);
        owner.signal_done();

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), cancel)
                .await
                .expect("cancel returns after completion")
                .expect("join cancel task")
                .expect("cancel"),
            CancelDownloadOutcome::Cancelled
        );
        assert!(owners.reserve("model").await.is_ok());
    }
}
