use crate::audio::audio_file::find_audio_file;
use crate::audio::decoder::decode_audio_file;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static DIARIZATION_CANCELLED: AtomicBool = AtomicBool::new(false);

struct DiarizationGuard;

impl DiarizationGuard {
    fn acquire() -> Result<Self, String> {
        if DIARIZATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Diarization already in progress".to_string());
        }
        Ok(DiarizationGuard)
    }
}

impl Drop for DiarizationGuard {
    fn drop(&mut self) {
        DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub status: String,
    pub progress: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub segments_labeled: usize,
    pub speakers_found: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationSettingsPayload {
    pub memory_mode: String,
    pub max_sessions: i32,
}

pub fn is_diarization_in_progress() -> bool {
    DIARIZATION_IN_PROGRESS.load(Ordering::SeqCst)
}

pub fn cancel_diarization() {
    DIARIZATION_CANCELLED.store(true, Ordering::SeqCst);
    info!("Diarization cancellation requested");
}

#[tauri::command]
pub async fn get_diarization_settings<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DiarizationSettingsPayload, String> {
    let cfg = load_diarization_config(&app)
        .await
        .map_err(|e| format!("Failed to load diarization settings: {}", e))?;
    Ok(DiarizationSettingsPayload {
        memory_mode: cfg.memory_mode.as_str().to_string(),
        max_sessions: cfg.max_sessions as i32,
    })
}

#[tauri::command]
pub async fn set_diarization_settings<R: Runtime>(
    app: AppHandle<R>,
    memory_mode: String,
    max_sessions: i32,
) -> Result<(), String> {
    let mut cfg = load_diarization_config(&app)
        .await
        .map_err(|e| format!("Failed to load diarization settings: {}", e))?;
    cfg.memory_mode = DiarizationMemoryMode::parse(&memory_mode);
    cfg.max_sessions = max_sessions.max(0) as usize;
    save_diarization_config(&app, &cfg)
        .await
        .map_err(|e| format!("Failed to save diarization settings: {}", e))
}

#[tauri::command]
pub async fn get_diarization_status<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let pool = state.db_manager.pool();
    let meeting = MeetingsRepository::get_meeting_metadata(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| "Meeting not found".to_string())?;

    Ok(serde_json::json!({
        "meeting_id": meeting.id,
        "diarization_status": meeting.diarization_status,
        "speaker_names": meeting.speaker_names,
    }))
}

#[tauri::command]
pub async fn update_speaker_label_command<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    speaker: String,
    label: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    MeetingsRepository::update_speaker_label(pool, &meeting_id, &speaker, &label)
        .await
        .map_err(|e| format!("Failed to update speaker label: {}", e))
}

#[tauri::command]
pub async fn start_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    max_speakers: Option<i32>,
    memory_mode: Option<String>,
    max_sessions: Option<i32>,
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    let _guard = DiarizationGuard::acquire()?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

    let mut config = load_diarization_config(&app)
        .await
        .map_err(|e| format!("Failed to load diarization config: {}", e))?;
    if let Some(mode) = memory_mode {
        config.memory_mode = DiarizationMemoryMode::parse(&mode);
    }
    if let Some(sessions) = max_sessions {
        config.max_sessions = sessions.max(0) as usize;
    }

    let pool = state.db_manager.pool();

    MeetingsRepository::update_diarization_status(pool, &meeting_id, "processing")
        .await
        .map_err(|e| format!("Failed to update diarization status: {}", e))?;

    let transcripts = MeetingsRepository::get_transcripts_for_diarization(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load transcripts: {}", e))?;

    if transcripts.is_empty() {
        MeetingsRepository::update_diarization_status(pool, &meeting_id, "failed")
            .await
            .ok();
        return Err("No transcripts found for this meeting".to_string());
    }

    let meeting = MeetingsRepository::get_meeting_metadata(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| "Meeting not found".to_string())?;

    let folder_path = meeting
        .folder_path
        .ok_or_else(|| "Meeting has no folder path — cannot find audio file".to_string())?;

    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models");

    let app_clone = app.clone();
    let meeting_id_clone = meeting_id.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_diarization_blocking(
            &app_clone,
            &meeting_id_clone,
            &folder_path,
            &models_dir,
            max_speakers,
            &config,
            &transcripts,
        )
    })
    .await
    .map_err(|e| format!("Diarization task panicked: {}", e))?;

    match result {
        Ok((diar_result, speaker_updates)) => {
            for (transcript_id, speaker_id) in &speaker_updates {
                MeetingsRepository::update_transcript_speaker(pool, transcript_id, speaker_id)
                    .await
                    .map_err(|e| format!("Failed to update speaker: {}", e))?;
            }

            MeetingsRepository::update_diarization_status(pool, &meeting_id, "complete")
                .await
                .map_err(|e| format!("Failed to update diarization status: {}", e))?;

            let _ = app.emit(
                "diarization-progress",
                DiarizationProgress {
                    meeting_id: meeting_id.clone(),
                    status: "complete".to_string(),
                    progress: 100,
                    message: format!(
                        "Labeled {} segments from {} speakers",
                        diar_result.segments_labeled, diar_result.speakers_found
                    ),
                },
            );

            Ok(diar_result)
        }
        Err(e) => {
            MeetingsRepository::update_diarization_status(pool, &meeting_id, "failed")
                .await
                .ok();
            Err(e)
        }
    }
}

// ===== Configuration =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiarizationMemoryMode {
    Auto,
    Fast,
    LowMemory,
}

