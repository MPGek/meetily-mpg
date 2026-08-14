// Retranscription module - allows re-processing stored audio with different settings

use crate::api::TranscriptSegment;
use crate::audio::audio_file::find_audio_file;
use crate::audio::decoder::decode_audio_file;
use crate::audio::vad::{get_speech_chunks_with_progress, merge_segments, VadConfig};
use super::common::{create_transcript_segments, create_transcript_segments_with_source, write_transcripts_json};
use crate::config::{DEFAULT_WHISPER_MODEL, DEFAULT_PARAKEET_MODEL};
use crate::parakeet_engine::ParakeetEngine;
use crate::state::AppState;
use crate::whisper_engine::WhisperEngine;
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Global flag to track if retranscription is in progress
static RETRANSCRIPTION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Global flag to signal cancellation
static RETRANSCRIPTION_CANCELLED: AtomicBool = AtomicBool::new(false);

/// RAII guard for RETRANSCRIPTION_IN_PROGRESS flag
/// Ensures flag is cleared even if retranscription panics or returns early
struct RetranscriptionGuard;

impl RetranscriptionGuard {
    /// Create guard and set flag atomically
    fn acquire() -> Result<Self, String> {
        if RETRANSCRIPTION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Retranscription already in progress".to_string());
        }
        Ok(RetranscriptionGuard)
    }
}

impl Drop for RetranscriptionGuard {
    fn drop(&mut self) {
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// Progress update emitted during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionProgress {
    pub meeting_id: String,
    pub stage: String, // "decoding", "transcribing", "saving"
    pub progress_percentage: u32,
    pub message: String,
}

/// Result of retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionResult {
    pub meeting_id: String,
    pub segments_count: usize,
    pub duration_seconds: f64,
    pub language: Option<String>,
}

/// Error during retranscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionError {
    pub meeting_id: String,
    pub error: String,
}

/// Check if retranscription is currently in progress
pub fn is_retranscription_in_progress() -> bool {
    RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Cancel ongoing retranscription
pub fn cancel_retranscription() {
    RETRANSCRIPTION_CANCELLED.store(true, Ordering::SeqCst);
}

/// Start retranscription of a meeting's audio
pub async fn start_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    // Acquire guard - ensures flag is cleared even on panic/early return
    let _guard = RetranscriptionGuard::acquire().map_err(|e| anyhow!(e))?;

    // Reset cancellation flag
    RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);

    let use_parakeet = provider.as_deref() == Some("parakeet");
    let result = run_retranscription(app.clone(), meeting_id.clone(), meeting_folder_path, language, model, provider).await;

    // Unload the engine after the batch job (success, failure, or cancellation)
    super::common::unload_engine_after_batch(use_parakeet).await;

    // Guard will automatically clear flag on drop
    // No need for manual: RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);

    match &result {
        Ok(res) => {
            let _ = app.emit(
                "retranscription-complete",
                serde_json::json!({
                    "meeting_id": res.meeting_id,
                    "segments_count": res.segments_count,
                    "duration_seconds": res.duration_seconds,
                    "language": res.language
                }),
            );
        }
        Err(e) => {
            let _ = app.emit(
                "retranscription-error",
                RetranscriptionError {
                    meeting_id: meeting_id.clone(),
                    error: e.to_string(),
                },
            );
        }
    }

    result
}

