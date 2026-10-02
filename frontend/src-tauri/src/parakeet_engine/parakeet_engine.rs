use crate::model_download::transfer::{self, ArtifactSpec, TransferProgress};
use crate::model_download::{
    is_download_cancelled, CancelDownloadOutcome, DownloadCancelled, DownloadOwner,
    DownloadOwners, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT,
};
use crate::parakeet_engine::model::ParakeetModel;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::sync::{Mutex, RwLock};

/// Quantization type for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub enum QuantizationType {
    FP32, // Full precision
    #[default]
    Int8, // 8-bit integer quantization (faster)
}


/// Model status for Parakeet models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Missing,
    Downloading {
        progress: u8,
    },
    Error(String),
    Corrupted {
        file_size: u64,
        expected_min_size: u64,
    },
}

/// Detailed download progress info (MB-based with speed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
    /// Total file size in bytes
    pub total_bytes: u64,
    /// Downloaded in MB (for display)
    pub downloaded_mb: f64,
    /// Total size in MB (for display)
    pub total_mb: f64,
    /// Download speed in MB/s
    pub speed_mbps: f64,
    /// Percentage complete (0-100)
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64, speed_mbps: f64) -> Self {
        let percent = if total > 0 {
            ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u8
        } else {
            0
        };
        Self {
            downloaded_bytes: downloaded,
            total_bytes: total,
            downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
            total_mb: total as f64 / (1024.0 * 1024.0),
            speed_mbps,
            percent,
        }
    }
}

/// Information about a Parakeet model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u32,
    pub quantization: QuantizationType,
    pub speed: String, // Performance description
    pub status: ModelStatus,
    pub description: String,
}

#[derive(Debug)]
pub enum ParakeetEngineError {
    ModelNotLoaded,
    ModelNotFound(String),
    TranscriptionFailed(String),
    DownloadFailed(String),
    IoError(std::io::Error),
    Other(String),
}

impl std::fmt::Display for ParakeetEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParakeetEngineError::ModelNotLoaded => write!(f, "No Parakeet model loaded"),
            ParakeetEngineError::ModelNotFound(name) => write!(f, "Model '{}' not found", name),
            ParakeetEngineError::TranscriptionFailed(err) => {
                write!(f, "Transcription failed: {}", err)
            }
            ParakeetEngineError::DownloadFailed(err) => write!(f, "Download failed: {}", err),
            ParakeetEngineError::IoError(err) => write!(f, "IO error: {}", err),
            ParakeetEngineError::Other(err) => write!(f, "Error: {}", err),
        }
    }
}

impl std::error::Error for ParakeetEngineError {}

impl From<std::io::Error> for ParakeetEngineError {
    fn from(err: std::io::Error) -> Self {
        ParakeetEngineError::IoError(err)
    }
}

struct ModelSpec {
    name: &'static str,
    size_mb: u32,
    quantization: QuantizationType,
    speed: &'static str,
    description: &'static str,
    source_base_url: &'static str,
    artifacts: &'static [ArtifactSpec],
}

impl ModelSpec {
    fn exact_bytes(&self) -> u64 {
        self.artifacts.iter().map(|artifact| artifact.exact_bytes).sum()
    }
}

const PARAKEET_V3_ARTIFACTS: &[ArtifactSpec] = &[
    ArtifactSpec::same("encoder-model.int8.onnx", 652_183_999),
    ArtifactSpec::same("decoder_joint-model.int8.onnx", 18_202_004),
    ArtifactSpec::same("nemo128.onnx", 139_764),
    ArtifactSpec::same("vocab.txt", 93_939),
];

const PARAKEET_V2_ARTIFACTS: &[ArtifactSpec] = &[
    ArtifactSpec::same("encoder-model.int8.onnx", 652_184_014),
    ArtifactSpec::same("decoder_joint-model.int8.onnx", 8_998_286),
    ArtifactSpec::same("nemo128.onnx", 139_764),
    ArtifactSpec::same("vocab.txt", 9_384),
];

const PARAKEET_MODEL_SPECS: &[ModelSpec] = &[
    ModelSpec {
        name: "parakeet-tdt-0.6b-v3-int8",
        size_mb: 670,
        quantization: QuantizationType::Int8,
        speed: "Ultra Fast (v3)",
        description: "Real time on M4 Max, latest version with int8 quantization",
        source_base_url:
            "https://meetily.towardsgeneralintelligence.com/models/parakeet-tdt-0.6b-v3-onnx",
        artifacts: PARAKEET_V3_ARTIFACTS,
    },
    ModelSpec {
        name: "parakeet-tdt-0.6b-v2-int8",
        size_mb: 661,
        quantization: QuantizationType::Int8,
        speed: "Fast (v2)",
        description: "Previous version with int8 quantization, good balance of speed and accuracy",
        source_base_url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85",
        artifacts: PARAKEET_V2_ARTIFACTS,
    },
];

fn find_model_spec(model_name: &str) -> Option<&'static ModelSpec> {
    PARAKEET_MODEL_SPECS
        .iter()
        .find(|spec| spec.name == model_name)
}

#[cfg(test)]
struct DownloadStateTestHook {
    finalization_ready: tokio::sync::Notify,
    continue_finalization: tokio::sync::Notify,
    discovery_scanned: tokio::sync::Notify,
    continue_discovery: tokio::sync::Notify,
}

#[cfg(test)]
struct ModelLifecycleTestHook {
    load_started: tokio::sync::Notify,
    continue_load: tokio::sync::Notify,
    unload_attempted: tokio::sync::Notify,
}