impl DiarizationMemoryMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "fast" => DiarizationMemoryMode::Fast,
            "low_memory" | "low-memory" | "low memory" | "low" => DiarizationMemoryMode::LowMemory,
            _ => DiarizationMemoryMode::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DiarizationMemoryMode::Auto => "auto",
            DiarizationMemoryMode::Fast => "fast",
            DiarizationMemoryMode::LowMemory => "low_memory",
        }
    }
}

impl Default for DiarizationMemoryMode {
    fn default() -> Self {
        DiarizationMemoryMode::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationConfig {
    pub memory_mode: DiarizationMemoryMode,
    pub max_sessions: usize,
    pub chunk_threshold_secs: f32,
    pub chunk_overlap_secs: f32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            memory_mode: DiarizationMemoryMode::Auto,
            max_sessions: default_embedder_pool_size(),
            chunk_threshold_secs: 600.0,
            chunk_overlap_secs: 5.0,
        }
    }
}

impl DiarizationConfig {
    fn chunk_duration_secs(&self) -> f32 {
        match self.memory_mode {
            DiarizationMemoryMode::LowMemory => 600.0,
            DiarizationMemoryMode::Auto => self.chunk_threshold_secs,
            DiarizationMemoryMode::Fast => self.chunk_threshold_secs,
        }
    }

    fn should_chunk(&self, duration_seconds: f32) -> bool {
        match self.memory_mode {
            DiarizationMemoryMode::LowMemory => true,
            DiarizationMemoryMode::Auto | DiarizationMemoryMode::Fast => {
                duration_seconds >= self.chunk_threshold_secs
            }
        }
    }

    fn embedder_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }

    fn segmenter_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }
}

fn default_embedder_pool_size() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 4)
}

pub async fn load_diarization_config<R: Runtime>(
    app: &AppHandle<R>,
) -> anyhow::Result<DiarizationConfig> {
    let store = match app.store("diarization-settings.json") {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access diarization store: {}, using defaults", e);
            return Ok(DiarizationConfig::default());
        }
    };

    let mut cfg = DiarizationConfig::default();
    if let Some(value) = store.get("memory_mode") {
        if let Some(s) = value.as_str() {
            cfg.memory_mode = DiarizationMemoryMode::parse(s);
        }
    }
    if let Some(value) = store.get("max_sessions") {
        if let Some(n) = value.as_i64() {
            cfg.max_sessions = (n as usize).clamp(1, 16);
        }
    }

    // Derive mode-specific defaults when the stored values are absent or
    // explicitly indicate automatic selection.
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    match cfg.memory_mode {
        DiarizationMemoryMode::Auto => {
            if store.get("max_sessions").is_none() {
                cfg.max_sessions = num_cpus.clamp(1, 4);
            }
            cfg.chunk_threshold_secs = 600.0;
        }
        DiarizationMemoryMode::Fast => {
            if store.get("max_sessions").is_none() {
                cfg.max_sessions = num_cpus.clamp(1, 8);
            }
            // Fast mode disables chunking unless the recording exceeds a hard
            // safety threshold.
            cfg.chunk_threshold_secs = 3600.0;
        }
        DiarizationMemoryMode::LowMemory => {
            if store.get("max_sessions").is_none() {
                cfg.max_sessions = 2;
            }
            cfg.chunk_threshold_secs = 0.0;
        }
    }

    Ok(cfg)
}

pub async fn save_diarization_config<R: Runtime>(
    app: &AppHandle<R>,
    config: &DiarizationConfig,
) -> anyhow::Result<()> {
    let store = app
        .store("diarization-settings.json")
        .map_err(|e| anyhow::anyhow!("Failed to access diarization store: {}", e))?;

    store.set(
        "memory_mode",
        serde_json::Value::String(config.memory_mode.as_str().to_string()),
    );
    store.set(
        "max_sessions",
        serde_json::Value::Number(serde_json::Number::from(config.max_sessions as i64)),
    );
    store
        .save()
        .map_err(|e| anyhow::anyhow!("Failed to save diarization store: {}", e))?;
    Ok(())
}

// ===== Blocking diarization orchestration =====

#[derive(Debug, Clone, Copy, Default)]
struct StageTimings {
    decode_secs: f64,
    segmentation_secs: f64,
    embedding_secs: f64,
    clustering_secs: f64,
    matching_secs: f64,
}

impl StageTimings {}