/// Internal function to run retranscription
async fn run_retranscription<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionResult> {
    let folder_path = PathBuf::from(&meeting_folder_path);
    let audio_path = find_audio_file(&folder_path).map_err(|e| anyhow!(e))?;

    // Determine which provider to use (default to whisper)
    let use_parakeet = provider.as_deref() == Some("parakeet");

    info!(
        "Starting retranscription for meeting {} with language {:?}, model {:?}, provider {:?}",
        meeting_id, language, model, provider
    );

    // Emit progress: decoding
    emit_progress(&app, &meeting_id, "decoding", 5, "Decoding audio file...");

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Decode the audio file (CPU-intensive, run in blocking task)
    let path_for_decode = audio_path.clone();
    let decoded = tokio::task::spawn_blocking(move || {
        decode_audio_file(&path_for_decode)
    })
    .await
    .map_err(|e| anyhow!("Decode task panicked: {}", e))??;
    let duration_seconds = decoded.duration_seconds;

    info!(
        "Decoded audio: {:.2}s, {}Hz, {} channels",
        duration_seconds, decoded.sample_rate, decoded.channels
    );

    emit_progress(&app, &meeting_id, "decoding", 15, "Converting audio format...");

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Determine if audio is stereo or mono
    let is_stereo = decoded.channels == 2;
    info!("Audio is {} ({} channels)", if is_stereo { "stereo" } else { "mono" }, decoded.channels);

    // Extract channels and resample to 16kHz
    // For stereo: left=Microphone, right=System
    // For mono: single channel with source_device=None
    let (mic_samples, sys_samples) = if is_stereo {
        emit_progress(&app, &meeting_id, "decoding", 17, "Extracting audio channels...");

        let decoded_for_extract = decoded.clone();
        let (left, right) = tokio::task::spawn_blocking(move || {
            decoded_for_extract.extract_channels()
        })
        .await
        .map_err(|e| anyhow!("Channel extraction task panicked: {}", e))?;

        let left_samples = left.unwrap_or_default();
        let right_samples = right.unwrap_or_default();

        emit_progress(&app, &meeting_id, "decoding", 18, "Resampling channels to 16kHz...");

        // Resample each channel independently in blocking tasks
        let sample_rate = decoded.sample_rate;
        let left_for_resample = left_samples;
        let right_for_resample = right_samples;

        let mic_resampled = tokio::task::spawn_blocking(move || {
            resample_channel_to_16k(&left_for_resample, sample_rate)
        })
        .await
        .map_err(|e| anyhow!("Mic resample task panicked: {}", e))?;

        let sys_resampled = tokio::task::spawn_blocking(move || {
            resample_channel_to_16k(&right_for_resample, sample_rate)
        })
        .await
        .map_err(|e| anyhow!("System resample task panicked: {}", e))?;

        info!("Resampled mic channel: {} samples, system channel: {} samples",
            mic_resampled.len(), sys_resampled.len());

        (Some(mic_resampled), Some(sys_resampled))
    } else {
        // Mono: convert to 16kHz using existing path
        let mono_samples = tokio::task::spawn_blocking(move || {
            decoded.to_whisper_format()
        })
        .await
        .map_err(|e| anyhow!("Resample task panicked: {}", e))?;
        info!("Converted mono to 16kHz format: {} samples", mono_samples.len());
        (Some(mono_samples), None)
    };

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    // Run VAD on each channel independently
    // For stereo: VAD on mic (20-25%) and system (25-30%)
    // For mono: VAD on single channel (20-25%)
    let (mic_speech_segments, sys_speech_segments) = if is_stereo {
        let mic_audio = mic_samples.as_ref().unwrap().clone();
        let sys_audio = sys_samples.as_ref().unwrap().clone();

        emit_progress(&app, &meeting_id, "vad", 20, "Detecting speech in microphone channel...");

        let app_for_mic_vad = app.clone();
        let meeting_id_for_mic_vad = meeting_id.clone();
        let mic_segments = tokio::task::spawn_blocking(move || {
            get_speech_chunks_with_progress(
                &mic_audio,
                VadConfig::batch(),
                |vad_progress, segments_found| {
                    let overall_progress = 20 + (vad_progress as f32 * 0.05) as u32;
                    emit_progress(
                        &app_for_mic_vad,
                        &meeting_id_for_mic_vad,
                        "vad",
                        overall_progress,
                        &format!("Mic VAD... {}% ({} found)", vad_progress, segments_found),
                    );
                    !RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst)
                },
            )
        })
        .await
        .map_err(|e| anyhow!("Mic VAD task panicked: {}", e))?
        .map_err(|e| anyhow!("Mic VAD processing failed: {}", e))?;

        // Check for cancellation between channels
        if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Retranscription cancelled"));
        }

        emit_progress(&app, &meeting_id, "vad", 25, "Detecting speech in system channel...");

        let app_for_sys_vad = app.clone();
        let meeting_id_for_sys_vad = meeting_id.clone();
        let sys_segments = tokio::task::spawn_blocking(move || {
            get_speech_chunks_with_progress(
                &sys_audio,
                VadConfig::batch(),
                |vad_progress, segments_found| {
                    let overall_progress = 25 + (vad_progress as f32 * 0.05) as u32;
                    emit_progress(
                        &app_for_sys_vad,
                        &meeting_id_for_sys_vad,
                        "vad",
                        overall_progress,
                        &format!("System VAD... {}% ({} found)", vad_progress, segments_found),
                    );
                    !RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst)
                },
            )
        })
        .await
        .map_err(|e| anyhow!("System VAD task panicked: {}", e))?
        .map_err(|e| anyhow!("System VAD processing failed: {}", e))?;

        info!("VAD detected {} mic segments, {} system segments",
            mic_segments.len(), sys_segments.len());

        (mic_segments, sys_segments)
    } else {
        // Mono: single VAD pass
        let mono_audio = mic_samples.as_ref().unwrap().clone();

        emit_progress(&app, &meeting_id, "vad", 20, "Detecting speech segments...");

        let app_for_vad = app.clone();
        let meeting_id_for_vad = meeting_id.clone();
        let mono_segments = tokio::task::spawn_blocking(move || {
            get_speech_chunks_with_progress(
                &mono_audio,
                VadConfig::batch(),
                |vad_progress, segments_found| {
                    let overall_progress = 20 + (vad_progress as f32 * 0.05) as u32;
                    emit_progress(
                        &app_for_vad,
                        &meeting_id_for_vad,
                        "vad",
                        overall_progress,
                        &format!("Detecting speech segments... {}% ({} found)", vad_progress, segments_found),
                    );
                    !RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst)
                },
            )
        })
        .await
        .map_err(|e| anyhow!("VAD task panicked: {}", e))?
        .map_err(|e| anyhow!("VAD processing failed: {}", e))?;

        info!("VAD detected {} speech segments", mono_segments.len());

        (mono_segments, vec![])
    };

    let total_mic_segments = mic_speech_segments.len();
    let total_sys_segments = sys_speech_segments.len();
    let total_segments = total_mic_segments + total_sys_segments;

    // Log segment stats
    if total_segments == 0 {
        warn!("No speech detected in audio");
        return Err(anyhow!("No speech detected in audio file"));
    }

    info!("Total VAD segments: {} (mic: {}, system: {})", total_segments, total_mic_segments, total_sys_segments);

    emit_progress(&app, &meeting_id, "transcribing", 30, "Loading transcription engine...");

    // Initialize the appropriate engine once (not per-segment)
    let whisper_engine = if !use_parakeet {
        Some(get_or_init_whisper(&app, model.as_deref()).await?)
    } else {
        None
    };
    let parakeet_engine = if use_parakeet {
        Some(get_or_init_parakeet(&app, model.as_deref()).await?)
    } else {
        None
    };

    // Merge adjacent segments (gap < 2000ms) and split at 25s boundaries
    const MAX_SEGMENT_SAMPLES: usize = 25 * 16000;
    let mic_merged = merge_segments(&mic_speech_segments, 2000.0, MAX_SEGMENT_SAMPLES);
    let sys_merged = merge_segments(&sys_speech_segments, 2000.0, MAX_SEGMENT_SAMPLES);

    info!("After merge: mic {}→{} segments, sys {}→{} segments",
        mic_speech_segments.len(), mic_merged.len(),
        sys_speech_segments.len(), sys_merged.len());

    let mic_processable = mic_merged;
    let sys_processable = sys_merged;

    let mic_count = mic_processable.len();
    let sys_count = sys_processable.len();
    let total_processable = mic_count + sys_count;
    info!("Processing {} segments (mic: {}, system: {})", total_processable, mic_count, sys_count);

    // Transcribe each channel's segments with progress updates
    // Progress range: 30-80% for transcription
    let mut mic_transcripts: Vec<(String, f64, f64)> = Vec::new();
    let mut sys_transcripts: Vec<(String, f64, f64)> = Vec::new();
    let mut total_confidence = 0.0f32;
    let mut transcribed_count = 0;

    // Transcribe microphone segments
    for (i, segment) in mic_processable.iter().enumerate() {
        if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Retranscription cancelled"));
        }

        // Progress: 30-55% for mic (if stereo) or 30-80% for mono
        let progress_range = if is_stereo { 25.0 } else { 50.0 };
        let progress = 30 + ((i as f32 / mic_count.max(1) as f32) * progress_range) as u32;
        let segment_duration_sec = (segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0;
        emit_progress(
            &app,
            &meeting_id,
            "transcribing",
            progress,
            &format!("Transcribing mic segment {} of {} ({:.1}s)...", i + 1, mic_count, segment_duration_sec),
        );

        if segment.samples.len() < 1600 {
            debug!("Skipping short mic segment {} with {} samples", i, segment.samples.len());
            continue;
        }

        let (text, conf) = transcribe_segment(
            segment,
            &whisper_engine,
            &parakeet_engine,
            use_parakeet,
            language.clone(),
        ).await?;

        let trimmed = text.trim();
        if !trimmed.is_empty() {
            debug!("Mic segment {}/{}: {:.1}s, conf={:.2}, text='{}'", i + 1, mic_count, segment_duration_sec, conf,
                if trimmed.len() > 80 { &trimmed[..80] } else { trimmed });
            mic_transcripts.push((text, segment.start_timestamp_ms, segment.end_timestamp_ms));
            total_confidence += conf;
            transcribed_count += 1;
        }
    }

    // Transcribe system segments (stereo only)
    if is_stereo {
        for (i, segment) in sys_processable.iter().enumerate() {
            if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
                return Err(anyhow!("Retranscription cancelled"));
            }

            // Progress: 55-80% for system
            let progress = 55 + ((i as f32 / sys_count.max(1) as f32) * 25.0) as u32;
            let segment_duration_sec = (segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0;
            emit_progress(
                &app,
                &meeting_id,
                "transcribing",
                progress,
                &format!("Transcribing system segment {} of {} ({:.1}s)...", i + 1, sys_count, segment_duration_sec),
            );

            if segment.samples.len() < 1600 {
                debug!("Skipping short system segment {} with {} samples", i, segment.samples.len());
                continue;
            }

            let (text, conf) = transcribe_segment(
                segment,
                &whisper_engine,
                &parakeet_engine,
                use_parakeet,
                language.clone(),
            ).await?;

            let trimmed = text.trim();
            if !trimmed.is_empty() {
                debug!("System segment {}/{}: {:.1}s, conf={:.2}, text='{}'", i + 1, sys_count, segment_duration_sec, conf,
                    if trimmed.len() > 80 { &trimmed[..80] } else { trimmed });
                sys_transcripts.push((text, segment.start_timestamp_ms, segment.end_timestamp_ms));
                total_confidence += conf;
                transcribed_count += 1;
            }
        }
    }

    let avg_confidence = if transcribed_count > 0 {
        total_confidence / transcribed_count as f32
    } else {
        0.0
    };

    info!(
        "Transcription complete: {} segments transcribed out of {} (mic: {}, system: {}), avg confidence: {:.2}",
        transcribed_count, total_processable, mic_transcripts.len(), sys_transcripts.len(), avg_confidence
    );

    // Check for cancellation
    if RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst) {
        return Err(anyhow!("Retranscription cancelled"));
    }

    emit_progress(&app, &meeting_id, "saving", 80, "Saving transcripts...");

    // Create transcript segments with source_device labels
    let mut mic_segments = create_transcript_segments_with_source(
        &mic_transcripts,
        Some("Microphone".to_string()),
    );
    let mut sys_segments = create_transcript_segments_with_source(
        &sys_transcripts,
        Some("System".to_string()),
    );

    // For mono, use None source_device
    if !is_stereo {
        mic_segments = create_transcript_segments(&mic_transcripts);
        sys_segments.clear();
    }

    // Merge and sort by audio_start_time
    let mut segments: Vec<TranscriptSegment> = mic_segments;
    segments.extend(sys_segments);
    segments.sort_by(|a, b| {
        let a_time = a.audio_start_time.unwrap_or(0.0);
        let b_time = b.audio_start_time.unwrap_or(0.0);
        a_time.partial_cmp(&b_time).unwrap_or(std::cmp::Ordering::Equal)
    });

    info!("Merged and sorted {} transcript segments", segments.len());

    // Save to database
    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow!("App state not available"))?;

    // Wrap delete+insert+update in a transaction to prevent data loss
    let pool = app_state.db_manager.pool();
    let mut conn = pool.acquire().await.map_err(|e| anyhow!("DB error: {}", e))?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|e| anyhow!("Failed to start transaction: {}", e))?;

    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(&meeting_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to delete existing transcripts: {}", e))?;

    for segment in &segments {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, source_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&segment.id)
        .bind(&meeting_id)
        .bind(&segment.text)
        .bind(&segment.timestamp)
        .bind(segment.audio_start_time)
        .bind(segment.audio_end_time)
        .bind(segment.duration)
        .bind(&segment.source_device)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("Failed to insert transcript: {}", e))?;
    }

    tx.commit().await
        .map_err(|e| anyhow!("Failed to commit transaction: {}", e))?;

    info!(
        "Updated {} transcripts for meeting {} in transaction",
        segments.len(),
        meeting_id
    );

    // Write updated transcripts.json and metadata.json to the meeting folder
    emit_progress(&app, &meeting_id, "saving", 90, "Writing transcript files...");

    if let Err(e) = write_transcripts_json(&folder_path, &segments) {
        warn!("Failed to write transcripts.json: {}", e);
    }

    // Find audio filename for metadata
    let audio_filename = audio_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp4")
        .to_string();

    if let Err(e) = write_retranscription_metadata(
        &folder_path,
        &meeting_id,
        duration_seconds,
        &audio_filename,
    ) {
        warn!("Failed to update metadata.json: {}", e);
    }

    emit_progress(&app, &meeting_id, "complete", 100, "Retranscription complete");

    Ok(RetranscriptionResult {
        meeting_id,
        segments_count: segments.len(),
        duration_seconds,
        language,
    })
}

