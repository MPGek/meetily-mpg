//! HF multi-file download for alignment models (task 3.3, design D5;
//! hardened by openspec `harden-model-downloads` group 3).
//!
//! Uses the shared protocol in `crate::model_download`: a per-model
//! `DownloadOwners` claim (released only by the worker, after cleanup) and the
//! exact-size resumable `transfer::download_artifacts` (skip only on exact
//! size, validated `Range` resume, partials kept on cancel or error).
//! Models live under `app_data_dir/models/alignment/<id>/`.

use super::catalog::{
    model_dir, resolve_status, spec_by_id, AlignmentModelSpec, AlignmentModelStatus,
    DEFAULT_ALIGNMENT_MODEL_ID,
};
use crate::model_download::transfer::{self, TransferProgress};
use crate::model_download::{
    is_download_cancelled, CancelDownloadOutcome, DownloadCancelled, DownloadOwner,
    DownloadOwners, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT,
};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;

/// Detailed download progress reported to the UI.
#[derive(Debug, Clone)]
pub struct AlignmentDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: u8,
    pub speed_mbps: f64,
}

impl From<TransferProgress> for AlignmentDownloadProgress {
    fn from(progress: TransferProgress) -> Self {
        Self {
            downloaded_bytes: progress.confirmed_bytes,
            total_bytes: progress.total_bytes,
            percent: progress.percent,
            speed_mbps: progress.speed_mbps,
        }
    }
}

type ProgressCallback = Box<dyn Fn(AlignmentDownloadProgress) + Send + Sync>;

/// Pinned Hugging Face base URL for a catalogued model.
fn source_base_url(spec: &AlignmentModelSpec) -> String {
    format!(
        "https://huggingface.co/{}/resolve/{}",
        spec.hf_repo, spec.revision
    )
}

/// Manages catalogued alignment models: status, download, cancel, delete.
pub struct AlignmentModelManager {
    models_root: PathBuf,
    downloads: DownloadOwners,
}

impl AlignmentModelManager {
    pub fn new(models_root: PathBuf) -> Self {
        Self {
            models_root,
            downloads: DownloadOwners::new(),
        }
    }

    pub fn models_root(&self) -> &Path {
        &self.models_root
    }

    /// Current status of one model id, overriding the filesystem view with
    /// the owner's progress while a download is in flight.
    pub async fn status(&self, id: &str) -> AlignmentModelStatus {
        let spec = match spec_by_id(id) {
            Some(s) => s,
            None => return AlignmentModelStatus::Missing,
        };
        if let Some(owner) = self.downloads.lock().await.owner(id) {
            return AlignmentModelStatus::Downloading {
                progress: owner.progress(),
            };
        }
        resolve_status(&model_dir(&self.models_root, id), spec)
    }