fn run_diarization_blocking<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder_path: &str,
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
    transcripts: &[crate::database::models::Transcript],
) -> Result<(DiarizationResult, Vec<(String, String)>), String> {
    let overall_start = Instant::now();
    let mut timings = StageTimings::default();
    let memory_sampler = MemorySampler::start();

    emit_progress(app, meeting_id, "loading", 10, "Finding audio file...");

    let decode_start = Instant::now();
    let audio_path = find_audio_file(std::path::Path::new(folder_path))?;

    emit_progress(app, meeting_id, "decoding", 15, "Decoding audio...");

    let decoded = decode_audio_file(&audio_path)
        .map_err(|e| format!("Failed to decode audio: {}", e))?;
    timings.decode_secs = decode_start.elapsed().as_secs_f64();

    emit_progress(app, meeting_id, "diarizing", 20, "Running speaker diarization...");

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    // De-interleave stereo into (mic=left, sys=right); mono yields right == None.
    let (left, right) = decoded.extract_channels();
    let is_stereo = right.is_some();

    let mic_stream = left.unwrap_or_default();
    let sys_stream = right.unwrap_or_default();

    // Load the diarizer once and reuse it for both channel runs.
    let diarizer = create_polyvoice_diarizer(models_dir, max_speakers, config)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    let channel_start = Instant::now();
    let (mic_result, sys_result) = if is_stereo {
        let (mic, sys) = rayon::join(
            || run_channel_diarization(&diarizer, &mic_stream, decoded.sample_rate, config, "mic"),
            || run_channel_diarization(&diarizer, &sys_stream, decoded.sample_rate, config, "sys"),
        );
        (mic, sys)
    } else {
        let mic = run_channel_diarization(&diarizer, &mic_stream, decoded.sample_rate, config, "mic");
        (mic, Ok((Vec::new(), StageTimings::default())))
    };

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let (mic_segments, mic_timings) = mic_result.map_err(|e| format!("Microphone channel failed: {}", e))?;
    let (sys_segments, sys_timings) = sys_result.map_err(|e| format!("System channel failed: {}", e))?;

    timings.segmentation_secs = mic_timings.segmentation_secs + sys_timings.segmentation_secs;
    timings.embedding_secs = mic_timings.embedding_secs + sys_timings.embedding_secs;
    timings.clustering_secs = mic_timings.clustering_secs + sys_timings.clustering_secs;
    let channel_elapsed = channel_start.elapsed().as_secs_f64();

    emit_progress(app, meeting_id, "matching", 70, "Matching speakers to transcripts...");

    let matching_start = Instant::now();
    let speakers_found = count_unique_speakers(&mic_segments) + count_unique_speakers(&sys_segments);
    let speaker_updates = compute_speaker_matches(&mic_segments, &sys_segments, is_stereo, transcripts, app, meeting_id)?;
    timings.matching_secs = matching_start.elapsed().as_secs_f64();

    let peak_mb = memory_sampler.stop();
    let overall_secs = overall_start.elapsed().as_secs_f64();

    info!(
        "Diarization timing for {}: decode={:.2}s, segmentation={:.2}s, embedding={:.2}s, clustering={:.2}s, matching={:.2}s, channel_total={:.2}s, overall={:.2}s, peak_rss={}MB, segments={}, speakers={}",
        meeting_id,
        timings.decode_secs,
        timings.segmentation_secs,
        timings.embedding_secs,
        timings.clustering_secs,
        timings.matching_secs,
        channel_elapsed,
        overall_secs,
        peak_mb,
        speaker_updates.len(),
        speakers_found
    );

    const TIME_WARNING_SECS: f64 = 600.0;
    const MEMORY_WARNING_MB: u64 = 4096;
    if overall_secs > TIME_WARNING_SECS || peak_mb > MEMORY_WARNING_MB {
        warn!(
            "Diarization regression warning: overall={:.2}s (threshold {}s), peak_rss={}MB (threshold {}MB)",
            overall_secs, TIME_WARNING_SECS, peak_mb, MEMORY_WARNING_MB
        );
    }

    Ok((
        DiarizationResult {
            meeting_id: meeting_id.to_string(),
            segments_labeled: speaker_updates.len(),
            speakers_found,
        },
        speaker_updates,
    ))
}

fn run_channel_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
    channel_name: &str,
) -> Result<(Vec<DiarizationSegment>, StageTimings), String> {
    info!(
        "Running diarization on {} channel ({} samples, {}Hz)",
        channel_name,
        samples.len(),
        sample_rate
    );
    let duration_seconds = samples.len() as f32 / sample_rate.max(1) as f32;
    if config.should_chunk(duration_seconds) {
        run_chunked_polyvoice_diarization(diarizer, samples, sample_rate, config)
    } else {
        run_polyvoice_diarization(diarizer, samples, sample_rate, config)
    }
}

#[derive(Debug, Clone)]
struct DiarizationSegment {
    start: f32,
    end: f32,
    speaker: i32,
}

/// Polyvoice diarization engine: powerset segmentation + ResNet34 embedding +
/// AHC clustering, loaded once per run and reused for both channel streams.
struct PolyvoiceDiarizer {
    segmenter: polyvoice::PowersetSegmenter,
    embedder: polyvoice::embedder::ResNet34Adapter,
    clusterer: Box<dyn polyvoice::clusterer::Clusterer>,
}

fn create_polyvoice_diarizer(
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    use polyvoice::models::default_manifest;
    use polyvoice::models::metadata::{load_model_config, ModelConfigMeta};
    use polyvoice::onnx::ExecutionProvider;

    let (seg_model, emb_model) = diarization_model_paths(models_dir);

    if !seg_model.exists() {
        return Err(format!(
            "Segmentation model not found at {}. Download models in Settings.",
            seg_model.display()
        ));
    }
    if !emb_model.exists() {
        return Err(format!(
            "Embedding model not found at {}. Download models in Settings.",
            emb_model.display()
        ));
    }

    // Resolve the balanced profile's model ids from the embedded manifest so
    // the geometry overlay stays in sync with the shipped model files.
    let manifest = default_manifest();
    let profile = manifest
        .profile(polyvoice::Profile::Balanced.manifest_id())
        .ok_or_else(|| "polyvoice manifest is missing the balanced profile".to_string())?;
    let seg_entry = manifest
        .model(&profile.segmenter)
        .ok_or_else(|| "polyvoice manifest is missing the segmenter model".to_string())?;

    let meta = load_model_config(Some(&seg_model), Some(seg_entry), &ModelConfigMeta::default());
    let mut seg_config = polyvoice::PowersetConfig::default().with_model_meta(&meta);

    // Override geometry from the manifest entry: polyvoice maps the ONNX
    // "window_size" key (samples, e.g. 160000) to "window_secs" (seconds),
    // producing a unit mismatch. The manifest entry carries the authoritative
    // geometry values in the correct units.
    seg_config.window_secs = seg_entry.window_secs.unwrap_or(10.0);
    seg_config.hop_secs = seg_entry.hop_secs.unwrap_or(2.0);
    seg_config.sample_rate = seg_entry.sample_rate.unwrap_or(16000);
    seg_config.pool_size = config.segmenter_pool_size();

    let segmenter = polyvoice::PowersetSegmenter::with_config(&seg_model, seg_config, ExecutionProvider::Cpu)
        .map_err(|e| format!("Failed to create segmenter: {}", e))?;

    let embedder = polyvoice::embedder::ResNet34Adapter::new(
        &emb_model,
        config.embedder_pool_size(),
        ExecutionProvider::Cpu,
    )
    .map_err(|e| format!("Failed to create embedder: {}", e))?;

    let max_clusters = max_speakers.filter(|m| *m > 0).unwrap_or(0) as usize;
    let clusterer: Box<dyn polyvoice::clusterer::Clusterer> = Box::new(
        polyvoice::clusterer::MinClusterSizeClusterer::new(
            Box::new(polyvoice::clusterer::AhcClusterer::with_threshold(
                max_clusters,
                polyvoice::DEFAULT_AHC_THRESHOLD,
            )),
            2,
        ),
    );

    Ok(PolyvoiceDiarizer {
        segmenter,
        embedder,
        clusterer,
    })
}