/// Emit progress event
fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    stage: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "retranscription-progress",
        RetranscriptionProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Get or initialize the Whisper engine, auto-loading the model if needed
/// If `requested_model` is provided, ensures that specific model is loaded
async fn get_or_init_whisper<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<WhisperEngine>> {
    use crate::whisper_engine::commands::WHISPER_ENGINE;

    let engine = {
        let guard = WHISPER_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_whisper_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Whisper model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first (populates the internal cache)
                info!("Discovering available Whisper models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Whisper model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Whisper model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Whisper model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Whisper model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Whisper engine not initialized")),
    }
}

/// Get the configured Whisper model name from the database
async fn get_configured_whisper_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Whisper model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    debug!("Querying transcript_settings table...");

    // Query the transcript settings from the database - get both provider and model
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            // Check if provider is Whisper-based
            if provider == "localWhisper" || provider == "whisper" {
                Ok(model)
            } else {
                error!("Retranscription requires Whisper provider, but configured provider is: {}", provider);
                Err(anyhow!("Retranscription requires Whisper. Current provider '{}' does not support retranscription with language selection.", provider))
            }
        },
        None => {
            // Default to configured Whisper model if no config exists
            warn!("No transcript config found, using default model '{}'", DEFAULT_WHISPER_MODEL);
            Ok(DEFAULT_WHISPER_MODEL.to_string())
        }
    }
}