fn to_download_progress(progress: TransferProgress) -> DownloadProgress {
    let mut detailed = DownloadProgress::new(
        progress.confirmed_bytes,
        progress.total_bytes,
        progress.speed_mbps,
    );
    detailed.percent = progress.percent;
    detailed
}

pub struct ParakeetEngine {
    models_dir: PathBuf,
    current_model: Arc<RwLock<Option<ParakeetModel>>>,
    current_model_name: Arc<RwLock<Option<String>>>,
    model_lifecycle_lock: Mutex<()>,
    pub(crate) available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    // Per-model download owners; a claim is released only by its own worker after cleanup.
    downloads: DownloadOwners,
    #[cfg(test)]
    download_state_test_hook: Mutex<Option<Arc<DownloadStateTestHook>>>,
    #[cfg(test)]
    model_lifecycle_test_hook: Mutex<Option<Arc<ModelLifecycleTestHook>>>,
}

impl ParakeetEngine {
    /// Create a new Parakeet engine with optional custom models directory
    pub fn new_with_models_dir(models_dir: Option<PathBuf>) -> Result<Self> {
        let models_dir = if let Some(dir) = models_dir {
            dir.join("parakeet") // Parakeet models in subdirectory
        } else {
            // Fallback to default location
            let current_dir = std::env::current_dir()
                .map_err(|e| anyhow!("Failed to get current directory: {}", e))?;

            if cfg!(debug_assertions) {
                // Development mode
                current_dir.join("models").join("parakeet")
            } else {
                // Production mode
                dirs::data_dir()
                    .or_else(dirs::home_dir)
                    .ok_or_else(|| anyhow!("Could not find system data directory"))?
                    .join("Meetily")
                    .join("models")
                    .join("parakeet")
            }
        };

        log::info!(
            "ParakeetEngine using models directory: {}",
            models_dir.display()
        );

        // Create directory if it doesn't exist
        if !models_dir.exists() {
            std::fs::create_dir_all(&models_dir)?;
        }

        Ok(Self {
            models_dir,
            current_model: Arc::new(RwLock::new(None)),
            current_model_name: Arc::new(RwLock::new(None)),
            model_lifecycle_lock: Mutex::new(()),
            available_models: Arc::new(RwLock::new(HashMap::new())),
            downloads: DownloadOwners::new(),
            #[cfg(test)]
            download_state_test_hook: Mutex::new(None),
            #[cfg(test)]
            model_lifecycle_test_hook: Mutex::new(None),
        })
    }

    #[cfg(test)]
    async fn test_hook(&self) -> Option<Arc<DownloadStateTestHook>> {
        self.download_state_test_hook.lock().await.clone()
    }

    #[cfg(test)]
    async fn load_test_hook(&self) -> Option<Arc<ModelLifecycleTestHook>> {
        self.model_lifecycle_test_hook.lock().await.clone()
    }

    /// Discover available Parakeet models
    pub async fn discover_models(&self) -> Result<Vec<ModelInfo>> {
        self.discover_models_from_specs(PARAKEET_MODEL_SPECS).await
    }

    /// Scan disk without locks, then commit the result unless a download was
    /// reserved or released meanwhile (revision changed), in which case rescan.
    /// Owned models are reported as Downloading with the owner's progress.
    async fn discover_models_from_specs(&self, specs: &[ModelSpec]) -> Result<Vec<ModelInfo>> {
        loop {
            let revision = self.downloads.lock().await.revision();
            let mut models = Vec::with_capacity(specs.len());
            let mut validation_errors = Vec::new();

            for spec in specs {
                let model_path = self.models_dir.join(spec.name);
                let status = if model_path.exists() {
                    match Self::validate_model_directory(&model_path, spec.artifacts) {
                        Ok(()) => ModelStatus::Available,
                        Err(error) => {
                            let file_size = spec
                                .artifacts
                                .iter()
                                .filter_map(|artifact| {
                                    std::fs::metadata(model_path.join(artifact.local)).ok()
                                })
                                .map(|metadata| metadata.len())
                                .sum();
                            validation_errors.push((spec.name, error));
                            ModelStatus::Corrupted {
                                file_size,
                                expected_min_size: spec.exact_bytes(),
                            }
                        }
                    }
                } else {
                    ModelStatus::Missing
                };

                models.push(ModelInfo {
                    name: spec.name.to_string(),
                    path: model_path,
                    size_mb: spec.size_mb,
                    quantization: spec.quantization.clone(),
                    speed: spec.speed.to_string(),
                    status,
                    description: spec.description.to_string(),
                });
            }

            #[cfg(test)]
            if let Some(hook) = self.test_hook().await {
                hook.discovery_scanned.notify_one();
                hook.continue_discovery.notified().await;
            }

            let downloads = self.downloads.lock().await;
            if downloads.revision() != revision {
                continue;
            }

            validation_errors.retain(|(model_name, _)| !downloads.contains(model_name));
            for model in &mut models {
                if let Some(owner) = downloads.owner(&model.name) {
                    model.status = ModelStatus::Downloading {
                        progress: owner.progress(),
                    };
                }
            }

            let mut available_models = self.available_models.write().await;
            available_models.clear();
            for model in &models {
                available_models.insert(model.name.clone(), model.clone());
            }
            drop(available_models);
            drop(downloads);

            for (model_name, error) in validation_errors {
                log::warn!("Model directory {} appears corrupted: {}", model_name, error);
            }
            return Ok(models);
        }
    }