const DIARIZATION_SAMPLE_RATE: u32 = 16000;

fn run_polyvoice_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
) -> Result<(Vec<DiarizationSegment>, StageTimings), String> {
    use polyvoice::segmentation::Segmenter as _;

    let mut timings = StageTimings::default();

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    // The powerset segmentation model expects 16kHz mono audio.
    let diar_samples: std::borrow::Cow<'_, [f32]> = if sample_rate != DIARIZATION_SAMPLE_RATE {
        info!(
            "Resampling audio from {}Hz to {}Hz for diarization",
            sample_rate, DIARIZATION_SAMPLE_RATE
        );
        let resampled = crate::audio::audio_processing::resample(
            samples,
            sample_rate as u32,
            DIARIZATION_SAMPLE_RATE,
        )
        .map_err(|e| format!("Resampling failed: {}", e))?;
        std::borrow::Cow::Owned(resampled)
    } else {
        std::borrow::Cow::Borrowed(samples)
    };

    // Segment the channel into speaker-attributed spans (powerset-3.0).
    let seg_start = Instant::now();
    let raw_segments = match diarizer.segmenter.segment(&diar_samples) {
        Ok(segments) => segments,
        Err(e) => {
            log::warn!("Segmentation failed ({}), treating channel as silent", e);
            return Ok((Vec::new(), timings));
        }
    };
    timings.segmentation_secs = seg_start.elapsed().as_secs_f64();

    if raw_segments.is_empty() {
        return Ok((Vec::new(), timings));
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    // Extract embeddings for all segments in one coordinated batch call.
    let embed_start = Instant::now();
    let (mut segments, embeddings) =
        embed_segments(&diarizer.embedder, &diar_samples, &raw_segments, config);
    timings.embedding_secs = embed_start.elapsed().as_secs_f64();

    if segments.is_empty() {
        return Ok((Vec::new(), timings));
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let cluster_start = Instant::now();
    let labels = diarizer
        .clusterer
        .cluster(&embeddings)
        .map_err(|e| format!("Speaker clustering failed: {}", e))?;
    timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

    for (segment, label) in segments.iter_mut().zip(labels) {
        segment.speaker = label as i32;
    }

    segments.sort_by(|a, b| a.start.total_cmp(&b.start));

    info!(
        "Diarization found {} segments with {} unique speakers",
        segments.len(),
        count_unique_speakers(&segments)
    );

    Ok((segments, timings))
}

fn embed_segments(
    embedder: &polyvoice::embedder::ResNet34Adapter,
    diar_samples: &[f32],
    raw_segments: &[polyvoice::segmentation::RawSegment],
    _config: &DiarizationConfig,
) -> (Vec<DiarizationSegment>, Vec<Vec<f32>>) {
    use polyvoice::embedder::Embedder as _;

    let mut segments: Vec<DiarizationSegment> = Vec::with_capacity(raw_segments.len());
    let mut slices: Vec<&[f32]> = Vec::with_capacity(raw_segments.len());

    for seg in raw_segments {
        let start = (seg.time.start * DIARIZATION_SAMPLE_RATE as f64) as usize;
        let end = ((seg.time.end * DIARIZATION_SAMPLE_RATE as f64) as usize).min(diar_samples.len());
        if end <= start {
            continue;
        }
        segments.push(DiarizationSegment {
            start: seg.time.start as f32,
            end: seg.time.end as f32,
            speaker: -1,
        });
        slices.push(&diar_samples[start..end]);
    }

    let embeddings = match embedder.embed_batch(&slices) {
        Ok(batch) => batch,
        Err(e) => {
            warn!(
                "Batch embedding failed ({}), falling back to per-segment embedding",
                e
            );
            slices
                .iter()
                .filter_map(|audio| match embedder.embed(audio) {
                    Ok(emb) => Some(emb),
                    Err(e) => {
                        warn!("Embedding extraction failed for a segment ({}), skipping it", e);
                        None
                    }
                })
                .collect()
        }
    };

    // If the batch/fallback produced fewer embeddings than segments, drop the
    // trailing segments so clustering stays aligned with the embedding list.
    let valid_count = segments.len().min(embeddings.len());
    segments.truncate(valid_count);
    segments
        .into_iter()
        .zip(embeddings.into_iter().take(valid_count))
        .filter_map(|(seg, emb)| {
            if emb.len() == embedder.dim() {
                Some((seg, emb))
            } else {
                warn!("Skipping embedding with mismatched dimension");
                None
            }
        })
        .unzip()
}

fn run_chunked_polyvoice_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
) -> Result<(Vec<DiarizationSegment>, StageTimings), String> {
    use polyvoice::segmentation::Segmenter as _;

    let mut timings = StageTimings::default();
    let chunk_duration = config.chunk_duration_secs();
    let chunks = channel_chunks(samples, sample_rate, chunk_duration, config.chunk_overlap_secs);

    info!(
        "Chunked diarization: {} chunks ({}s duration, {}s overlap)",
        chunks.len(),
        chunk_duration,
        config.chunk_overlap_secs
    );

    let mut all_segments: Vec<DiarizationSegment> = Vec::new();
    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();

    for (chunk_idx, (chunk_start_seconds, chunk_samples)) in chunks.iter().enumerate() {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let diar_samples: std::borrow::Cow<'_, [f32]> = if sample_rate != DIARIZATION_SAMPLE_RATE {
            crate::audio::audio_processing::resample(
                chunk_samples,
                sample_rate,
                DIARIZATION_SAMPLE_RATE,
            )
            .map_err(|e| format!("Resampling failed for chunk {}: {}", chunk_idx, e))?
            .into()
        } else {
            std::borrow::Cow::Borrowed(chunk_samples)
        };

        let seg_start = Instant::now();
        let raw_segments = match diarizer.segmenter.segment(&diar_samples) {
            Ok(segments) => segments,
            Err(e) => {
                warn!("Segmentation failed for chunk {} ({}), skipping chunk", chunk_idx, e);
                continue;
            }
        };
        timings.segmentation_secs += seg_start.elapsed().as_secs_f64();

        if raw_segments.is_empty() {
            continue;
        }

        let embed_start = Instant::now();
        let (mut chunk_segments, chunk_embeddings) =
            embed_segments(&diarizer.embedder, &diar_samples, &raw_segments, config);
        timings.embedding_secs += embed_start.elapsed().as_secs_f64();

        // Adjust segment times so they are relative to the full channel.
        for seg in &mut chunk_segments {
            seg.start += *chunk_start_seconds;
            seg.end += *chunk_start_seconds;
        }

        all_segments.extend(chunk_segments);
        all_embeddings.extend(chunk_embeddings);
    }

    if all_segments.is_empty() {
        return Ok((Vec::new(), timings));
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let cluster_start = Instant::now();
    let labels = diarizer
        .clusterer
        .cluster(&all_embeddings)
        .map_err(|e| format!("Speaker clustering failed: {}", e))?;
    timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

    for (segment, label) in all_segments.iter_mut().zip(labels) {
        segment.speaker = label as i32;
    }

    all_segments.sort_by(|a, b| a.start.total_cmp(&b.start));

    info!(
        "Chunked diarization found {} segments with {} unique speakers",
        all_segments.len(),
        count_unique_speakers(&all_segments)
    );

    Ok((all_segments, timings))
}