/// Get or initialize the Parakeet engine, auto-loading the model if needed
async fn get_or_init_parakeet<R: Runtime>(
    app: &AppHandle<R>,
    requested_model: Option<&str>,
) -> Result<Arc<ParakeetEngine>> {
    use crate::parakeet_engine::commands::PARAKEET_ENGINE;

    let engine = {
        let guard = PARAKEET_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().cloned()
    };

    match engine {
        Some(e) => {
            // Determine which model to use
            let target_model = match requested_model {
                Some(model) => model.to_string(),
                None => get_configured_parakeet_model(app).await?,
            };

            // Check if the correct model is already loaded
            let current_model = e.get_current_model().await;
            let needs_load = match &current_model {
                Some(loaded) => loaded != &target_model,
                None => true,
            };

            if needs_load {
                info!(
                    "Loading Parakeet model '{}' (current: {:?})",
                    target_model, current_model
                );

                // Discover available models first
                info!("Discovering available Parakeet models...");
                if let Err(discover_err) = e.discover_models().await {
                    warn!("Error during Parakeet model discovery (continuing anyway): {}", discover_err);
                }

                match e.load_model(&target_model).await {
                    Ok(_) => {
                        info!("Parakeet model '{}' loaded successfully", target_model);
                        Ok(e)
                    }
                    Err(load_err) => {
                        error!("Failed to load Parakeet model '{}': {}", target_model, load_err);
                        Err(anyhow!("Failed to load Parakeet model '{}': {}", target_model, load_err))
                    }
                }
            } else {
                info!("Parakeet model '{}' already loaded", target_model);
                Ok(e)
            }
        }
        None => Err(anyhow!("Parakeet engine not initialized")),
    }
}