    /// Validate a model directory: every artifact must exist with exactly its catalogued size.
    fn validate_model_directory(model_dir: &Path, artifacts: &[ArtifactSpec]) -> Result<()> {
        for artifact in artifacts {
            let path = model_dir.join(artifact.local);
            let metadata = std::fs::metadata(&path)
                .map_err(|error| anyhow!("Failed to read {} metadata: {}", artifact.local, error))?;
            if metadata.len() != artifact.exact_bytes {
                return Err(anyhow!(
                    "{} has {} bytes, expected exactly {} bytes",
                    artifact.local,
                    metadata.len(),
                    artifact.exact_bytes
                ));
            }
        }

        Ok(())
    }

    /// Load a Parakeet model
    pub async fn load_model(&self, model_name: &str) -> Result<()> {
        // Clone the entry and release the catalog lock before the native load.
        let model_info = {
            let models = self.available_models.read().await;
            models
                .get(model_name)
                .cloned()
                .ok_or_else(|| anyhow!("Model {} not found", model_name))?
        };

        match &model_info.status {
            ModelStatus::Available => {
                let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
                // Check if this model is already loaded
                let current_model = self.current_model_name.read().await.clone();
                if current_model.as_deref() == Some(model_name) {
                    log::info!(
                        "Parakeet model {} is already loaded, skipping reload",
                        model_name
                    );
                    return Ok(());
                }

                if let Some(current_model) = current_model {
                    // Unload current model before loading new one
                    log::info!(
                        "Unloading current Parakeet model '{}' before loading '{}'",
                        current_model,
                        model_name
                    );
                }
                self.unload_model_locked().await;

                log::info!("Loading Parakeet model: {}", model_name);

                // Load model based on quantization type, off the async runtime
                let quantized = model_info.quantization == QuantizationType::Int8;
                let model_path = model_info.path.clone();
                #[cfg(test)]
                let model_lifecycle_test_hook = self.load_test_hook().await;
                #[cfg(test)]
                let runtime_handle = tokio::runtime::Handle::current();
                let model = tokio::task::spawn_blocking(move || {
                    #[cfg(test)]
                    if let Some(hook) = model_lifecycle_test_hook {
                        hook.load_started.notify_one();
                        runtime_handle.block_on(hook.continue_load.notified());
                    }
                    ParakeetModel::new(&model_path, quantized).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| {
                    anyhow!(
                        "Parakeet model load task failed for {}: {}",
                        model_name,
                        error
                    )
                })?
                .map_err(|error| {
                    anyhow!("Failed to load Parakeet model {}: {}", model_name, error)
                })?;

                // Update current model and model name
                *self.current_model.write().await = Some(model);
                *self.current_model_name.write().await = Some(model_name.to_string());

                log::info!(
                    "Successfully loaded Parakeet model: {} ({})",
                    model_name,
                    if quantized { "Int8 quantized" } else { "FP32" }
                );
                Ok(())
            }
            ModelStatus::Missing => Err(anyhow!("Parakeet model {} is not downloaded", model_name)),
            ModelStatus::Downloading { .. } => Err(anyhow!(
                "Parakeet model {} is currently downloading",
                model_name
            )),
            ModelStatus::Error(err) => {
                Err(anyhow!("Parakeet model {} has error: {}", model_name, err))
            }
            ModelStatus::Corrupted { .. } => Err(anyhow!(
                "Parakeet model {} is corrupted and cannot be loaded",
                model_name
            )),
        }
    }

    /// Unload the current model. Waits for any in-flight load to finish first.
    pub async fn unload_model(&self) -> bool {
        #[cfg(test)]
        if let Some(hook) = self.load_test_hook().await {
            hook.unload_attempted.notify_one();
        }
        let _lifecycle_guard = self.model_lifecycle_lock.lock().await;
        self.unload_model_locked().await
    }

    async fn unload_model_locked(&self) -> bool {
        let unloaded = self.current_model.write().await.take().is_some();
        if unloaded {
            log::info!("Parakeet model unloaded");
        }
        self.current_model_name.write().await.take();
        unloaded
    }

    /// Get the currently loaded model name
    pub async fn get_current_model(&self) -> Option<String> {
        self.current_model_name.read().await.clone()
    }

    /// Check if a model is loaded
    pub async fn is_model_loaded(&self) -> bool {
        self.current_model.read().await.is_some()
    }

    /// Transcribe audio samples using the loaded Parakeet model
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>) -> Result<String> {
        let (text, _tokens) = self.transcribe_audio_with_tokens(audio_data).await?;
        Ok(text)
    }

    /// Transcribe audio samples and return per-word tokens derived from the
    /// model's native token-frame alignment (word-level-diarization-alignment D1).
    /// Token timestamps are chunk-relative seconds quantized to the encoder
    /// frame granularity; empty text yields an empty token list.
    pub async fn transcribe_audio_with_tokens(
        &self,
        audio_data: Vec<f32>,
    ) -> Result<(String, Vec<crate::audio::token_assignment::Token>)> {
        let mut model_guard = self.current_model.write().await;
        let model = model_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No Parakeet model loaded. Please load a model first."))?;

        let duration_seconds = audio_data.len() as f64 / 16000.0;
        log::info!(
            "Parakeet transcribing {} samples ({:.1}s duration)",
            audio_data.len(),
            duration_seconds
        );

        // Run inference with timeout and panic catch.
        // ORT native code can hang or segfault on corrupted models / bad inputs;
        // Rust panics from ndarray shape mismatches are caught so they don't kill
        // the transcription worker task.
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let inference = catch_unwind(AssertUnwindSafe(|| model.transcribe_samples(audio_data)));

        let result = match inference {
            Ok(Ok(ts)) => ts,
            Ok(Err(e)) => {
                let msg = format!("Parakeet inference failed: {}", e);
                log::error!("{}", msg);
                return Err(anyhow!(msg));
            }
            Err(panic_payload) => {
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic during Parakeet inference".to_string()
                };
                log::error!(
                    "Parakeet model panicked: {}. Unloading corrupted model state.",
                    msg
                );
                drop(model_guard);
                self.unload_model().await;
                return Err(anyhow!(
                    "Parakeet model panicked and has been unloaded: {}",
                    msg
                ));
            }
        };

