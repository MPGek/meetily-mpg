use crate::audio::decoder::decode_audio_file;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use futures_util::StreamExt;
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

    let diar_segments = run_sherpa_diarization(&decoded.samples, decoded.sample_rate, models_dir, max_speakers)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    emit_progress(app, meeting_id, "matching", 70, "Matching speakers to transcripts...");

    let speakers_found = count_unique_speakers(&diar_segments);
    let speaker_updates = compute_speaker_matches(&diar_segments, transcripts, app, meeting_id)?;

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

fn run_sherpa_diarization(
    samples: &[f32],
    sample_rate: u32,
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
) -> Result<Vec<DiarizationSegment>, String> {
    use sherpa_onnx::{
        FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
        OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
        SpeakerEmbeddingExtractorConfig,
    };

    const DIARIZATION_SAMPLE_RATE: u32 = 16000;

    // The pyannote segmentation model expects 16kHz mono audio.
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

    let seg_model = models_dir
        .join("sherpa-onnx-pyannote-segmentation-3-0")
        .join("model.int8.onnx");
    let emb_model = models_dir
        .join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx");

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

    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(seg_model.to_string_lossy().to_string()),
            },
            num_threads: 2,
            debug: false,
            provider: Some("cpu".to_string()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(emb_model.to_string_lossy().to_string()),
            num_threads: 2,
            debug: false,
            provider: Some("cpu".to_string()),
        },
        clustering: FastClusteringConfig {
            num_clusters: max_speakers.unwrap_or(-1),
            threshold: 0.5,
        },
        min_duration_on: 0.3,
        min_duration_off: 0.5,
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| "Failed to create diarizer — check model paths".to_string())?;

    let result = diarizer.process(&diar_samples)
        .ok_or_else(|| "Diarization processing failed — no result returned".to_string())?;

    let segments: Vec<DiarizationSegment> = result
        .sort_by_start_time()
        .into_iter()
        .map(|s| DiarizationSegment {
            start: s.start,
            end: s.end,
            speaker: s.speaker,
        })
        .collect();

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
    diar_segments: &[DiarizationSegment],
    transcripts: &[crate::database::models::Transcript],
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut updates: Vec<(String, String)> = Vec::new();
    let total = transcripts.len();
    let mut skipped_system = 0usize;
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

        let speaker_id = if transcript.source_device.as_deref() == Some("System") {
            skipped_system += 1;
            "SystemAudio".to_string()
        } else {
            match find_best_speaker(diar_segments, t_start, t_end) {
                Some(spk) => format!("SPEAKER_{:02}", spk),
                None => {
                    skipped_no_match += 1;
                    continue;
                }
            }
        };

        updates.push((transcript.id.clone(), speaker_id));
    }

    info!(
        "Speaker matching: {} total, {} matched, {} system-audio, {} no-match skipped",
        total,
        updates.len(),
        skipped_system,
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

    best_speaker
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

// ===== Model download helpers =====

const SEGMENTATION_ARCHIVE_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const EMBEDDING_MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";

fn diarization_model_paths(models_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let segmentation_dir = models_dir.join("sherpa-onnx-pyannote-segmentation-3-0");
    let segmentation_model = segmentation_dir.join("model.int8.onnx");
    let embedding_model = models_dir.join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx");
    (segmentation_model, embedding_model)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationModelStatus {
    pub segmentation_ready: bool,
    pub embedding_ready: bool,
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

async fn download_with_progress<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    dest: &std::path::Path,
    start_pct: u32,
    end_pct: u32,
    message: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Failed to start download from {}: {}", url, e))?;
    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut last_emitted_pct = start_pct;

    let parent = dest.parent().ok_or_else(|| "Invalid destination path".to_string())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("Failed to create model directory: {}", e))?;

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("Failed to create file {}: {}", dest.display(), e))?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error from {}: {}", url, e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write file {}: {}", dest.display(), e))?;
        downloaded += chunk.len() as u64;

        if total > 0 {
            let pct = start_pct + ((downloaded as f64 / total as f64) * (end_pct - start_pct) as f64) as u32;
            if pct > last_emitted_pct {
                last_emitted_pct = pct;
                let _ = app.emit(
                    "diarization-model-download-progress",
                    DiarizationDownloadProgress {
                        progress: pct,
                        message: message.to_string(),
                    },
                );
            }
        }
    }

    file.flush().await.map_err(|e| format!("Failed to flush file: {}", e))?;
    Ok(())
}

fn extract_segmentation_model<R: Runtime>(
    app: &AppHandle<R>,
    archive_path: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("Failed to open segmentation archive: {}", e))?;
    let decompressor = bzip2::read::BzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressor);

    let dest_dir = dest.parent().ok_or_else(|| "Invalid segmentation destination".to_string())?;

    for entry in archive.entries().map_err(|e| format!("Failed to read archive entries: {}", e))? {
        let mut entry = entry.map_err(|e| format!("Archive entry error: {}", e))?;
        let path = entry.path().map_err(|e| format!("Archive path error: {}", e))?;
        if path.file_name().map(|n| n == "model.int8.onnx").unwrap_or(false) {
            std::fs::create_dir_all(dest_dir)
                .map_err(|e| format!("Failed to create segmentation directory: {}", e))?;
            entry.unpack(dest).map_err(|e| format!("Failed to extract segmentation model: {}", e))?;
            let _ = app.emit(
                "diarization-model-download-progress",
                DiarizationDownloadProgress {
                    progress: 75,
                    message: "Extracted segmentation model".to_string(),
                },
            );
            return Ok(());
        }
    }

    Err("Segmentation archive did not contain model.int8.onnx".to_string())
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
    let (segmentation_model, embedding_model) = diarization_model_paths(&models_dir);

    let _ = app.emit(
        "diarization-model-download-progress",
        DiarizationDownloadProgress {
            progress: 0,
            message: "Starting diarization model download...".to_string(),
        },
    );

    // Download and extract segmentation archive to a temp file.
    let temp_archive = models_dir.join("sherpa-onnx-pyannote-segmentation-3-0.tar.bz2.tmp");
    download_with_progress(
        &app,
        SEGMENTATION_ARCHIVE_URL,
        &temp_archive,
        0,
        40,
        "Downloading segmentation model...",
    )
    .await?;

    let app_for_extract = app.clone();
    let temp_archive_clone = temp_archive.clone();
    let segmentation_model_clone = segmentation_model.clone();
    tokio::task::spawn_blocking(move || {
        extract_segmentation_model(&app_for_extract, &temp_archive_clone, &segmentation_model_clone)
    })
    .await
    .map_err(|e| format!("Extraction task panicked: {}", e))??;

    // Clean up archive
    let _ = tokio::fs::remove_file(&temp_archive).await;

    // Download embedding model
    download_with_progress(
        &app,
        EMBEDDING_MODEL_URL,
        &embedding_model,
        75,
        100,
        "Downloading speaker embedding model...",
    )
    .await?;

    let _ = app.emit(
        "diarization-model-download-complete",
        serde_json::json!({}),
    );

    Ok(())
}