/// Get the configured Parakeet model name from the database
async fn get_configured_parakeet_model<R: Runtime>(app: &AppHandle<R>) -> Result<String> {
    debug!("Getting configured Parakeet model from database...");

    let app_state = app
        .try_state::<AppState>()
        .ok_or_else(|| {
            error!("App state not available");
            anyhow!("App state not available")
        })?;

    // Query the transcript settings from the database
    let result: Option<(String, String)> = sqlx::query_as(
        "SELECT provider, model FROM transcript_settings WHERE id = '1'"
    )
    .fetch_optional(app_state.db_manager.pool())
    .await
    .map_err(|e| {
        error!("Failed to query transcript config: {}", e);
        anyhow!("Failed to query transcript config: {}", e)
    })?;

    match result {
        Some((provider, model)) => {
            info!("Found transcript config: provider={}, model={}", provider, model);

            if provider == "parakeet" {
                Ok(model)
            } else {
                // Default to configured Parakeet model
                warn!("Configured provider is not Parakeet, using default model");
                Ok(DEFAULT_PARAKEET_MODEL.to_string())
            }
        },
        None => {
            // Default to configured Parakeet model if no config exists
            warn!("No transcript config found, using default Parakeet model");
            Ok(DEFAULT_PARAKEET_MODEL.to_string())
        }
    }
}