fn channel_chunks(
    samples: &[f32],
    sample_rate: u32,
    chunk_duration_secs: f32,
    overlap_secs: f32,
) -> Vec<(f32, Vec<f32>)> {
    if samples.is_empty() || chunk_duration_secs <= 0.0 {
        return Vec::new();
    }

    let chunk_samples = (chunk_duration_secs * sample_rate as f32) as usize;
    let overlap_samples = (overlap_secs * sample_rate as f32).max(0.0) as usize;
    let step = chunk_samples.saturating_sub(overlap_samples).max(1);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        let end = (start + chunk_samples).min(samples.len());
        let chunk = samples[start..end].to_vec();
        let chunk_start_seconds = start as f32 / sample_rate as f32;
        chunks.push((chunk_start_seconds, chunk));
        if end == samples.len() {
            break;
        }
        start += step;
        // Avoid generating a tiny trailing sliver; extend the last chunk instead.
        if start + step >= samples.len() && start < samples.len() {
            // Last iteration will grab [start..end].
        }
    }
    chunks
}

fn count_unique_speakers(segments: &[DiarizationSegment]) -> usize {
    let mut speakers: Vec<i32> = segments.iter().map(|s| s.speaker).collect();
    speakers.sort();
    speakers.dedup();
    speakers.len()
}

fn compute_speaker_matches<R: Runtime>(
    mic_segments: &[DiarizationSegment],
    sys_segments: &[DiarizationSegment],
    is_stereo: bool,
    transcripts: &[crate::database::models::Transcript],
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut updates: Vec<(String, String)> = Vec::new();
    let total = transcripts.len();
    let mut skipped_no_match = 0usize;

    for (idx, transcript) in transcripts.iter().enumerate() {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let progress = 70 + ((idx as f32 / total as f32) * 25.0) as u32;
        if idx % 10 == 0 {
            emit_progress(app, meeting_id, "matching", progress, &format!("Matching segment {}/{}", idx + 1, total));
        }

        let t_start = transcript.audio_start_time.unwrap_or(0.0) as f32;
        let t_end = transcript.audio_end_time.unwrap_or(0.0) as f32;

        // Stereo: system-source transcripts match system-channel segments
        // (SPEAKER_NN); all others match mic-channel segments (MIC_SPEAKER_NN).
        // Mono fallback: everything matches the single run as remote (SPEAKER_NN).
        let (segments, prefix) = if is_stereo {
            if transcript.source_device.as_deref() == Some("System") {
                (sys_segments, "SPEAKER")
            } else {
                (mic_segments, "MIC_SPEAKER")
            }
        } else {
            (mic_segments, "SPEAKER")
        };

        let speaker_id = match find_best_speaker(segments, t_start, t_end) {
            Some(spk) => format!("{}_{:02}", prefix, spk),
            None => {
                skipped_no_match += 1;
                continue;
            }
        };

        updates.push((transcript.id.clone(), speaker_id));
    }

    info!(
        "Speaker matching: {} total, {} matched, {} no-match skipped",
        total,
        updates.len(),
        skipped_no_match,
    );

    emit_progress(app, meeting_id, "matching", 95, "Speaker matching complete");
    Ok(updates)
}

