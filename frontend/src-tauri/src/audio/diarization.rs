use crate::audio::decoder::decode_audio_file;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

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

pub fn is_diarization_in_progress() -> bool {
    DIARIZATION_IN_PROGRESS.load(Ordering::SeqCst)
}

pub fn cancel_diarization() {
    DIARIZATION_CANCELLED.store(true, Ordering::SeqCst);
    info!("Diarization cancellation requested");
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
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    let _guard = DiarizationGuard::acquire()?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

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
        run_diarization_blocking(&app_clone, &meeting_id_clone, &folder_path, &models_dir, max_speakers, &transcripts)
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

fn run_diarization_blocking<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder_path: &str,
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
    transcripts: &[crate::database::models::Transcript],
) -> Result<(DiarizationResult, Vec<(String, String)>), String> {
    emit_progress(app, meeting_id, "loading", 10, "Finding audio file...");

    let audio_path = find_audio_file(folder_path)?;

    emit_progress(app, meeting_id, "decoding", 15, "Decoding audio...");

    let decoded = decode_audio_file(&audio_path)
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

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
    let diarizer = create_polyvoice_diarizer(models_dir, max_speakers)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    let mic_segments = run_polyvoice_diarization(&diarizer, &mic_stream, decoded.sample_rate)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let sys_segments = if is_stereo {
        info!(
            "Running diarization on system channel ({} samples)",
            sys_stream.len()
        );
        run_polyvoice_diarization(&diarizer, &sys_stream, decoded.sample_rate)
            .map_err(|e| format!("Diarization failed: {}", e))?
    } else {
        info!("Mono audio — treating as remote-only, skipping system-channel run");
        Vec::new()
    };

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    emit_progress(app, meeting_id, "matching", 70, "Matching speakers to transcripts...");

    let speakers_found = count_unique_speakers(&mic_segments) + count_unique_speakers(&sys_segments);
    let speaker_updates = compute_speaker_matches(&mic_segments, &sys_segments, is_stereo, transcripts, app, meeting_id)?;

    Ok((
        DiarizationResult {
            meeting_id: meeting_id.to_string(),
            segments_labeled: speaker_updates.len(),
            speakers_found,
        },
        speaker_updates,
    ))
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
) -> Result<PolyvoiceDiarizer, String> {
    use polyvoice::models::metadata::{ModelConfigMeta, load_model_config};
    use polyvoice::models::default_manifest;
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
    let mut config = polyvoice::PowersetConfig::default().with_model_meta(&meta);

    // Override geometry from the manifest entry: polyvoice maps the ONNX
    // "window_size" key (samples, e.g. 160000) to "window_secs" (seconds),
    // producing a unit mismatch. The manifest entry carries the authoritative
    // geometry values in the correct units.
    config.window_secs = seg_entry.window_secs.unwrap_or(10.0);
    config.hop_secs = seg_entry.hop_secs.unwrap_or(2.0);
    config.sample_rate = seg_entry.sample_rate.unwrap_or(16000);

    let segmenter = polyvoice::PowersetSegmenter::with_config(&seg_model, config, ExecutionProvider::Cpu)
        .map_err(|e| format!("Failed to create segmenter: {}", e))?;

    let embedder = polyvoice::embedder::ResNet34Adapter::new(&emb_model, 1, ExecutionProvider::Cpu)
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

fn run_polyvoice_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
) -> Result<Vec<DiarizationSegment>, String> {
    use polyvoice::embedder::Embedder as _;
    use polyvoice::segmentation::Segmenter as _;

    const DIARIZATION_SAMPLE_RATE: u32 = 16000;

    // The powerset segmentation model expects 16kHz mono audio.
    let diar_samples: std::borrow::Cow<'_, [f32]> = if sample_rate != DIARIZATION_SAMPLE_RATE {
        log::info!(
            "Resampling audio from {}Hz to {}Hz for diarization",
            sample_rate,
            DIARIZATION_SAMPLE_RATE
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
    let raw_segments = match diarizer.segmenter.segment(&diar_samples) {
        Ok(segments) => segments,
        Err(e) => {
            log::warn!("Segmentation failed ({}), treating channel as silent", e);
            return Ok(Vec::new());
        }
    };

    if raw_segments.is_empty() {
        return Ok(Vec::new());
    }

    // Embed each segment's audio slice, then cluster into speakers.
    let mut segments: Vec<DiarizationSegment> = Vec::new();
    let mut embeddings: Vec<Vec<f32>> = Vec::new();
    for seg in &raw_segments {
        let start = (seg.time.start * DIARIZATION_SAMPLE_RATE as f64) as usize;
        let end =
            ((seg.time.end * DIARIZATION_SAMPLE_RATE as f64) as usize).min(diar_samples.len());
        if end <= start {
            continue;
        }
        match diarizer.embedder.embed(&diar_samples[start..end]) {
            Ok(embedding) => {
                embeddings.push(embedding);
                segments.push(DiarizationSegment {
                    start: seg.time.start as f32,
                    end: seg.time.end as f32,
                    speaker: -1,
                });
            }
            Err(e) => {
                log::warn!("Embedding extraction failed for a segment ({}), skipping it", e);
            }
        }
    }

    if segments.is_empty() {
        return Ok(Vec::new());
    }

    let labels = diarizer
        .clusterer
        .cluster(&embeddings)
        .map_err(|e| format!("Speaker clustering failed: {}", e))?;

    for (segment, label) in segments.iter_mut().zip(labels) {
        segment.speaker = label as i32;
    }

    segments.sort_by(|a, b| a.start.total_cmp(&b.start));

    info!("Diarization found {} segments with {} unique speakers",
        segments.len(),
        count_unique_speakers(&segments));

    Ok(segments)
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

fn find_audio_file(folder_path: &str) -> Result<std::path::PathBuf, String> {
    use crate::audio::constants::AUDIO_EXTENSIONS;
    let dir = std::path::Path::new(folder_path);
    if !dir.exists() {
        return Err(format!("Folder not found: {}", folder_path));
    }

    for entry in std::fs::read_dir(dir).map_err(|e| format!("Cannot read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("Dir entry error: {}", e))?;
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if AUDIO_EXTENSIONS.iter().any(|ae| ae.eq_ignore_ascii_case(ext)) {
                return Ok(path);
            }
        }
    }

    Err(format!("No audio file found in {}", folder_path))
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
        let diarizer = create_polyvoice_diarizer(&models_dir, None)
            .expect("polyvoice diarizer should initialize with the INT8 models");
        let samples = synthetic_speech_16k();
        let segments = run_polyvoice_diarization(&diarizer, &samples, 16000)
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
            1,
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

        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = diarization_model_paths(&models_dir);
        let extractor = polyvoice::embedder::ResNet34Adapter::new(
            &emb_model,
            1,
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("ResNet34Adapter should initialize");

        let vad = polyvoice::vad::EnergyVad::new(-100.0, 16000, 512);
        let mut pipeline = StreamingPipeline::with_latency_preset(
            vad,
            extractor,
            LatencyPreset::Balanced,
            polyvoice::vad::VadConfig::default(),
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
            1,
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
}