/// Write or update metadata.json for retranscription (preserves existing fields, adds retranscribed_at)
fn write_retranscription_metadata(
    folder: &Path,
    meeting_id: &str,
    duration_seconds: f64,
    audio_filename: &str,
) -> Result<()> {
    let metadata_path = folder.join("metadata.json");
    let temp_path = folder.join(".metadata.json.tmp");
    let now = chrono::Utc::now().to_rfc3339();

    // Try to read existing metadata and update it
    let json = if metadata_path.exists() {
        let existing = std::fs::read_to_string(&metadata_path)?;
        let mut value: serde_json::Value = serde_json::from_str(&existing)?;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("retranscribed_at".to_string(), serde_json::json!(now));
            obj.insert("status".to_string(), serde_json::json!("completed"));
            obj.insert("transcript_file".to_string(), serde_json::json!("transcripts.json"));
            obj.remove("detected_summary_language");
        }
        value
    } else {
        serde_json::json!({
            "version": "1.0",
            "meeting_id": meeting_id,
            "created_at": now,
            "completed_at": now,
            "retranscribed_at": now,
            "duration_seconds": duration_seconds,
            "audio_file": audio_filename,
            "transcript_file": "transcripts.json",
            "status": "completed",
            "source": "retranscription"
        })
    };

    let json_string = serde_json::to_string_pretty(&json)?;
    std::fs::write(&temp_path, &json_string)?;
    std::fs::rename(&temp_path, &metadata_path)?;

    info!("Wrote metadata.json to {}", metadata_path.display());
    Ok(())
}

// Tauri commands

/// Response when retranscription is started
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetranscriptionStarted {
    pub meeting_id: String,
    pub message: String,
}

// Start retranscription (Beta gated using configContext.betaFeatures)
#[tauri::command]
pub async fn start_retranscription_command<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    meeting_folder_path: String,
    language: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<RetranscriptionStarted, String> {

    // Check if retranscription is already in progress (guard will be acquired in start_retranscription)
    if RETRANSCRIPTION_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("Retranscription already in progress".to_string());
    }

    // Clone values for the spawned task
    let meeting_id_clone = meeting_id.clone();

    // Spawn the retranscription in a background task
    tauri::async_runtime::spawn(async move {
        let result = start_retranscription(
            app,
            meeting_id_clone,
            meeting_folder_path,
            language,
            model,
            provider,
        )
        .await;

        // Errors are already emitted as events in start_retranscription
        // so we just log here for debugging
        if let Err(e) = result {
            error!("Retranscription failed: {}", e);
        }
    });

    Ok(RetranscriptionStarted {
        meeting_id,
        message: "Retranscription started".to_string(),
    })
}

