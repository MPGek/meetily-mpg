//! HF multi-file download for alignment models (task 3.3, design D5).
//!
//! Structurally mirrors `ParakeetEngine::download_model_detailed`: weighted
//! byte progress, Range resume, cancellation with partial cleanup, delete.
//! Models live under `app_data_dir/models/alignment/<id>/`.

use super::catalog::{
    model_dir, resolve_status, spec_by_id, AlignmentModelStatus, DEFAULT_ALIGNMENT_MODEL_ID,
};
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Detailed download progress reported to the UI.
#[derive(Debug, Clone)]
pub struct AlignmentDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: u8,
    pub speed_mbps: f64,
}

/// Manages catalogued alignment models: status, download, cancel, delete.
pub struct AlignmentModelManager {
    models_root: PathBuf,
    cancel_flag: Arc<RwLock<Option<String>>>,
    active_downloads: Arc<RwLock<HashSet<String>>>,
}

impl AlignmentModelManager {
    pub fn new(models_root: PathBuf) -> Self {
        Self {
            models_root,
            cancel_flag: Arc::new(RwLock::new(None)),
            active_downloads: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn models_root(&self) -> &Path {
        &self.models_root
    }

    /// Current status of one model id, overriding the filesystem view while a
    /// download is in flight.
    pub async fn status(&self, id: &str) -> AlignmentModelStatus {
        let spec = match spec_by_id(id) {
            Some(s) => s,
            None => return AlignmentModelStatus::Missing,
        };
        if self.active_downloads.read().await.contains(id) {
            return AlignmentModelStatus::Downloading { progress: 0 };
        }
        resolve_status(&model_dir(&self.models_root, id), spec)
    }

    /// Delete a downloaded (or partial) model directory.
    pub async fn delete_model(&self, id: &str) -> Result<()> {
        if self.active_downloads.read().await.contains(id) {
            return Err(anyhow!("Cannot delete while downloading"));
        }
        let dir = model_dir(&self.models_root, id);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .await
                .map_err(|e| anyhow!("Failed to delete model directory: {}", e))?;
            log::info!("Deleted alignment model {} ({})", id, dir.display());
        }
        Ok(())
    }

    /// Download a catalogued model with weighted progress and Range resume.
    pub async fn download_model(
        &self,
        id: &str,
        progress_callback: Option<Box<dyn Fn(AlignmentDownloadProgress) + Send + Sync>>,
    ) -> Result<()> {
        let spec = spec_by_id(id).ok_or_else(|| anyhow!("Unknown alignment model: {}", id))?;

        {
            let mut active = self.active_downloads.write().await;
            if !active.insert(id.to_string()) {
                return Err(anyhow!("Download already in progress for: {}", id));
            }
        }
        *self.cancel_flag.write().await = None;

        let result = self
            .download_model_inner(spec, progress_callback.as_deref())
            .await;

        self.active_downloads.write().await.remove(id);

        if let Err(e) = &result {
            log::warn!("Alignment model download failed for {}: {}", id, e);
        }
        result
    }

    async fn download_model_inner(
        &self,
        spec: &super::catalog::AlignmentModelSpec,
        progress_callback: Option<&(dyn Fn(AlignmentDownloadProgress) + Send + Sync)>,
    ) -> Result<()> {
        let model_dir = model_dir(&self.models_root, spec.id);
        fs::create_dir_all(&model_dir)
            .await
            .map_err(|e| anyhow!("Failed to create model directory: {}", e))?;

        let base_url = format!("https://huggingface.co/{}/resolve/main", spec.hf_repo);
        let total_size_bytes: u64 = spec.files.iter().map(|f| f.expected_bytes).sum();

        let mut already_downloaded: u64 = 0;
        for f in spec.files {
            let path = model_dir.join(f.local);
            let size = fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
            already_downloaded += size.min(f.expected_bytes);
        }

        let mut total_downloaded = already_downloaded;
        let download_start = Instant::now();
        let mut last_report_time = Instant::now();
        let mut bytes_since_last_report: u64 = 0;
        let mut last_reported_progress: u8 = 0;

        let client = reqwest::Client::builder()
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        log::info!(
            "Downloading alignment model {} ({} files, {:.1} MB total, {:.1} MB present)",
            spec.id,
            spec.files.len(),
            total_size_bytes as f64 / 1_048_576.0,
            already_downloaded as f64 / 1_048_576.0
        );

        for (index, spec_file) in spec.files.iter().enumerate() {
            let file_url = format!("{}/{}", base_url, spec_file.remote);
            let local_rel = PathBuf::from(spec_file.local);
            let file_path = model_dir.join(&local_rel);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).await.ok();
            }

            let existing_size = fs::metadata(&file_path).await.map(|m| m.len()).unwrap_or(0);
            if existing_size >= spec_file.min_bytes {
                log::info!(
                    "Skipping complete file {}/{}: {}",
                    index + 1,
                    spec.files.len(),
                    spec_file.local
                );
                continue;
            }

            let mut request = client.get(&file_url);
            if existing_size > 0 {
                request = request.header("Range", format!("bytes={}-", existing_size));
            }

            let response = request
                .send()
                .await
                .map_err(|e| anyhow!("Failed to start download for {}: {}", spec_file.local, e))?;

            let (file_total_size, resuming) = if response.status()
                == reqwest::StatusCode::PARTIAL_CONTENT
            {
                (existing_size + response.content_length().unwrap_or(0), true)
            } else if response.status().is_success() {
                if existing_size > 0 {
                    log::warn!(
                        "Server does not support resume for {}, restarting",
                        spec_file.local
                    );
                }
                (response.content_length().unwrap_or(0), false)
            } else {
                return Err(anyhow!(
                    "Download failed for {} with status: {}",
                    spec_file.local,
                    response.status()
                ));
            };

            let file = if resuming {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&file_path)
                    .await
                    .map_err(|e| anyhow!("Failed to open {} for resume: {}", spec_file.local, e))?
            } else {
                fs::File::create(&file_path)
                    .await
                    .map_err(|e| anyhow!("Failed to create {}: {}", spec_file.local, e))?
            };
            let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);

            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut file_downloaded = if resuming { existing_size } else { 0u64 };

            loop {
                if self.cancel_flag.read().await.as_deref() == Some(spec.id) {
                    let _ = writer.flush().await;
                    drop(writer);
                    // Cancel: partial files are cleaned up by cancel_download().
                    return Err(anyhow!("Download cancelled by user"));
                }

                let next = timeout(Duration::from_secs(30), stream.next()).await;
                let chunk = match next {
                    Err(_) => {
                        let _ = writer.flush().await;
                        return Err(anyhow!(
                            "Download timeout - no data received for 30 seconds ({})",
                            spec_file.local
                        ));
                    }
                    Ok(None) => break,
                    Ok(Some(Ok(c))) => c,
                    Ok(Some(Err(e))) => {
                        let _ = writer.flush().await;
                        return Err(anyhow!(
                            "Download stream error for {}: {}",
                            spec_file.local,
                            e
                        ));
                    }
                };

                writer
                    .write_all(&chunk)
                    .await
                    .map_err(|e| anyhow!("Failed to write {}: {}", spec_file.local, e))?;

                let chunk_len = chunk.len() as u64;
                file_downloaded += chunk_len;
                total_downloaded += chunk_len;
                bytes_since_last_report += chunk_len;

                let overall_progress = if total_size_bytes > 0 {
                    ((total_downloaded as f64 / total_size_bytes as f64) * 100.0).min(99.0) as u8
                } else {
                    0
                };

                let elapsed_since_report = last_report_time.elapsed();
                if overall_progress > last_reported_progress
                    || elapsed_since_report >= Duration::from_millis(500)
                    || file_downloaded >= file_total_size
                {
                    let total_elapsed = download_start.elapsed().as_secs_f64();
                    let speed_mbps = if elapsed_since_report.as_secs_f64() >= 0.1 {
                        (bytes_since_last_report as f64 / (1024.0 * 1024.0))
                            / elapsed_since_report.as_secs_f64()
                    } else if total_elapsed > 0.0 {
                        ((total_downloaded - already_downloaded) as f64 / (1024.0 * 1024.0))
                            / total_elapsed
                    } else {
                        0.0
                    };
                    last_reported_progress = overall_progress;
                    last_report_time = Instant::now();
                    bytes_since_last_report = 0;

                    if let Some(cb) = progress_callback {
                        cb(AlignmentDownloadProgress {
                            downloaded_bytes: total_downloaded,
                            total_bytes: total_size_bytes,
                            percent: overall_progress,
                            speed_mbps,
                        });
                    }
                }
            }

            writer
                .flush()
                .await
                .map_err(|e| anyhow!("Failed to flush {}: {}", spec_file.local, e))?;
            log::info!(
                "Completed download {}/{}: {} ({:.2} MB)",
                index + 1,
                spec.files.len(),
                spec_file.local,
                file_downloaded as f64 / 1_048_576.0
            );
        }

        // Integrity gate: every file must meet its minimum size before the
        // model is reported available.
        let status = resolve_status(&model_dir, spec);
        if status != AlignmentModelStatus::Available {
            return Err(anyhow!(
                "Downloaded model failed integrity check: {:?}",
                status
            ));
        }

        if let Some(cb) = progress_callback {
            let total_elapsed = download_start.elapsed().as_secs_f64().max(0.001);
            cb(AlignmentDownloadProgress {
                downloaded_bytes: total_size_bytes,
                total_bytes: total_size_bytes,
                percent: 100,
                speed_mbps: ((total_size_bytes - already_downloaded) as f64 / (1024.0 * 1024.0))
                    / total_elapsed,
            });
        }
        log::info!("Alignment model {} downloaded successfully", spec.id);
        Ok(())
    }

    /// Cancel an in-flight download and remove all partial files.
    pub async fn cancel_download(&self, id: &str) -> Result<()> {
        if !self.active_downloads.read().await.contains(id) {
            return Ok(());
        }
        log::info!("Cancelling alignment model download: {}", id);
        *self.cancel_flag.write().await = Some(id.to_string());

        // Wait briefly for the download loop to exit, then clean partials.
        for _ in 0..50 {
            if !self.active_downloads.read().await.contains(id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let spec = spec_by_id(id);
        if let (Some(spec), true) = (spec, model_dir(&self.models_root, id).exists()) {
            let dir = model_dir(&self.models_root, id);
            // Remove any file that is not complete; the directory itself goes
            // away once empty.
            for file in spec.files {
                let path = dir.join(file.local);
                let size = fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
                if path.exists() && size < file.min_bytes {
                    let _ = fs::remove_file(&path).await;
                }
            }
            let mut dir_read = match fs::read_dir(&dir).await {
                Ok(r) => r,
                Err(_) => return Ok(()),
            };
            let remaining = dir_read.next_entry().await.ok().flatten().is_some();
            if !remaining {
                let _ = fs::remove_dir_all(&dir).await;
            }
            log::info!("Cleaned up cancelled alignment download for {}", id);
        }
        Ok(())
    }
}

/// Default model id used when no explicit selection exists.
pub fn default_model_id() -> String {
    DEFAULT_ALIGNMENT_MODEL_ID.to_string()
}

/// Convenience map of spec id -> expected sizes (used by tests/UI sizing).
pub fn expected_sizes(id: &str) -> HashMap<&'static str, u64> {
    spec_by_id(id)
        .map(|spec| {
            spec.files
                .iter()
                .map(|f| (f.local, f.expected_bytes))
                .collect()
        })
        .unwrap_or_default()
}