    /// Delete a downloaded (or partial) model directory. Rejected while a
    /// download owns the model; the owners lock is held across the removal so
    /// no download can start in between.
    pub async fn delete_model(&self, id: &str) -> Result<()> {
        let downloads = self.downloads.lock().await;
        if downloads.contains(id) {
            return Err(anyhow!("Cannot delete while downloading"));
        }
        let dir = model_dir(&self.models_root, id);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .await
                .map_err(|e| anyhow!("Failed to delete model directory: {}", e))?;
            log::info!("Deleted alignment model {} ({})", id, dir.display());
        }
        drop(downloads);
        Ok(())
    }

    /// Download a catalogued model with weighted progress and validated resume.
    pub async fn download_model(
        &self,
        id: &str,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<()> {
        let spec = spec_by_id(id).ok_or_else(|| anyhow!("Unknown alignment model: {}", id))?;
        self.download_spec_from_source(spec, &source_base_url(spec), progress_callback)
            .await
    }

    /// Reserve the owner, run the shared transfer and the integrity gate, then
    /// release the owner. Partial files are never deleted here.
    async fn download_spec_from_source(
        &self,
        spec: &AlignmentModelSpec,
        base_url: &str,
        progress_callback: Option<ProgressCallback>,
    ) -> Result<()> {
        let client = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        let owner = self.downloads.reserve(spec.id).await?;
        log::info!(
            "Downloading alignment model {} ({} files) from {}",
            spec.id,
            spec.files.len(),
            base_url
        );

        let result = self
            .download_with_owner(&client, spec, base_url, &owner, progress_callback.as_deref())
            .await;

        // Release only after the worker is done with the files.
        let cancellation_won = owner.cancellation().is_cancelled()
            || result.as_ref().err().is_some_and(is_download_cancelled);
        {
            let mut downloads = self.downloads.lock().await;
            if downloads.is_owner(spec.id, &owner) {
                downloads.release(spec.id);
            }
        }
        owner.signal_done();

        if cancellation_won {
            log::info!("Alignment model download cancelled: {}", spec.id);
            return Err(DownloadCancelled.into());
        }
        match result {
            Ok(final_progress) => {
                if let Some(cb) = progress_callback.as_deref() {
                    cb(final_progress.into());
                }
                log::info!("Alignment model {} downloaded successfully", spec.id);
                Ok(())
            }
            Err(e) => {
                log::warn!("Alignment model download failed for {}: {}", spec.id, e);
                Err(e)
            }
        }
    }

    async fn download_with_owner(
        &self,
        client: &reqwest::Client,
        spec: &AlignmentModelSpec,
        base_url: &str,
        owner: &DownloadOwner,
        progress_callback: Option<&(dyn Fn(AlignmentDownloadProgress) + Send + Sync)>,
    ) -> Result<TransferProgress> {
        let model_dir = model_dir(&self.models_root, spec.id);
        let mut on_progress = |progress: TransferProgress| {
            if let Some(cb) = progress_callback {
                cb(progress.into());
            }
        };
        let final_progress = transfer::download_artifacts(
            client,
            base_url,
            &model_dir,
            spec.files,
            owner,
            &mut on_progress,
        )
        .await?;

        // Integrity gate: every file must have exactly its catalogued size
        // before the model is reported available.
        let status = resolve_status(&model_dir, spec);
        if status != AlignmentModelStatus::Available {
            return Err(anyhow!(
                "Downloaded model failed integrity check: {:?}",
                status
            ));
        }
        Ok(final_progress)
    }

    /// Cancel an in-flight download. Partial files are kept for resume.
    /// Returns `Pending` if the worker has not finished cleanup within 5 s.
    pub async fn cancel_download(&self, id: &str) -> Result<CancelDownloadOutcome> {
        self.cancel_download_with_timeout(id, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)
            .await
    }

    async fn cancel_download_with_timeout(
        &self,
        id: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        log::info!("Cancelling alignment model download: {}", id);
        self.downloads.cancel_with_timeout(id, cleanup_timeout).await
    }
}

/// Default model id used when no explicit selection exists.
pub fn default_model_id() -> String {
    DEFAULT_ALIGNMENT_MODEL_ID.to_string()
}