#[tauri::command]
pub async fn cancel_retranscription_command() -> Result<(), String> {
    if !is_retranscription_in_progress() {
        return Err("No retranscription in progress".to_string());
    }
    cancel_retranscription();
    Ok(())
}

#[tauri::command]
pub async fn is_retranscription_in_progress_command() -> bool {
    is_retranscription_in_progress()
}

/// Resample a single channel to 16kHz mono format for VAD and transcription.
/// Reuses the same normalization and resampling logic as DecodedAudio::to_whisper_format.
fn resample_channel_to_16k(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    use crate::audio::decoder::normalize_audio_samples;
    use crate::audio::audio_processing::resample_audio;

    const WHISPER_SAMPLE_RATE: u32 = 16000;

    // Normalize samples to valid range
    let normalized = normalize_audio_samples(samples.to_vec());

    // Resample to 16kHz if needed
    if sample_rate != WHISPER_SAMPLE_RATE {
        let mut resampled = resample_audio(&normalized, sample_rate, WHISPER_SAMPLE_RATE);
        // Clamp after resampling (Gibbs phenomenon)
        for s in &mut resampled {
            *s = s.clamp(-1.0, 1.0);
        }
        resampled
    } else {
        normalized
    }
}

/// Transcribe a single speech segment using the configured engine.
async fn transcribe_segment(
    segment: &crate::audio::vad::SpeechSegment,
    whisper_engine: &Option<Arc<WhisperEngine>>,
    parakeet_engine: &Option<Arc<ParakeetEngine>>,
    use_parakeet: bool,
    language: Option<String>,
) -> Result<(String, f32)> {
    if use_parakeet {
        let engine = parakeet_engine.as_ref().unwrap();
        let text = engine
            .transcribe_audio(segment.samples.clone())
            .await
            .map_err(|e| anyhow!("Parakeet transcription failed: {}", e))?;
        Ok((text, 0.9f32))
    } else {
        let engine = whisper_engine.as_ref().unwrap();
        let (text, conf, _) = engine
            .transcribe_audio_with_confidence(segment.samples.clone(), language, None)
            .await
            .map_err(|e| anyhow!("Whisper transcription failed: {}", e))?;
        Ok((text, conf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::constants::AUDIO_EXTENSIONS;

    #[test]
    fn test_create_transcript_segments_empty() {
        let transcripts: Vec<(String, f64, f64)> = vec![];
        let segments = create_transcript_segments(&transcripts);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_create_transcript_segments_single() {
        let transcripts = vec![
            ("Hello world".to_string(), 0.0, 1500.0), // 0-1.5 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello world");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(1.5));
        assert_eq!(segments[0].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_multiple() {
        let transcripts = vec![
            ("First segment".to_string(), 0.0, 2000.0),      // 0-2 seconds
            ("Second segment".to_string(), 3000.0, 5000.0),  // 3-5 seconds
            ("Third segment".to_string(), 6500.0, 8000.0),   // 6.5-8 seconds
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 3);

        // First segment
        assert_eq!(segments[0].text, "First segment");
        assert_eq!(segments[0].audio_start_time, Some(0.0));
        assert_eq!(segments[0].audio_end_time, Some(2.0));
        assert_eq!(segments[0].duration, Some(2.0));

        // Second segment
        assert_eq!(segments[1].text, "Second segment");
        assert_eq!(segments[1].audio_start_time, Some(3.0));
        assert_eq!(segments[1].audio_end_time, Some(5.0));
        assert_eq!(segments[1].duration, Some(2.0));

        // Third segment
        assert_eq!(segments[2].text, "Third segment");
        assert_eq!(segments[2].audio_start_time, Some(6.5));
        assert_eq!(segments[2].audio_end_time, Some(8.0));
        assert_eq!(segments[2].duration, Some(1.5));
    }

    #[test]
    fn test_create_transcript_segments_trims_whitespace() {
        let transcripts = vec![
            ("  Hello with spaces  ".to_string(), 0.0, 1000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Hello with spaces");
    }

    #[test]
    fn test_create_transcript_segments_generates_unique_ids() {
        let transcripts = vec![
            ("Segment one".to_string(), 0.0, 1000.0),
            ("Segment two".to_string(), 1000.0, 2000.0),
        ];
        let segments = create_transcript_segments(&transcripts);

        assert_eq!(segments.len(), 2);
        assert_ne!(segments[0].id, segments[1].id);
        assert!(segments[0].id.starts_with("transcript-"));
        assert!(segments[1].id.starts_with("transcript-"));
    }

    #[test]
    fn test_cancellation_flag() {
        // Reset flag to known state
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
        RETRANSCRIPTION_IN_PROGRESS.store(false, Ordering::SeqCst);

        assert!(!is_retranscription_in_progress());

        // Test cancellation
        cancel_retranscription();
        assert!(RETRANSCRIPTION_CANCELLED.load(Ordering::SeqCst));

        // Reset for other tests
        RETRANSCRIPTION_CANCELLED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_vad_redemption_time_constant() {
        // Batch config uses 200ms core VAD redemption, combined with
        // merge_segments at 2000ms gap threshold for batch processing
        let config = VadConfig::batch();
        assert_eq!(config.redemption_ms, 200);
        assert_eq!(config.max_segment_samples, Some(25 * 16000));
    }

    #[test]
    fn test_find_audio_file_common_candidates() {
        let dir = tempfile::tempdir().unwrap();

        // No audio file → error
        assert!(find_audio_file(dir.path()).is_err());

        // Create audio.mp4 — should be found first
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_non_mp4_extensions() {
        let dir = tempfile::tempdir().unwrap();

        // Create audio.wav (imported as .wav, not .mp4)
        std::fs::write(dir.path().join("audio.wav"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.wav");
    }

    #[test]
    fn test_find_audio_file_fallback_scan() {
        let dir = tempfile::tempdir().unwrap();

        // Create a file with an audio extension but non-standard name
        std::fs::write(dir.path().join("my_recording.flac"), b"fake").unwrap();
        // Also add a non-audio file that should be ignored
        std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();

        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "my_recording.flac");
    }

    #[test]
    fn test_find_audio_file_priority_order() {
        let dir = tempfile::tempdir().unwrap();

        // Create both audio.m4a and audio.mp4 — mp4 should win (listed first in candidates)
        std::fs::write(dir.path().join("audio.m4a"), b"fake").unwrap();
        std::fs::write(dir.path().join("audio.mp4"), b"fake").unwrap();
        let found = find_audio_file(dir.path()).unwrap();
        assert_eq!(found.file_name().unwrap(), "audio.mp4");
    }

    #[test]
    fn test_find_audio_file_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_audio_file(dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No audio file found"));
    }

    #[test]
    fn test_find_audio_file_nonexistent_folder() {
        let result = find_audio_file(Path::new("/nonexistent/path/12345"));
        assert!(result.is_err());
    }

    #[test]
    fn test_audio_extensions_constant() {
        // Verify all expected formats are covered
        assert!(AUDIO_EXTENSIONS.contains(&"mp4"));
        assert!(AUDIO_EXTENSIONS.contains(&"m4a"));
        assert!(AUDIO_EXTENSIONS.contains(&"wav"));
        assert!(AUDIO_EXTENSIONS.contains(&"mp3"));
        assert!(AUDIO_EXTENSIONS.contains(&"flac"));
        assert!(AUDIO_EXTENSIONS.contains(&"ogg"));
        assert!(AUDIO_EXTENSIONS.contains(&"aac"));
        // FFmpeg-backed formats
        assert!(AUDIO_EXTENSIONS.contains(&"mkv"));
        assert!(AUDIO_EXTENSIONS.contains(&"webm"));
        assert!(AUDIO_EXTENSIONS.contains(&"wma"));
        // Non-audio formats
        assert!(!AUDIO_EXTENSIONS.contains(&"txt"));
        assert!(!AUDIO_EXTENSIONS.contains(&"pdf"));
    }
}
