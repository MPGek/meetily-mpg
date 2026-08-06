use crate::api::TranscriptSegment;
use crate::audio::decoder::decode_audio_file;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use log::{info, warn};
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
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    let _guard = DiarizationGuard::acquire()?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

    let pool = state.db_manager.pool();

    MeetingsRepository::update_diarization_status(pool, &meeting_id, "processing")
        .await
        .map_err(|e| format!("Failed to update diarization status: {}", e))?;

    let app_clone = app.clone();
    let meeting_id_clone = meeting_id.clone();
    let pool_clone = pool.clone();

    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models");

    let result = tokio::task::spawn_blocking(move || {
        run_diarization_blocking(&app_clone, &meeting_id_clone, &pool_clone, &models_dir)
    })
    .await
    .map_err(|e| format!("Diarization task panicked: {}", e))?
    .map_err(|e| {
        let _ = tokio::runtime::Handle::current().block_on(
            MeetingsRepository::update_diarization_status(pool, &meeting_id, "failed"),
        );
        e
    })?;

    MeetingsRepository::update_diarization_status(pool, &meeting_id, "complete")
        .await
        .map_err(|e| format!("Failed to update diarization status: {}", e))?;

    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.clone(),
            status: "complete".to_string(),
            progress: 100,
            message: format!("Labeled {} segments from {} speakers", result.segments_labeled, result.speakers_found),
        },
    );

    Ok(result)
}

fn run_diarization_blocking<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    pool: &sqlx::SqlitePool,
    models_dir: &PathBuf,
) -> Result<DiarizationResult, String> {
    emit_progress(app, meeting_id, "loading", 5, "Loading meeting metadata...");

    let transcripts = tokio::runtime::Handle::current()
        .block_on(MeetingsRepository::get_transcripts_for_diarization(pool, meeting_id))
        .map_err(|e| format!("Failed to load transcripts: {}", e))?;

    if transcripts.is_empty() {
        return Err("No transcripts found for this meeting".to_string());
    }

    emit_progress(app, meeting_id, "loading", 10, "Finding audio file...");

    let meeting = tokio::runtime::Handle::current()
        .block_on(MeetingsRepository::get_meeting_metadata(pool, meeting_id))
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| "Meeting not found".to_string())?;

    let folder_path = meeting
        .folder_path
        .ok_or_else(|| "Meeting has no folder path — cannot find audio file".to_string())?;

    let audio_path = find_audio_file(&folder_path)?;

    emit_progress(app, meeting_id, "decoding", 15, "Decoding audio...");

    let decoded = decode_audio_file(&audio_path)
        .map_err(|e| format!("Failed to decode audio: {}", e))?;

    emit_progress(app, meeting_id, "diarizing", 20, "Running speaker diarization...");

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let diar_segments = run_sherpa_diarization(&decoded.samples, decoded.sample_rate, models_dir)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    emit_progress(app, meeting_id, "matching", 70, "Matching speakers to transcripts...");

    let speakers_found = count_unique_speakers(&diar_segments);
    let labeled_count = match_speakers_to_transcripts(
        &diar_segments,
        &transcripts,
        pool,
        app,
        meeting_id,
    )?;

    Ok(DiarizationResult {
        meeting_id: meeting_id.to_string(),
        segments_labeled: labeled_count,
        speakers_found,
    })
}

#[derive(Debug, Clone)]
struct DiarizationSegment {
    start: f32,
    end: f32,
    speaker: i32,
}

fn run_sherpa_diarization(samples: &[f32], sample_rate: u32, models_dir: &PathBuf) -> Result<Vec<DiarizationSegment>, String> {
    use sherpa_onnx::{
        FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
        OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
        SpeakerEmbeddingExtractorConfig,
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
            num_clusters: -1,
            threshold: 0.5,
        },
        min_duration_on: 0.3,
        min_duration_off: 0.5,
    };

    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| "Failed to create diarizer — check model paths".to_string())?;

    let result = diarizer.process(samples)
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

fn match_speakers_to_transcripts<R: Runtime>(
    diar_segments: &[DiarizationSegment],
    transcripts: &[crate::database::models::Transcript],
    pool: &sqlx::SqlitePool,
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<usize, String> {
    let rt = tokio::runtime::Handle::current();
    let mut labeled_count = 0usize;
    let total = transcripts.len();

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
            "SystemAudio".to_string()
        } else {
            match find_best_speaker(diar_segments, t_start, t_end) {
                Some(spk) => format!("SPEAKER_{:02}", spk),
                None => continue,
            }
        };

        rt.block_on(
            MeetingsRepository::update_transcript_speaker(pool, &transcript.id, &speaker_id),
        )
        .map_err(|e| format!("Failed to update speaker: {}", e))?;

        labeled_count += 1;
    }

    emit_progress(app, meeting_id, "matching", 95, "Speaker matching complete");
    Ok(labeled_count)
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