        log::info!("Parakeet transcription result: '{}'", result.text);

        let tokens = if result.text.trim().is_empty() {
            Vec::new()
        } else {
            crate::parakeet_engine::model::build_word_tokens(&result.tokens, &result.timestamps)
        };

        Ok((result.text, tokens))
    }

    /// Get the models directory path
    pub async fn get_models_directory(&self) -> PathBuf {
        self.models_dir.clone()
    }

    /// Delete a corrupted model
    pub async fn delete_model(&self, model_name: &str) -> Result<String> {
        log::info!("Attempting to delete Parakeet model: {}", model_name);

        // Get model info to find the directory path
        let model_info = {
            let models = self.available_models.read().await;
            models.get(model_name).cloned()
        };

        let model_info =
            model_info.ok_or_else(|| anyhow!("Parakeet model '{}' not found", model_name))?;

        log::info!(
            "Parakeet model '{}' has status: {:?}",
            model_name,
            model_info.status
        );

        // Allow deletion of corrupted or available models
        match &model_info.status {
            ModelStatus::Corrupted { .. } | ModelStatus::Available => {
                // Delete the entire model directory
                if model_info.path.exists() {
                    fs::remove_dir_all(&model_info.path).await
                        .map_err(|e| anyhow!("Failed to delete directory '{}': {}", model_info.path.display(), e))?;
                    log::info!("Successfully deleted Parakeet model directory: {}", model_info.path.display());
                } else {
                    log::warn!("Directory '{}' does not exist, nothing to delete", model_info.path.display());
                }

                // Update model status to Missing
                {
                    let mut models = self.available_models.write().await;
                    if let Some(model) = models.get_mut(model_name) {
                        model.status = ModelStatus::Missing;
                    }
                }

                Ok(format!("Successfully deleted Parakeet model '{}'", model_name))
            }
            _ => {
                Err(anyhow!(
                    "Can only delete corrupted or available Parakeet models. Model '{}' has status: {:?}",
                    model_name,
                    model_info.status
                ))
            }
        }
    }

    /// Download a Parakeet model from HuggingFace (backward-compatible wrapper)
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        // Wrap simple callback to use detailed version
        let detailed_callback: Option<Box<dyn Fn(DownloadProgress) + Send>> = progress_callback
            .map(|cb| {
                Box::new(move |p: DownloadProgress| cb(p.percent))
                    as Box<dyn Fn(DownloadProgress) + Send>
            });
        self.download_model_detailed(model_name, detailed_callback)
            .await
    }

    /// Download a catalogued Parakeet model with detailed progress (MB/speed/resume support)
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for Parakeet model: {}", model_name);

        let model_info = self
            .available_models
            .read()
            .await
            .get(model_name)
            .cloned()
            .ok_or_else(|| anyhow!("Model {} not found", model_name))?;
        let spec = find_model_spec(model_name)
            .ok_or_else(|| anyhow!("Unsupported Parakeet model: {}", model_name))?;

        self.download_model_detailed_from_source(
            model_name,
            &model_info.path,
            spec.source_base_url,
            spec.artifacts,
            progress_callback,
        )
        .await
    }

    async fn set_downloading_status(&self, model_name: &str, progress: u8) {
        let mut models = self.available_models.write().await;
        if let Some(model) = models.get_mut(model_name) {
            model.status = ModelStatus::Downloading { progress };
        }
    }

    /// Reserve the model's owner, run the shared exact-size transfer, then commit.
    async fn download_model_detailed_from_source(
        &self,
        model_name: &str,
        model_dir: &Path,
        base_url: &str,
        artifacts: &[ArtifactSpec],
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        // Optimized HTTP client for large file downloads
        let client = reqwest::Client::builder()
            .tcp_nodelay(true) // Disable Nagle's algorithm for better streaming
            .pool_max_idle_per_host(1) // Keep connection alive
            .timeout(Duration::from_secs(3600)) // 1 hour timeout for large files
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        let owner = self.downloads.reserve(model_name).await?;
        self.set_downloading_status(model_name, 0).await;

        let mut progress_callback = progress_callback;
        let result = {
            let mut on_progress = |progress: TransferProgress| {
                if let Some(callback) = progress_callback.as_mut() {
                    callback(to_download_progress(progress));
                }
            };
            transfer::download_artifacts(
                &client,
                base_url,
                model_dir,
                artifacts,
                &owner,
                &mut on_progress,
            )
            .await
        };

        self.finish_download(
            model_name,
            model_dir,
            artifacts,
            &owner,
            result.map(to_download_progress),
            progress_callback,
        )
        .await
    }

    /// Commit a finished transfer: re-validate exact sizes, then, under the
    /// owners lock and the catalog lock, release this owner and publish
    /// Available or Missing. Partial files are never deleted here. The final
    /// 100% progress is reported only after the commit.
    async fn finish_download(
        &self,
        model_name: &str,
        model_dir: &Path,
        artifacts: &[ArtifactSpec],
        owner: &Arc<DownloadOwner>,
        mut result: Result<DownloadProgress>,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        if result.is_ok() && !owner.cancellation().is_cancelled() {
            let final_progress = result.expect("successful transfer must carry final progress");
            result = Self::validate_model_directory(model_dir, artifacts).map(|()| final_progress);
        }

        #[cfg(test)]
        if let Some(hook) = self.test_hook().await {
            hook.finalization_ready.notify_one();
            hook.continue_finalization.notified().await;
        }

        let mut downloads = self.downloads.lock().await;
        if !downloads.is_owner(model_name, owner) {
            drop(downloads);
            owner.signal_done();
            return result.map(|_| ());
        }

        let cancellation_won = owner.cancellation().is_cancelled()
            || result.as_ref().err().is_some_and(is_download_cancelled);
        let mut models = self.available_models.write().await;
        downloads.release(model_name);
        if let Some(model) = models.get_mut(model_name) {
            if cancellation_won || result.is_err() {
                model.status = ModelStatus::Missing;
            } else {
                model.status = ModelStatus::Available;
                model.path = model_dir.to_path_buf();
            }
        }
        drop(models);
        drop(downloads);
        owner.signal_done();

        if cancellation_won {
            log::info!("Download cancelled for Parakeet model: {}", model_name);
            return Err(DownloadCancelled.into());
        }
        let final_progress = result.inspect_err(|error| {
            log::error!("Download failed for Parakeet model {}: {}", model_name, error);
        })?;
        if let Some(callback) = progress_callback {
            callback(final_progress);
        }
        log::info!("Download completed for Parakeet model: {}", model_name);
        Ok(())
    }

    /// Cancel an ongoing model download. Partial files are kept for resume.
    pub async fn cancel_download(&self, model_name: &str) -> Result<CancelDownloadOutcome> {
        log::info!("Cancelling download for Parakeet model: {}", model_name);
        self.cancel_download_with_timeout(model_name, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)
            .await
    }

    async fn cancel_download_with_timeout(
        &self,
        model_name: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        self.downloads
            .cancel_with_timeout(model_name, cleanup_timeout)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_download::transfer::test_server::{response, serve_requests, ExpectedResponse};
    use crossbeam::queue::SegQueue;
    use tempfile::tempdir;
    use tokio::sync::oneshot;
    const TEST_MODEL_NAME: &str = "parakeet-test";
    const SMALL_ARTIFACTS: &[ArtifactSpec] = &[
        ArtifactSpec::same("encoder.bin", 4),
        ArtifactSpec::same("decoder.bin", 3),
        ArtifactSpec::same("nemo.bin", 2),
        ArtifactSpec::same("vocab.txt", 1),
    ];
    const SMALL_MODEL_SPECS: &[ModelSpec] = &[ModelSpec {
        name: TEST_MODEL_NAME,
        size_mb: 1,
        quantization: QuantizationType::Int8,
        speed: "test",
        description: "test model",
        source_base_url: "",
        artifacts: SMALL_ARTIFACTS,
    }];

    async fn test_engine() -> (tempfile::TempDir, Arc<ParakeetEngine>, PathBuf) {
        let temp_dir = tempdir().expect("create temporary models directory");
        let engine = Arc::new(
            ParakeetEngine::new_with_models_dir(Some(temp_dir.path().to_path_buf()))
                .expect("create Parakeet engine"),
        );
        let model_dir = engine.models_dir.join(TEST_MODEL_NAME);
        engine.available_models.write().await.insert(
            TEST_MODEL_NAME.to_string(),
            ModelInfo {
                name: TEST_MODEL_NAME.to_string(),
                path: model_dir.clone(),
                size_mb: 1,
                quantization: QuantizationType::Int8,
                speed: "test".to_string(),
                status: ModelStatus::Missing,
                description: "test model".to_string(),
            },
        );
        (temp_dir, engine, model_dir)
    }

    async fn test_model_status(engine: &ParakeetEngine) -> ModelStatus {
        engine
            .available_models
            .read()
            .await
            .get(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .status
            .clone()
    }

    #[tokio::test]
    async fn directory_validation_requires_exact_artifact_sizes() {
        let temp_dir = tempdir().expect("create temporary model directory");
        for artifact in SMALL_ARTIFACTS {
            fs::write(
                temp_dir.path().join(artifact.local),
                vec![0; artifact.exact_bytes as usize],
            )
            .await
            .expect("seed exact artifact");
        }
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_ok());

        fs::remove_file(temp_dir.path().join("vocab.txt"))
            .await
            .expect("remove required artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());

        fs::write(temp_dir.path().join("vocab.txt"), [])
            .await
            .expect("restore undersized artifact");
        fs::write(temp_dir.path().join("encoder.bin"), [0; 3])
            .await
            .expect("seed one-byte-short artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());

        fs::write(temp_dir.path().join("encoder.bin"), [0; 5])
            .await
            .expect("seed one-byte-oversized artifact");
        assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());
    }

    #[tokio::test]
    async fn loading_releases_available_models_and_serializes_unload() {
        let (_temp_dir, engine, _model_dir) = test_engine().await;
        engine
            .available_models
            .write()
            .await
            .get_mut(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .status = ModelStatus::Available;
        *engine.current_model_name.write().await = Some("previous-test-model".to_string());

        let hook = Arc::new(ModelLifecycleTestHook {
            load_started: tokio::sync::Notify::new(),
            continue_load: tokio::sync::Notify::new(),
            unload_attempted: tokio::sync::Notify::new(),
        });
        *engine.model_lifecycle_test_hook.lock().await = Some(Arc::clone(&hook));

        let load_engine = Arc::clone(&engine);
        let load = tokio::spawn(async move { load_engine.load_model(TEST_MODEL_NAME).await });
        tokio::time::timeout(Duration::from_secs(1), hook.load_started.notified())
            .await
            .expect("model load must reach its blocking task");

        {
            let mut models = tokio::time::timeout(
                Duration::from_secs(1),
                engine.available_models.write(),
            )
            .await
            .expect("model cache must remain writable during native loading");
            models
                .get_mut(TEST_MODEL_NAME)
                .expect("test model remains registered")
                .description = "updated while native load is paused".to_string();
        }

        let (unloaded_tx, mut unloaded_rx) = oneshot::channel();
        let unload_engine = Arc::clone(&engine);
        let unload = tokio::spawn(async move {
            let unloaded = unload_engine.unload_model().await;
            unloaded_tx
                .send(unloaded)
                .expect("unload receiver remains connected");
        });
        tokio::time::timeout(Duration::from_secs(1), hook.unload_attempted.notified())
            .await
            .expect("unload must reach the lifecycle boundary");
        assert!(matches!(
            unloaded_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        hook.continue_load.notify_one();
        tokio::time::timeout(Duration::from_secs(1), load)
            .await
            .expect("model load must finish after release")
            .expect("join model load task")
            .expect_err("empty model directory must fail loading");
        assert!(
            !tokio::time::timeout(Duration::from_secs(1), unloaded_rx)
                .await
                .expect("unload must finish after model load")
                .expect("unload sender remains connected")
        );
        tokio::time::timeout(Duration::from_secs(1), unload)
            .await
            .expect("unload task must join")
            .expect("join unload task");

        assert_eq!(
            engine
                .available_models
                .read()
                .await
                .get(TEST_MODEL_NAME)
                .expect("test model remains registered")
                .description,
            "updated while native load is paused"
        );
        assert!(engine.current_model.read().await.is_none());
        assert!(engine.current_model_name.read().await.is_none());
        *engine.model_lifecycle_test_hook.lock().await = None;
    }

    #[tokio::test]
    async fn completed_sibling_survives_403_then_retry_resumes_partial() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("encoder.bin"), b"ABCD")
            .await
            .expect("seed completed sibling");
        fs::write(model_dir.join("decoder.bin"), b"X")
            .await
            .expect("seed resumable artifact");

        let (base_url, server) = serve_requests(vec![response(
            "decoder.bin",
            Some("bytes=1-"),
            "403 Forbidden",
            b"",
            None,
        )])
        .await;
        let error = engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                SMALL_ARTIFACTS,
                None,
            )
            .await
            .expect_err("403 must fail without destructive cleanup");
        server.await.expect("join 403 server");
        assert!(error.to_string().contains("403"));
        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"ABCD");
        assert_eq!(fs::read(model_dir.join("decoder.bin")).await.unwrap(), b"X");
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));

        let (base_url, server) = serve_requests(vec![
            response(
                "decoder.bin",
                Some("bytes=1-"),
                "206 Partial Content",
                b"YZ",
                Some("bytes 1-2/3"),
            ),
            response("nemo.bin", None, "200 OK", b"NO", None),
            response("vocab.txt", None, "200 OK", b"V", None),
        ])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                SMALL_ARTIFACTS,
                None,
            )
            .await
            .expect("retry resumes only the partial artifact");
        server.await.expect("join retry server");

        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"ABCD");
        assert_eq!(fs::read(model_dir.join("decoder.bin")).await.unwrap(), b"XYZ");
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
    }

    #[tokio::test]
    async fn cancelled_near_complete_artifact_is_resumed_on_retry() {
        const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec::same("near.bin", 100)];
        const PREFIX: &[u8] = &[b'A'; 99];

        let (_temp_dir, engine, model_dir) = test_engine().await;
        let (release_tx, release_rx) = oneshot::channel();
        let (progress_tx, progress_rx) = oneshot::channel();
        let progress_tx = Arc::new(std::sync::Mutex::new(Some(progress_tx)));
        let (base_url, server) = serve_requests(vec![ExpectedResponse {
            filename: "near.bin",
            range: None,
            status: "200 OK",
            content_length: Some(100),
            content_range: None,
            body: PREFIX,
            release_after_body: Some(release_rx),
        }])
        .await;

        let download_engine = Arc::clone(&engine);
        let download_dir = model_dir.clone();
        let download = tokio::spawn(async move {
            download_engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &download_dir,
                    &base_url,
                    ARTIFACTS,
                    Some(Box::new(move |progress| {
                        if progress.downloaded_bytes == 99 {
                            if let Some(sender) = progress_tx
                                .lock()
                                .expect("lock progress sender")
                                .take()
                            {
                                let _ = sender.send(());
                            }
                        }
                    })),
                )
                .await
        });

        tokio::time::timeout(Duration::from_secs(5), progress_rx)
            .await
            .expect("receive near-complete progress")
            .expect("progress sender remains connected");
        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_secs(5))
                .await
                .expect("cancel near-complete download"),
            CancelDownloadOutcome::Cancelled
        );
        release_tx.send(()).expect("release partial response");
        let error = download
            .await
            .expect("join cancelled download")
            .expect_err("cancelled download must not succeed");
        server.await.expect("join partial-response server");

        assert!(is_download_cancelled(&error));
        assert_eq!(fs::metadata(model_dir.join("near.bin")).await.unwrap().len(), 99);
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Missing));

        let (base_url, server) = serve_requests(vec![response(
            "near.bin",
            Some("bytes=99-"),
            "206 Partial Content",
            b"B",
            Some("bytes 99-99/100"),
        )])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                ARTIFACTS,
                None,
            )
            .await
            .expect("retry resumes the cancelled near-complete artifact");
        server.await.expect("join retry server");

        assert_eq!(fs::metadata(model_dir.join("near.bin")).await.unwrap().len(), 100);
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
    }

    #[tokio::test]
    async fn range_ignored_replaces_partial_with_honest_progress() {
        const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec::same("model.bin", 4)];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("model.bin"), b"zz")
            .await
            .expect("seed partial artifact");
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) = serve_requests(vec![response(
            "model.bin",
            Some("bytes=2-"),
            "200 OK",
            b"ABCD",
            None,
        )])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                ARTIFACTS,
                Some(Box::new(move |progress| {
                    callback_events.push(progress);
                })),
            )
            .await
            .expect("range-ignored response replaces the partial artifact");
        server.await.expect("join range-ignored server");

        let events: Vec<_> = std::iter::from_fn(|| events.pop()).collect();
        assert_eq!(fs::read(model_dir.join("model.bin")).await.unwrap(), b"ABCD");
        assert!(events.iter().all(|progress| progress.downloaded_bytes <= progress.total_bytes));
        assert_eq!(events.last().expect("final event").percent, 100);
        assert!(events[..events.len() - 1].iter().all(|progress| progress.percent < 100));
    }

    #[tokio::test]
    async fn range_416_retries_fresh_with_honest_progress() {
        const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec::same("model.bin", 4)];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("model.bin"), b"zz")
            .await
            .expect("seed partial artifact");
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) = serve_requests(vec![
            response(
                "model.bin",
                Some("bytes=2-"),
                "416 Range Not Satisfiable",
                b"",
                Some("bytes */4"),
            ),
            response("model.bin", None, "200 OK", b"ABCD", None),
        ])
        .await;
        engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                ARTIFACTS,
                Some(Box::new(move |progress| {
                    callback_events.push(progress);
                })),
            )
            .await
            .expect("416 should retry without Range");
        server.await.expect("join 416 server");

        assert_eq!(fs::read(model_dir.join("model.bin")).await.unwrap(), b"ABCD");
        assert!(std::iter::from_fn(|| events.pop())
            .all(|progress| progress.downloaded_bytes <= progress.total_bytes));
    }

    #[tokio::test]
    async fn invalid_or_short_response_never_publishes_available() {
        const ARTIFACTS: &[ArtifactSpec] = &[
            ArtifactSpec::same("complete.bin", 1),
            ArtifactSpec::same("target.bin", 4),
        ];
        let cases = vec![
            (
                "malformed",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(3),
                    content_range: Some("bytes invalid"),
                    body: b"XYZ",
                    release_after_body: None,
                },
            ),
            (
                "wrong-start",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(4),
                    content_range: Some("bytes 0-3/4"),
                    body: b"ABCD",
                    release_after_body: None,
                },
            ),
            (
                "wrong-total",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "206 Partial Content",
                    content_length: Some(3),
                    content_range: Some("bytes 1-3/5"),
                    body: b"XYZ",
                    release_after_body: None,
                },
            ),
            (
                "short",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "200 OK",
                    content_length: Some(4),
                    content_range: None,
                    body: b"ABC",
                    release_after_body: None,
                },
            ),
            (
                "overlong",
                ExpectedResponse {
                    filename: "target.bin",
                    range: Some("bytes=1-"),
                    status: "200 OK",
                    content_length: Some(5),
                    content_range: None,
                    body: b"ABCDE",
                    release_after_body: None,
                },
            ),
        ];

        for (case_name, invalid_response) in cases {
            let (_temp_dir, engine, model_dir) = test_engine().await;
            fs::create_dir_all(&model_dir).await.expect("create model directory");
            fs::write(model_dir.join("complete.bin"), b"C")
                .await
                .expect("seed completed sibling");
            fs::write(model_dir.join("target.bin"), b"Z")
                .await
                .expect("seed retained prefix");

            let (base_url, server) = serve_requests(vec![invalid_response]).await;
            assert!(
                engine
                    .download_model_detailed_from_source(
                        TEST_MODEL_NAME,
                        &model_dir,
                        &base_url,
                        ARTIFACTS,
                        None,
                    )
                    .await
                    .is_err(),
                "{case_name} response must fail"
            );
            server.await.expect("join invalid-response server");
            assert_eq!(fs::read(model_dir.join("complete.bin")).await.unwrap(), b"C");
            assert!(!matches!(test_model_status(&engine).await, ModelStatus::Available));
            assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
        }
    }

    #[tokio::test]
    async fn pending_cancellation_keeps_owner_and_blocks_retry() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("encoder.bin"), b"AB")
            .await
            .expect("seed resumable prefix");
        let owner = engine
            .downloads
            .reserve(TEST_MODEL_NAME)
            .await
            .expect("reserve initial owner");

        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
                .await
                .expect("request cancellation"),
            CancelDownloadOutcome::Pending
        );
        assert!(engine.downloads.reserve(TEST_MODEL_NAME).await.is_err());

        let error = engine
            .finish_download(
                TEST_MODEL_NAME,
                &model_dir,
                SMALL_ARTIFACTS,
                &owner,
                Err(DownloadCancelled.into()),
                None,
            )
            .await
            .expect_err("cancelled owner must finish as cancellation");
        assert!(is_download_cancelled(&error));
        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"AB");
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
        assert!(engine.downloads.reserve(TEST_MODEL_NAME).await.is_ok());
    }

    #[tokio::test]
    async fn cancellation_wins_before_terminal_commit() {
        const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec::same("model.bin", 4)];
        let (_temp_dir, engine, model_dir) = test_engine().await;
        let hook = Arc::new(DownloadStateTestHook {
            finalization_ready: tokio::sync::Notify::new(),
            continue_finalization: tokio::sync::Notify::new(),
            discovery_scanned: tokio::sync::Notify::new(),
            continue_discovery: tokio::sync::Notify::new(),
        });
        *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));
        let events = Arc::new(SegQueue::new());
        let callback_events = Arc::clone(&events);

        let (base_url, server) =
            serve_requests(vec![response("model.bin", None, "200 OK", b"ABCD", None)]).await;
        let download_engine = Arc::clone(&engine);
        let download_dir = model_dir.clone();
        let download = tokio::spawn(async move {
            download_engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &download_dir,
                    &base_url,
                    ARTIFACTS,
                    Some(Box::new(move |progress| {
                        callback_events.push(progress);
                    })),
                )
                .await
        });

        hook.finalization_ready.notified().await;
        assert_eq!(
            engine
                .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
                .await
                .expect("request cancellation while finalization is paused"),
            CancelDownloadOutcome::Pending
        );
        hook.continue_finalization.notify_one();

        let error = download
            .await
            .expect("join download task")
            .expect_err("cancellation must win before terminal commit");
        server.await.expect("join cancellation server");
        assert!(is_download_cancelled(&error));
        assert!(std::iter::from_fn(|| events.pop()).all(|progress| progress.percent < 100));
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Missing));
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
    }

    #[tokio::test]
    async fn discovery_retries_when_download_finalizes_after_disk_scan() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        let hook = Arc::new(DownloadStateTestHook {
            finalization_ready: tokio::sync::Notify::new(),
            continue_finalization: tokio::sync::Notify::new(),
            discovery_scanned: tokio::sync::Notify::new(),
            continue_discovery: tokio::sync::Notify::new(),
        });
        *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));

        let discovery_engine = Arc::clone(&engine);
        let discovery = tokio::spawn(async move {
            discovery_engine
                .discover_models_from_specs(SMALL_MODEL_SPECS)
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), hook.discovery_scanned.notified())
            .await
            .expect("discovery must finish its first disk scan");

        fs::create_dir_all(&model_dir).await.expect("create model directory");
        for artifact in SMALL_ARTIFACTS {
            fs::write(
                model_dir.join(artifact.local),
                vec![0; artifact.exact_bytes as usize],
            )
            .await
            .expect("seed exact artifact");
        }

        let owner = engine
            .downloads
            .reserve(TEST_MODEL_NAME)
            .await
            .expect("reserve download owner");
        let finalization_engine = Arc::clone(&engine);
        let finalization_dir = model_dir.clone();
        let finalization_owner = Arc::clone(&owner);
        let finalization = tokio::spawn(async move {
            finalization_engine
                .finish_download(
                    TEST_MODEL_NAME,
                    &finalization_dir,
                    SMALL_ARTIFACTS,
                    &finalization_owner,
                    Ok(DownloadProgress::new(10, 10, 0.0)),
                    None,
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), hook.finalization_ready.notified())
            .await
            .expect("finalization must reach its commit barrier");
        hook.continue_finalization.notify_one();
        tokio::time::timeout(Duration::from_secs(1), finalization)
            .await
            .expect("finalization must complete")
            .expect("join finalization task")
            .expect("successful download must finalize");

        *engine.download_state_test_hook.lock().await = None;
        hook.continue_discovery.notify_one();
        let discovered = tokio::time::timeout(Duration::from_secs(1), discovery)
            .await
            .expect("discovery must complete")
            .expect("join discovery task")
            .expect("discovery must succeed");

        let discovered_model = discovered
            .iter()
            .find(|model| model.name == TEST_MODEL_NAME)
            .expect("test model must be discovered");
        assert!(matches!(discovered_model.status, ModelStatus::Available));
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Available));
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
    }

    // Fork test (harden-model-downloads 1.8): cancel keeps the bytes already written.
    #[tokio::test]
    async fn cancelled_download_leaves_partial_files_on_disk() {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.expect("create model directory");
        fs::write(model_dir.join("encoder.bin"), b"AB")
            .await
            .expect("seed encoder prefix");
        let (release_tx, release_rx) = oneshot::channel();
        let (progress_tx, progress_rx) = oneshot::channel();
        let progress_tx = Arc::new(std::sync::Mutex::new(Some(progress_tx)));
        let (base_url, server) = serve_requests(vec![ExpectedResponse {
            filename: "encoder.bin",
            range: Some("bytes=2-"),
            status: "206 Partial Content",
            content_length: Some(2),
            content_range: Some("bytes 2-3/4"),
            body: b"C",
            release_after_body: Some(release_rx),
        }])
        .await;

        let download_engine = Arc::clone(&engine);
        let download_dir = model_dir.clone();
        let download = tokio::spawn(async move {
            download_engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &download_dir,
                    &base_url,
                    SMALL_ARTIFACTS,
                    Some(Box::new(move |progress| {
                        if progress.downloaded_bytes == 3 {
                            if let Some(sender) = progress_tx
                                .lock()
                                .expect("lock progress sender")
                                .take()
                            {
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
        assert_eq!(
            engine
                .cancel_download(TEST_MODEL_NAME)
                .await
                .expect("cancel download"),
            CancelDownloadOutcome::Cancelled
        );
        release_tx.send(()).expect("release partial response");
        let error = download
            .await
            .expect("join cancelled download")
            .expect_err("cancelled download must not succeed");
        server.await.expect("join partial-response server");

        assert!(is_download_cancelled(&error));
        assert_eq!(fs::read(model_dir.join("encoder.bin")).await.unwrap(), b"ABC");
        assert!(matches!(test_model_status(&engine).await, ModelStatus::Missing));
        assert!(!engine.downloads.lock().await.contains(TEST_MODEL_NAME));
    }
}