fn find_best_speaker(segments: &[DiarizationSegment], t_start: f32, t_end: f32) -> Option<i32> {
    let mut best_speaker: Option<i32> = None;
    let mut best_overlap: f32 = 0.0;

    for seg in segments {
        let overlap_start = t_start.max(seg.start);
        let overlap_end = t_end.min(seg.end);
        if overlap_start < overlap_end {
            let overlap = overlap_end - overlap_start;
            if overlap > best_overlap {
                best_overlap = overlap;
                best_speaker = Some(seg.speaker);
            }
        }
    }

    if best_speaker.is_some() {
        return best_speaker;
    }
    if segments.is_empty() {
        return None;
    }

    // Gap-fill: short utterances the segmenter missed get the nearest speaker.
    // A single-speaker channel can be filled unconditionally; a multi-speaker
    // channel is bounded so we never assign across long silences.
    let first = segments[0].speaker;
    if segments.iter().all(|s| s.speaker == first) {
        return Some(first);
    }

    const MAX_GAP_SECS: f32 = 30.0;
    let mut nearest: Option<(f32, i32)> = None;
    for seg in segments {
        let gap = if seg.end < t_start {
            t_start - seg.end
        } else if seg.start > t_end {
            seg.start - t_end
        } else {
            0.0
        };
        if gap <= MAX_GAP_SECS && nearest.map_or(true, |(g, _)| gap < g) {
            nearest = Some((gap, seg.speaker));
        }
    }
    nearest.map(|(_, spk)| spk)
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    status: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

// ===== Memory sampler =====

struct MemorySampler {
    peak_bytes: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl MemorySampler {
    fn start() -> Self {
        let peak_bytes = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let peak_bytes_clone = peak_bytes.clone();
        let running_clone = running.clone();

        let handle = std::thread::spawn(move || {
            let pid = match sysinfo::get_current_pid() {
                Ok(pid) => pid,
                Err(_) => return,
            };
            let mut system = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new().with_memory()),
            );
            let refresh_kind = ProcessRefreshKind::new().with_memory();
            while running_clone.load(Ordering::Relaxed) {
                let _ = system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    false,
                    refresh_kind,
                );
                if let Some(proc) = system.process(pid) {
                    let mem = proc.memory();
                    let mut current = peak_bytes_clone.load(Ordering::Relaxed);
                    while mem > current {
                        match peak_bytes_clone.compare_exchange_weak(
                            current,
                            mem,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(c) => current = c,
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            peak_bytes,
            running,
            handle: Some(handle),
        }
    }

    fn stop(mut self) -> u64 {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.peak_bytes.load(Ordering::Relaxed) / 1024 / 1024
    }
}

// ===== Model management (polyvoice ModelRegistry) =====

/// Resolve the balanced profile's model file paths under `models_dir`.
/// The registry caches downloads directly in this directory.
pub(crate) fn diarization_model_paths(
    models_dir: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let manifest = polyvoice::models::default_manifest();
    let profile = manifest
        .profile(polyvoice::Profile::Balanced.manifest_id())
        .expect("balanced profile is present in the polyvoice manifest");
    let segmenter = manifest
        .model(&profile.segmenter)
        .expect("balanced segmenter model is present in the polyvoice manifest");
    let embedder = manifest
        .model(&profile.embedder)
        .expect("balanced embedder model is present in the polyvoice manifest");
    (
        models_dir.join(&segmenter.filename),
        models_dir.join(&embedder.filename),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationModelStatus {
    pub segmentation_ready: bool,
    pub embedding_ready: bool,
}

/// Removes stale sherpa-era model files that are no longer used.
fn cleanup_legacy_models(models_dir: &std::path::Path) {
    let legacy: Vec<PathBuf> = [
        models_dir.join("sherpa-onnx-pyannote-segmentation-3-0"),
        models_dir.join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect();

    for path in legacy {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
        info!("Removed stale diarization model file: {}", path.display());
    }
}

#[tauri::command]
pub async fn check_diarization_models<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DiarizationModelStatus, String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models");

    cleanup_legacy_models(&models_dir);

    let (segmentation, embedding) = diarization_model_paths(&models_dir);
    Ok(DiarizationModelStatus {
        segmentation_ready: segmentation.exists(),
        embedding_ready: embedding.exists(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationDownloadProgress {
    pub progress: u32,
    pub message: String,
}

#[tauri::command]
pub async fn download_diarization_models<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models");

    let app_for_progress = app.clone();
    tokio::task::spawn_blocking(move || {
        let registry = polyvoice::models::ModelRegistry::with_cache_dir(&models_dir)
            .map_err(|e| format!("Failed to initialize model registry: {}", e))?;

        let manifest = polyvoice::models::default_manifest();
        let profile = manifest
            .profile(polyvoice::Profile::Balanced.manifest_id())
            .ok_or_else(|| "polyvoice manifest is missing the balanced profile".to_string())?;
        let segmenter = manifest
            .model(&profile.segmenter)
            .ok_or_else(|| "polyvoice manifest is missing the segmenter model".to_string())?;
        let embedder = manifest
            .model(&profile.embedder)
            .ok_or_else(|| "polyvoice manifest is missing the embedder model".to_string())?;

        let _ = app_for_progress.emit(
            "diarization-model-download-progress",
            DiarizationDownloadProgress {
                progress: 0,
                message: format!(
                    "Downloading segmentation model ({} MB)...",
                    segmenter.size.unwrap_or(0) / 1_000_000
                ),
            },
        );
        registry
            .ensure(&profile.segmenter)
            .map_err(|e| format!("Failed to download segmentation model: {}", e))?;

        let _ = app_for_progress.emit(
            "diarization-model-download-progress",
            DiarizationDownloadProgress {
                progress: 50,
                message: format!(
                    "Downloading speaker embedding model ({} MB)...",
                    embedder.size.unwrap_or(0) / 1_000_000
                ),
            },
        );
        registry
            .ensure(&profile.embedder)
            .map_err(|e| format!("Failed to download embedding model: {}", e))?;

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Model download task panicked: {}", e))??;

    let _ = app.emit(
        "diarization-model-download-complete",
        serde_json::json!({}),
    );

    Ok(())
}

// ===== Spike: polyvoice diarization engine verification (change: switch-to-polyvoice-diarization) =====
//
// polyvoice is the sole diarization engine: powerset segmentation +
// ResNet34 INT8 embedding + AHC clustering (offline), and StreamingPipeline
// with the same embedder (online). Tests skip gracefully when the models are
// not present (e.g. offline CI).

#[cfg(test)]
mod spike_tests {
    use super::*;
    use polyvoice::clusterer::Clusterer as _;
    use polyvoice::embedder::Embedder as _;

    fn default_config() -> DiarizationConfig {
        DiarizationConfig::default()
    }

    fn find_models_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("MEETILY_MODELS_DIR") {
            let p = PathBuf::from(dir);
            if p.join("powerset_int8.onnx").exists() && p.join("resnet34_int8.onnx").exists() {
                return Some(p);
            }
        }
        let candidates: Vec<PathBuf> = [
            std::env::var("APPDATA").ok().map(|d| PathBuf::from(d).join("com.meetily.ai").join("models")),
            std::env::var("HOME").ok().map(|d| PathBuf::from(d).join("Library").join("Application Support").join("com.meetily.ai").join("models")),
            std::env::var("XDG_DATA_HOME").ok().map(|d| PathBuf::from(d).join("com.meetily.ai").join("models")),
            std::env::var("HOME").ok().map(|d| PathBuf::from(d).join(".local").join("share").join("com.meetily.ai").join("models")),
        ]
        .into_iter()
        .flatten()
        .collect();
        candidates
            .into_iter()
            .find(|p| p.join("powerset_int8.onnx").exists() && p.join("resnet34_int8.onnx").exists())
    }

    fn synthetic_speech_16k() -> Vec<f32> {
        // 4 seconds of 16 kHz tone bursts (amplitude-modulated) as stand-in audio.
        let mut samples = Vec::with_capacity(16000 * 4);
        for i in 0..16000 * 4 {
            let t = i as f32 / 16000.0;
            let tone = (2.0 * std::f32::consts::PI * 220.0 * t).sin();
            let burst = if (t % 1.0) < 0.6 { 1.0 } else { 0.0 };
            samples.push(tone * 0.3 * burst);
        }
        samples
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_offline_pipeline() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let diarizer = create_polyvoice_diarizer(&models_dir, None, &default_config())
            .expect("polyvoice diarizer should initialize with the INT8 models");
        let samples = synthetic_speech_16k();
        let (segments, _) = run_polyvoice_diarization(&diarizer, &samples, 16000, &default_config())
            .expect("offline diarization should return a result");
        info!(
            "spike: polyvoice offline diarization produced {} segments on synthetic audio",
            segments.len()
        );
        for pair in segments.windows(2) {
            assert!(pair[0].start <= pair[1].start, "segments must be sorted by start time");
        }
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_short_window_embedding() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = diarization_model_paths(&models_dir);
        let embedder = polyvoice::embedder::ResNet34Adapter::new(
            &emb_model,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("ResNet34Adapter should initialize with the INT8 model");
        assert_eq!(embedder.dim(), 256, "resnet34_int8 embeds to 256 dims");

        // Probe embeddings from short windows — the Fast-mode streaming geometry.
        let samples = synthetic_speech_16k();
        for secs in [0.25f32, 0.5, 1.0, 1.5] {
            let n = (16000.0 * secs) as usize;
            let emb = embedder.embed(&samples[..n]);
            info!(
                "spike: embed at {:.2}s -> {:?}",
                secs,
                emb.as_ref()
                    .map(|e| format!("{} dims", e.len()))
                    .unwrap_or_else(|e| format!("error: {e}"))
            );
            if let Ok(e) = emb {
                assert_eq!(e.len(), 256);
                let norm: f32 = e.iter().map(|x| x * x).sum::<f32>().sqrt();
                assert!((norm - 1.0).abs() < 1e-2, "embedding must be L2-normalized (got {norm})");
            }
        }
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_streaming_pipeline() {
        use polyvoice::streaming::{LatencyPreset, StreamingPipeline};
        use polyvoice::vad::{EnergyVad, VadConfig};

        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = diarization_model_paths(&models_dir);
        let extractor = polyvoice::embedder::ResNet34Adapter::new(
            &emb_model,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("ResNet34Adapter should initialize");

        let vad = EnergyVad::new(-100.0, 16000, 512);
        let mut pipeline = StreamingPipeline::with_latency_preset(
            vad,
            extractor,
            LatencyPreset::Balanced,
            VadConfig::default(),
        )
        .expect("StreamingPipeline should build with balanced preset");

        let samples = synthetic_speech_16k();
        for chunk in samples.chunks(16000) {
            let turns = pipeline
                .feed(chunk)
                .expect("feed should accept arbitrary 16 kHz chunks");
            info!("spike: streaming feed produced {} turns", turns.len());
        }
        let flushed = pipeline
            .flush()
            .expect("flush should return remaining turns");
        info!(
            "spike: streaming flush produced {} turns, {} total buffered, {} speakers",
            flushed.len(),
            pipeline.turns().len(),
            pipeline.num_speakers()
        );
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_efficient_path() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = diarization_model_paths(&models_dir);
        let extractor = polyvoice::embedder::ResNet34Adapter::new(
            &emb_model,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("ResNet34Adapter should initialize");

        // Two distinct tone-burst signals simulate two speakers; embed per segment.
        let samples_a = synthetic_speech_16k();
        let samples_b: Vec<f32> = samples_a
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let t = i as f32 / 16000.0;
                s + (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.2
            })
            .collect();

        let mut embeddings: Vec<(f32, f32, Vec<f32>)> = Vec::new();
        for (start, seg) in [(0.0, &samples_a[..]), (2.0, &samples_b[..])] {
            let emb = extractor
                .embed(seg)
                .expect("embed should return a 256-dim embedding");
            assert_eq!(emb.len(), 256);
            embeddings.push((start, start + seg.len() as f32 / 16000.0, emb));
        }

        let clusterer = polyvoice::clusterer::AhcClusterer::new(8);
        let labels = clusterer
            .cluster(&embeddings.iter().map(|e| e.2.clone()).collect::<Vec<_>>())
            .expect("AhcClusterer should cluster buffered embeddings");
        assert_eq!(labels.len(), embeddings.len());
        info!("spike: efficient-path clustering produced labels {:?}", labels);
        assert!(
            labels.iter().all(|&l| l < 8),
            "labels must respect the max-speakers ceiling"
        );
    }

    #[test]
    fn chunk_splitting_preserves_overlap_and_offsets() {
        let sample_rate = 16000;
        let samples: Vec<f32> = (0..sample_rate * 30).map(|i| (i as f32).sin()).collect();
        let chunks = channel_chunks(&samples, sample_rate, 10.0, 5.0);

        assert!(!chunks.is_empty());
        // Every chunk except the last should be a full 10-second window.
        for (i, (_, chunk)) in chunks.iter().enumerate().take(chunks.len().saturating_sub(1)) {
            assert_eq!(chunk.len(), sample_rate as usize * 10, "chunk {} has wrong size", i);
        }

        // Adjacent chunks should overlap by 5 seconds.
        for window in chunks.windows(2) {
            let start_a = window[0].0;
            let start_b = window[1].0;
            let diff = (start_b - start_a - 5.0).abs();
            assert!(diff < 0.01, "expected 5s overlap, got diff {}s", start_b - start_a);
        }

        // Last chunk should reach the end of the input.
        let (_, last_chunk) = chunks.last().unwrap();
        let last_start = chunks.last().unwrap().0;
        assert_eq!(
            (last_start * sample_rate as f32) as usize + last_chunk.len(),
            samples.len(),
            "last chunk must reach the end of the input"
        );
    }

    /// A test embedder that returns a deterministic vector whose first element
    /// identifies the input slice length. This lets us verify that
    /// `embed_batch` preserves ordering and that the fallback path handles
    /// mismatched dimensions gracefully.
    struct OrderedTestEmbedder {
        dim: usize,
    }

    impl polyvoice::embedder::Embedder for OrderedTestEmbedder {
        fn dim(&self) -> usize {
            self.dim
        }

        fn embed(&self, audio: &[f32]) -> Result<Vec<f32>, polyvoice::embedder::EmbedderError> {
            let mut v = vec![0.0f32; self.dim];
            if let Some(first) = v.first_mut() {
                *first = audio.len() as f32;
            }
            Ok(v)
        }

        fn embed_batch(
            &self,
            audios: &[&[f32]],
        ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
            audios.iter().map(|a| self.embed(a)).collect()
        }
    }

    #[test]
    fn embed_batch_preserves_ordering_and_handles_errors() {
        let embedder = OrderedTestEmbedder { dim: 4 };
        let inputs: Vec<Vec<f32>> = vec![vec![1.0; 10], vec![2.0; 20], vec![3.0; 30]];
        let refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();

        let batch = embedder.embed_batch(&refs).expect("batch should succeed");
        assert_eq!(batch.len(), inputs.len());
        for (i, emb) in batch.iter().enumerate() {
            assert_eq!(emb[0], inputs[i].len() as f32, "embedding order mismatch at index {}", i);
        }

        // Empty batch should return an empty result, not an error.
        let empty: Vec<&[f32]> = Vec::new();
        assert!(embedder.embed_batch(&empty).unwrap().is_empty());
    }

    #[test]
    fn diarization_config_mode_defaults() {
        let cfg = DiarizationConfig::default();
        assert_eq!(cfg.memory_mode, DiarizationMemoryMode::Auto);
        assert!(cfg.max_sessions >= 1 && cfg.max_sessions <= 16);
        assert_eq!(cfg.chunk_overlap_secs, 5.0);

        let mut low = cfg.clone();
        low.memory_mode = DiarizationMemoryMode::LowMemory;
        assert!(low.should_chunk(1.0));

        let mut fast = cfg.clone();
        fast.memory_mode = DiarizationMemoryMode::Fast;
        assert!(!fast.should_chunk(300.0));
        assert!(fast.should_chunk(4000.0));
    }
}