/// Convenience map of spec id -> exact sizes (used by tests/UI sizing).
pub fn expected_sizes(id: &str) -> HashMap<&'static str, u64> {
    spec_by_id(id)
        .map(|spec| {
            spec.files
                .iter()
                .map(|f| (f.local, f.exact_bytes))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::model_download::transfer::test_server::{response, serve_requests, ExpectedResponse};
    use crate::model_download::transfer::ArtifactSpec;
    use tokio::sync::oneshot;

    const TEST_ID: &str = "alignment-test";
    static TEST_SPEC: AlignmentModelSpec = AlignmentModelSpec {
        id: TEST_ID,
        name: "test",
        hf_repo: "test/repo",
        revision: "0000000000000000000000000000000000000000",
        size_mb: 1,
        languages: "test",
        description: "test",
        files: &[
            ArtifactSpec {
                remote: "onnx/model.bin",
                local: "model.bin",
                exact_bytes: 4,
            },
            ArtifactSpec::same("config.json", 3),
        ],
    };

    fn test_manager() -> (tempfile::TempDir, Arc<AlignmentModelManager>, PathBuf) {
        let temp_dir = tempfile::tempdir().expect("create temporary models root");
        let manager = Arc::new(AlignmentModelManager::new(temp_dir.path().to_path_buf()));
        let dir = model_dir(temp_dir.path(), TEST_ID);
        (temp_dir, manager, dir)
    }

    #[tokio::test]
    async fn completed_file_is_skipped_and_partial_resumes_with_validated_range() {
        let (_temp_dir, manager, dir) = test_manager();
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("model.bin"), b"ABCD").await.unwrap();
        fs::write(dir.join("config.json"), b"X").await.unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let callback_events = Arc::clone(&events);

        // Only the partial file is requested; the complete one is skipped.
        let (base_url, server) = serve_requests(vec![response(
            "config.json",
            Some("bytes=1-"),
            "206 Partial Content",
            b"YZ",
            Some("bytes 1-2/3"),
        )])
        .await;
        manager
            .download_spec_from_source(
                &TEST_SPEC,
                &base_url,
                Some(Box::new(move |progress| {
                    callback_events.lock().unwrap().push(progress.percent);
                })),
            )
            .await
            .expect("resume completes the model");
        server.await.expect("join resume server");

        assert_eq!(fs::read(dir.join("model.bin")).await.unwrap(), b"ABCD");
        assert_eq!(fs::read(dir.join("config.json")).await.unwrap(), b"XYZ");
        assert_eq!(resolve_status(&dir, &TEST_SPEC), AlignmentModelStatus::Available);
        assert!(!manager.downloads.lock().await.contains(TEST_ID));
        let events = events.lock().unwrap();
        assert_eq!(events.last().copied(), Some(100));
        assert!(events[..events.len() - 1].iter().all(|percent| *percent < 100));
    }

    #[tokio::test]
    async fn mismatched_content_range_fails_without_publishing_available() {
        let (_temp_dir, manager, dir) = test_manager();
        fs::create_dir_all(&dir).await.unwrap();
        fs::write(dir.join("model.bin"), b"AB").await.unwrap();

        // The remote path differs from the local name; the 206 starts at the wrong byte.
        let (base_url, server) = serve_requests(vec![ExpectedResponse {
            filename: "onnx/model.bin",
            range: Some("bytes=2-"),
            status: "206 Partial Content",
            content_length: Some(3),
            content_range: Some("bytes 1-3/4"),
            body: b"BCD",
            release_after_body: None,
        }])
        .await;
        let error = manager
            .download_spec_from_source(&TEST_SPEC, &base_url, None)
            .await
            .expect_err("a mismatched Content-Range must fail");
        server.await.expect("join mismatch server");

        assert!(!is_download_cancelled(&error));
        assert!(error.to_string().contains("does not match"), "{error}");
        assert_eq!(fs::read(dir.join("model.bin")).await.unwrap(), b"AB");
        assert_ne!(manager.status(TEST_ID).await, AlignmentModelStatus::Available);
        assert_ne!(resolve_status(&dir, &TEST_SPEC), AlignmentModelStatus::Available);
        assert!(!manager.downloads.lock().await.contains(TEST_ID));
    }

    #[tokio::test]
    async fn cancel_keeps_partials_and_releases_owner() {
        let (_temp_dir, manager, dir) = test_manager();
        let (release_tx, release_rx) = oneshot::channel();
        let (progress_tx, progress_rx) = oneshot::channel();
        let progress_tx = Arc::new(std::sync::Mutex::new(Some(progress_tx)));
        let (base_url, server) = serve_requests(vec![ExpectedResponse {
            filename: "onnx/model.bin",
            range: None,
            status: "200 OK",
            content_length: Some(4),
            content_range: None,
            body: b"AB",
            release_after_body: Some(release_rx),
        }])
        .await;

        let download_manager = Arc::clone(&manager);
        let download = tokio::spawn(async move {
            download_manager
                .download_spec_from_source(
                    &TEST_SPEC,
                    &base_url,
                    Some(Box::new(move |progress| {
                        if progress.downloaded_bytes == 2 {
                            if let Some(sender) = progress_tx.lock().unwrap().take() {
                                let _ = sender.send(());
                            }
                        }
                    })),
                )
                .await
        });

        tokio::time::timeout(Duration::from_secs(5), progress_rx)
            .await
            .expect("receive partial progress")
            .expect("progress sender remains connected");
        assert!(manager.downloads.lock().await.contains(TEST_ID));
        assert_eq!(
            manager.cancel_download(TEST_ID).await.expect("cancel"),
            CancelDownloadOutcome::Cancelled
        );
        release_tx.send(()).expect("release partial response");
        let error = download
            .await
            .expect("join cancelled download")
            .expect_err("cancelled download must not succeed");
        server.await.expect("join partial-response server");

        assert!(is_download_cancelled(&error));
        assert_eq!(fs::read(dir.join("model.bin")).await.unwrap(), b"AB");
        assert!(!manager.downloads.lock().await.contains(TEST_ID));
        assert_ne!(resolve_status(&dir, &TEST_SPEC), AlignmentModelStatus::Available);
        // A retry can reserve the model again once the cancel has returned.
        assert!(manager.downloads.reserve(TEST_ID).await.is_ok());
    }
}
