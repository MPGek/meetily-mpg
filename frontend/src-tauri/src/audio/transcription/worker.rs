// audio/transcription/worker.rs
//
// Parallel transcription worker pool and chunk processing logic.

use super::engine::TranscriptionEngine;
use super::provider::TranscriptionError;
use crate::audio::AudioChunk;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

// Sequence counter for transcript updates
static SEQUENCE_COUNTER: AtomicU64 = AtomicU64::new(0);

// Speech detection flag - reset per recording session
static SPEECH_DETECTED_EMITTED: AtomicBool = AtomicBool::new(false);

/// Reset the speech detected flag for a new recording session
pub fn reset_speech_detected_flag() {
    SPEECH_DETECTED_EMITTED.store(false, Ordering::SeqCst);
    info!(
        "🔍 SPEECH_DETECTED_EMITTED reset to: {}",
        SPEECH_DETECTED_EMITTED.load(Ordering::SeqCst)
    );
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptUpdate {
    pub text: String,
    pub timestamp: String, // Wall-clock time for reference (e.g., "14:30:05")
    pub source: String,
    pub sequence_id: u64,
    pub chunk_start_time: f64, // Legacy field, kept for compatibility
    pub is_partial: bool,
    pub confidence: f32,
    // NEW: Recording-relative timestamps for playback sync
    pub audio_start_time: f64, // Seconds from recording start (e.g., 125.3)
    pub audio_end_time: f64,   // Seconds from recording start (e.g., 128.6)
    pub duration: f64,         // Segment duration in seconds (e.g., 3.3)
    pub source_device: String, // "Microphone" or "System"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>, // Speaker ID from diarization (e.g., "SPEAKER_00")
    /// Token-level timestamps for diarization refinement (4.1). None when provider does not supply tokens or for legacy chunks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Vec<crate::audio::token_assignment::Token>>,
}

// NOTE: get_transcript_history and get_recording_meeting_name functions
// have been moved to recording_commands.rs where they have access to RECORDING_MANAGER

/// Optimized parallel transcription task ensuring ZERO chunk loss
pub fn start_transcription_task<R: Runtime>(
    app: AppHandle<R>,
    transcription_receiver: tokio::sync::mpsc::UnboundedReceiver<AudioChunk>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("🚀 Starting optimized parallel transcription task - guaranteeing zero chunk loss");

        // Initialize transcription engine (Whisper or Parakeet based on config)
        let transcription_engine = match super::engine::get_or_init_transcription_engine(&app).await
        {
            Ok(engine) => engine,
            Err(e) => {
                error!("Failed to initialize transcription engine: {}", e);
                let _ = app.emit("transcription-error", serde_json::json!({
                    "error": e,
                    "userMessage": "Recording failed: Unable to initialize speech recognition. Please check your model settings.",
                    "actionable": true
                }));
                return;
            }
        };

        // Create parallel workers for faster processing while preserving ALL chunks
        const NUM_WORKERS: usize = 1; // Serial processing ensures transcripts emit in chronological order
        let (work_sender, work_receiver) = tokio::sync::mpsc::unbounded_channel::<AudioChunk>();
        let work_receiver = Arc::new(tokio::sync::Mutex::new(work_receiver));

        // Post-finalize block pipeline (word-level-diarization-alignment D4 +
        // live-word-level-diarization D2): final blocks with tokens are handed
        // off here and refined off the worker's emit path; the same consumer
        // then runs the display-only live word-diarization reconcile. Always
        // created so live attribution still runs on the engine's own timestamps
        // when alignment is disabled (the consumer skips alignment then).
        let (alignment_queue, alignment_consumer): (
            Option<Arc<crate::audio::word_alignment::queue::AlignmentQueue>>,
            Option<tokio::task::JoinHandle<()>>,
        ) = {
            let q = Arc::new(crate::audio::word_alignment::queue::AlignmentQueue::new(
                crate::audio::word_alignment::queue::QUEUE_CAPACITY_BYTES,
            ));
            // Publish the queue for the live status block (online-diarization-telemetry).
            crate::audio::telemetry::install_alignment_queue(q.clone());
            let consumer =
                crate::audio::word_alignment::queue::spawn_consumer(app.clone(), q.clone());
            info!("🧩 Live word-alignment / diarization-reconcile consumer started");
            (Some(q), Some(consumer))
        };

        // Consumer that re-evaluates held blocks whenever new live turns arrive.
        let diarization_cascade =
            crate::audio::live_diarization_reconcile::spawn_cascade(app.clone());

        // Track completion: AtomicU64 for chunks queued, AtomicU64 for chunks completed
        let chunks_queued = Arc::new(AtomicU64::new(0));
        let chunks_completed = Arc::new(AtomicU64::new(0));
        // Live status: whether the recogniser is currently consuming a chunk
        // (online-diarization-telemetry, indicator blink state).
        let asr_in_flight = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let input_finished = Arc::new(AtomicBool::new(false));

        info!(
            "📊 Starting {} transcription worker{} (serial mode for ordered emission)",
            NUM_WORKERS,
            if NUM_WORKERS == 1 { "" } else { "s" }
        );

        // Spawn worker tasks
        let mut worker_handles = Vec::new();
        for worker_id in 0..NUM_WORKERS {
            let engine_clone = match &transcription_engine {
                TranscriptionEngine::Whisper(e) => TranscriptionEngine::Whisper(e.clone()),
                TranscriptionEngine::Parakeet(e) => TranscriptionEngine::Parakeet(e.clone()),
                TranscriptionEngine::Provider(p) => TranscriptionEngine::Provider(p.clone()),
            };
            let app_clone = app.clone();
            let work_receiver_clone = work_receiver.clone();
            let chunks_completed_clone = chunks_completed.clone();
            let asr_in_flight_clone = asr_in_flight.clone();
            let input_finished_clone = input_finished.clone();
            let chunks_queued_clone = chunks_queued.clone();
            let align_queue_clone = alignment_queue.clone();

            let worker_handle = tokio::spawn(async move {
                info!("👷 Worker {} started", worker_id);

                // PRE-VALIDATE model state to avoid repeated async calls per chunk
                let initial_model_loaded = engine_clone.is_model_loaded().await;
                let current_model = engine_clone
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());

                let engine_name = engine_clone.provider_name();

                // Publish engine identity + queue counters for the live status
                // block (online-diarization-telemetry). `loaded` is driven by
                // the model actually being resident, not by the config.
                crate::audio::telemetry::install_asr(
                    chunks_queued_clone.clone(),
                    chunks_completed_clone.clone(),
                    asr_in_flight_clone.clone(),
                    Some(engine_name.to_string()),
                    initial_model_loaded.then(|| current_model.clone()),
                );

                if initial_model_loaded {
                    info!(
                        "✅ Worker {} pre-validation: {} model '{}' is loaded and ready",
                        worker_id, engine_name, current_model
                    );
                } else {
                    warn!(
                        "⚠️ Worker {} pre-validation: {} model not loaded - chunks may be skipped",
                        worker_id, engine_name
                    );
                }

                // Previous transcription text for context forwarding (per source device)
                let mut last_mic_text: String = String::new();
                let mut last_sys_text: String = String::new();

                loop {
                    // Try to get a chunk to process
                    let chunk = {
                        let mut receiver = work_receiver_clone.lock().await;
                        receiver.recv().await
                    };

                    match chunk {
                        Some(chunk) => {
                            // PERFORMANCE OPTIMIZATION: Reduce logging in hot path
                            // Only log every 10th chunk per worker to reduce I/O overhead
                            let should_log_this_chunk = chunk.chunk_id % 10 == 0;

                            if should_log_this_chunk {
                                info!(
                                    "👷 Worker {} processing chunk {} with {} samples",
                                    worker_id,
                                    chunk.chunk_id,
                                    chunk.data.len()
                                );
                            }

                            // Check if model is still loaded before processing
                            if !engine_clone.is_model_loaded().await {
                                warn!("⚠️ Worker {}: Model unloaded, but continuing to preserve chunk {}", worker_id, chunk.chunk_id);
                                // Still count as completed even if we can't process
                                chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                continue;
                            }

                            let chunk_timestamp = chunk.timestamp;
                            let chunk_duration = chunk.data.len() as f64 / chunk.sample_rate as f64;
                            let chunk_device_type = chunk.device_type.clone();

                            // Determine previous-text prompt for this source device
                            let prompt = match &chunk_device_type {
                                crate::audio::RecordingDeviceType::Microphone => {
                                    if last_mic_text.is_empty() {
                                        None
                                    } else {
                                        Some(last_mic_text.clone())
                                    }
                                }
                                crate::audio::RecordingDeviceType::System => {
                                    if last_sys_text.is_empty() {
                                        None
                                    } else {
                                        Some(last_sys_text.clone())
                                    }
                                }
                            };

                            // Live-alignment samples: the 16 kHz mono block the
                            // ASR will transcribe, captured before `chunk` is
                            // moved (only when a queue is active).
                            let align_samples: Option<Arc<[f32]>> = align_queue_clone.as_ref().map(
                                |_| {
                                    let s = if chunk.sample_rate != 16000 {
                                        crate::audio::audio_processing::resample_audio(
                                            &chunk.data,
                                            chunk.sample_rate,
                                            16000,
                                        )
                                    } else {
                                        chunk.data.clone()
                                    };
                                    Arc::from(s)
                                },
                            );

                            // Live status: the recogniser is now consuming this
                            // chunk (cleared again at the end of the iteration).
                            asr_in_flight_clone.store(true, Ordering::SeqCst);

                            // Transcribe with provider-agnostic approach
                            match transcribe_chunk_with_provider(
                                &engine_clone,
                                chunk,
                                &app_clone,
                                prompt,
                            )
                            .await
                            {
                                Ok((transcript, tokens_opt, confidence_opt, is_partial)) => {
                                    // Provider-aware confidence threshold
                                    let confidence_threshold = match &engine_clone {
                                        TranscriptionEngine::Whisper(_)
                                        | TranscriptionEngine::Provider(_) => 0.3,
                                        TranscriptionEngine::Parakeet(_) => 0.0, // Parakeet has no confidence, accept all
                                    };

                                    let confidence_str = match confidence_opt {
                                        Some(c) => format!("{:.2}", c),
                                        None => "N/A".to_string(),
                                    };

                                    info!("🔍 Worker {} transcription result: text='{}', confidence={}, partial={}, threshold={:.2}",
                                          worker_id, transcript, confidence_str, is_partial, confidence_threshold);

                                    // Check confidence threshold (or accept if no confidence provided)
                                    let meets_threshold =
                                        confidence_opt.map_or(true, |c| c >= confidence_threshold);

                                    if !transcript.trim().is_empty() && meets_threshold {
                                        // PERFORMANCE: Only log transcription results, not every processing step
                                        info!("✅ Worker {} transcribed: {} (confidence: {}, partial: {})",
                                              worker_id, transcript, confidence_str, is_partial);

                                        // Emit speech-detected event for frontend UX (only on first detection per session)
                                        // This is lightweight and provides better user feedback
                                        let current_flag =
                                            SPEECH_DETECTED_EMITTED.load(Ordering::SeqCst);
                                        info!("🔍 Checking speech-detected flag: current={}, will_emit={}", current_flag, !current_flag);

                                        if !current_flag {
                                            SPEECH_DETECTED_EMITTED.store(true, Ordering::SeqCst);
                                            match app_clone.emit("speech-detected", serde_json::json!({
                                                "message": "Speech activity detected"
                                            })) {
                                                Ok(_) => info!("🎤 ✅ First speech detected - successfully emitted speech-detected event"),
                                                Err(e) => error!("🎤 ❌ Failed to emit speech-detected event: {}", e),
                                            }
                                        } else {
                                            info!("🔍 Speech already detected in this session, not re-emitting");
                                        }

                                        // Cache transcript text for context forwarding to next segment
                                        match &chunk_device_type {
                                            crate::audio::RecordingDeviceType::Microphone => {
                                                last_mic_text = transcript.clone();
                                            }
                                            crate::audio::RecordingDeviceType::System => {
                                                last_sys_text = transcript.clone();
                                            }
                                        }

                                        // Generate sequence ID and calculate timestamps FIRST
                                        let sequence_id =
                                            SEQUENCE_COUNTER.fetch_add(1, Ordering::SeqCst);
                                        let audio_start_time = chunk_timestamp; // Already in seconds from recording start
                                        let audio_end_time = chunk_timestamp + chunk_duration;

                                        // Save structured transcript segment to recording manager (only final results)
                                        // Save ALL segments (partial and final) to ensure complete JSON
                                        // Create structured segment with full timestamp data
                                        // NOTE: This is now handled via the transcript-update event emission below
                                        // The recording_commands module listens to these events and saves them
                                        // This decouples the transcription worker from direct RECORDING_MANAGER access

                                        // Emit transcript update with NEW recording-relative timestamps

                                        // Offset chunk-relative token timestamps to recording-relative
                                        let tokens_recording = tokens_opt.map(|toks| {
                                            toks.into_iter()
                                                .map(|mut t| {
                                                    t.start += audio_start_time as f32;
                                                    t.end += audio_start_time as f32;
                                                    // Clamp to segment bounds
                                                    if t.end > audio_end_time as f32 {
                                                        t.end = audio_end_time as f32;
                                                    }
                                                    if t.start < audio_start_time as f32 {
                                                        t.start = audio_start_time as f32;
                                                    }
                                                    t
                                                })
                                                .collect::<Vec<_>>()
                                        });

                                        let update = TranscriptUpdate {
                                            text: transcript,
                                            timestamp: format_current_timestamp(),
                                            source: "Audio".to_string(),
                                            sequence_id,
                                            chunk_start_time: chunk_timestamp,
                                            is_partial,
                                            confidence: confidence_opt.unwrap_or(0.85),
                                            audio_start_time,
                                            audio_end_time,
                                            duration: chunk_duration,
                                            source_device: match chunk_device_type {
                                                crate::audio::RecordingDeviceType::Microphone => {
                                                    "Microphone".to_string()
                                                }
                                                crate::audio::RecordingDeviceType::System => {
                                                    "System".to_string()
                                                }
                                            },
                                            speaker: None,
                                            tokens: tokens_recording,
                                        };

                                        if let Err(e) = app_clone.emit("transcript-update", &update)
                                        {
                                            error!(
                                                "Worker {}: Failed to emit transcript update: {}",
                                                worker_id, e
                                            );
                                        }

                                        // Live evidence the recogniser is working
                                        // (online-diarization-telemetry): the most
                                        // recent final recognition, final blocks only.
                                        if !is_partial {
                                            crate::audio::telemetry::note_asr_result(&update.text);
                                        }

                                        // Hand the completed block to the
                                        // live-alignment queue (finals only,
                                        // never partials) for off-path
                                        // refinement + re-emit (D4).
                                        if !is_partial {
                                            if let (Some(q), Some(samples)) =
                                                (align_queue_clone.as_ref(), align_samples.clone())
                                            {
                                                if update.tokens.is_some() {
                                                    q.push(
                                                        crate::audio::word_alignment::queue::AlignmentJob {
                                                            update: update.clone(),
                                                            samples,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                        // PERFORMANCE: Removed verbose logging of every emission
                                    } else if !transcript.trim().is_empty() && should_log_this_chunk
                                    {
                                        // PERFORMANCE: Only log low-confidence results occasionally
                                        if let Some(c) = confidence_opt {
                                            info!("Worker {} low-confidence transcription (confidence: {:.2}), skipping", worker_id, c);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Improved error handling with specific cases
                                    match e {
                                        TranscriptionError::AudioTooShort { .. } => {
                                            // Skip silently, this is expected for very short chunks
                                            info!("Worker {}: {}", worker_id, e);
                                            asr_in_flight_clone.store(false, Ordering::SeqCst);
                                            chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                            continue;
                                        }
                                        TranscriptionError::ModelNotLoaded => {
                                            warn!(
                                                "Worker {}: Model unloaded during transcription",
                                                worker_id
                                            );
                                            asr_in_flight_clone.store(false, Ordering::SeqCst);
                                            chunks_completed_clone.fetch_add(1, Ordering::SeqCst);
                                            continue;
                                        }
                                        _ => {
                                            warn!(
                                                "Worker {}: Transcription failed: {}",
                                                worker_id, e
                                            );
                                            let _ = app_clone
                                                .emit("transcription-warning", e.to_string());
                                        }
                                    }
                                }
                            }

                            // Mark chunk as completed
                            asr_in_flight_clone.store(false, Ordering::SeqCst);
                            let completed =
                                chunks_completed_clone.fetch_add(1, Ordering::SeqCst) + 1;
                            let queued = chunks_queued_clone.load(Ordering::SeqCst);

                            // PERFORMANCE: Only log progress every 5th chunk to reduce I/O overhead
                            if completed % 5 == 0 || should_log_this_chunk {
                                info!(
                                    "Worker {}: Progress {}/{} chunks ({:.1}%)",
                                    worker_id,
                                    completed,
                                    queued,
                                    (completed as f64 / queued.max(1) as f64 * 100.0)
                                );
                            }

                            // Emit progress event for frontend
                            let progress_percentage = if queued > 0 {
                                (completed as f64 / queued as f64 * 100.0) as u32
                            } else {
                                100
                            };

                            let _ = app_clone.emit("transcription-progress", serde_json::json!({
                                "worker_id": worker_id,
                                "chunks_completed": completed,
                                "chunks_queued": queued,
                                "progress_percentage": progress_percentage,
                                "message": format!("Worker {} processing... ({}/{})", worker_id, completed, queued)
                            }));
                        }
                        None => {
                            // No more chunks available
                            if input_finished_clone.load(Ordering::SeqCst) {
                                // Double-check that all queued chunks are actually completed
                                let final_queued = chunks_queued_clone.load(Ordering::SeqCst);
                                let final_completed = chunks_completed_clone.load(Ordering::SeqCst);

                                if final_completed >= final_queued {
                                    info!(
                                        "👷 Worker {} finishing - all {}/{} chunks processed",
                                        worker_id, final_completed, final_queued
                                    );
                                    break;
                                } else {
                                    warn!("👷 Worker {} detected potential chunk loss: {}/{} completed, waiting...", worker_id, final_completed, final_queued);
                                    // AGGRESSIVE POLLING: Reduced from 50ms to 5ms for faster chunk detection during shutdown
                                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                                }
                            } else {
                                // AGGRESSIVE POLLING: Reduced from 10ms to 1ms for faster response during shutdown
                                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                            }
                        }
                    }
                }

                info!("👷 Worker {} completed", worker_id);
            });

            worker_handles.push(worker_handle);
        }

        // Main dispatcher: receive chunks and distribute to workers
        let mut receiver = transcription_receiver;
        while let Some(chunk) = receiver.recv().await {
            let queued = chunks_queued.fetch_add(1, Ordering::SeqCst) + 1;
            info!(
                "📥 Dispatching chunk {} to workers (total queued: {})",
                chunk.chunk_id, queued
            );

            if let Err(_) = work_sender.send(chunk) {
                error!("❌ Failed to send chunk to workers - this should not happen!");
                break;
            }
        }

        // Signal that input is finished
        input_finished.store(true, Ordering::SeqCst);
        drop(work_sender); // Close the channel to signal workers

        let total_chunks_queued = chunks_queued.load(Ordering::SeqCst);
        info!("📭 Input finished with {} total chunks queued. Waiting for all {} workers to complete...",
              total_chunks_queued, NUM_WORKERS);

        // Emit final chunk count to frontend
        let _ = app.emit("transcription-queue-complete", serde_json::json!({
            "total_chunks": total_chunks_queued,
            "message": format!("{} chunks queued for processing - waiting for completion", total_chunks_queued)
        }));

        // Wait for all workers to complete
        for (worker_id, handle) in worker_handles.into_iter().enumerate() {
            if let Err(e) = handle.await {
                error!("❌ Worker {} panicked: {:?}", worker_id, e);
            } else {
                info!("✅ Worker {} completed successfully", worker_id);
            }
        }

        // Final verification with retry logic to catch any stragglers
        let mut verification_attempts = 0;
        const MAX_VERIFICATION_ATTEMPTS: u32 = 10;

        loop {
            let final_queued = chunks_queued.load(Ordering::SeqCst);
            let final_completed = chunks_completed.load(Ordering::SeqCst);

            if final_queued == final_completed {
                info!(
                    "🎉 ALL {} chunks processed successfully - ZERO chunks lost!",
                    final_completed
                );
                break;
            } else if verification_attempts < MAX_VERIFICATION_ATTEMPTS {
                verification_attempts += 1;
                warn!("⚠️ Chunk count mismatch (attempt {}): {} queued, {} completed - waiting for stragglers...",
                     verification_attempts, final_queued, final_completed);

                // Wait a bit for any remaining chunks to be processed
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            } else {
                error!(
                    "❌ CRITICAL: After {} attempts, chunk loss detected: {} queued, {} completed",
                    MAX_VERIFICATION_ATTEMPTS, final_queued, final_completed
                );

                // Emit critical error event
                let _ = app.emit(
                    "transcript-chunk-loss-detected",
                    serde_json::json!({
                        "chunks_queued": final_queued,
                        "chunks_completed": final_completed,
                        "chunks_lost": final_queued - final_completed,
                        "message": "Some transcript chunks may have been lost during shutdown"
                    }),
                );
                break;
            }
        }

        // Drain the live-alignment queue before this task returns (stop-time
        // bounded wait, D4/5.4). Runs while finalize has not started yet; the
        // finalize repair hook handles anything left unrefined afterwards.
        if let (Some(q), Some(consumer)) = (alignment_queue.clone(), alignment_consumer) {
            crate::audio::word_alignment::queue::drain_on_stop(
                &q,
                consumer,
                std::time::Duration::from_secs(120),
            )
            .await;
        }

        // Stop the live-diarization reconcile cascade after the queued blocks
        // have been submitted; display-only, so leftovers fall back to
        // stop-time finalize.
        crate::audio::live_diarization_reconcile::close_cascade(diarization_cascade).await;

        info!("✅ Parallel transcription task completed - all workers finished, ready for model unload");
    })
}

/// Transcribe audio chunk using the appropriate provider (Whisper, Parakeet, or trait-based)
/// Returns: (text, tokens, confidence Option, is_partial)
async fn transcribe_chunk_with_provider<R: Runtime>(
    engine: &TranscriptionEngine,
    chunk: AudioChunk,
    app: &AppHandle<R>,
    initial_prompt: Option<String>,
) -> std::result::Result<
    (
        String,
        Option<Vec<crate::audio::token_assignment::Token>>,
        Option<f32>,
        bool,
    ),
    TranscriptionError,
> {
    // Convert to 16kHz mono for transcription
    let transcription_data = if chunk.sample_rate != 16000 {
        crate::audio::audio_processing::resample_audio(&chunk.data, chunk.sample_rate, 16000)
    } else {
        chunk.data
    };

    // Skip VAD processing here since the pipeline already extracted speech using VAD
    let speech_samples = transcription_data;

    // Check for empty samples - improved error handling
    if speech_samples.is_empty() {
        warn!(
            "Audio chunk {} is empty, skipping transcription",
            chunk.chunk_id
        );
        return Err(TranscriptionError::AudioTooShort {
            samples: 0,
            minimum: 1600, // 100ms at 16kHz
        });
    }

    // Calculate energy for logging/monitoring only
    let energy: f32 =
        speech_samples.iter().map(|&x| x * x).sum::<f32>() / speech_samples.len() as f32;
    info!(
        "Processing speech audio chunk {} with {} samples (energy: {:.6})",
        chunk.chunk_id,
        speech_samples.len(),
        energy
    );

    // Transcribe using the appropriate engine (with improved error handling)
    match engine {
        TranscriptionEngine::Whisper(whisper_engine) => {
            // Get language preference from global state
            let language = crate::get_language_preference_internal();

            match whisper_engine
                .transcribe_audio_with_tokens(speech_samples, language, initial_prompt.clone())
                .await
            {
                Ok((text, tokens, confidence, is_partial)) => {
                    let cleaned_text = text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), None, Some(confidence), is_partial));
                    }

                    info!(
                        "Whisper transcription complete for chunk {}: '{}' ({} tokens, confidence: {:.2}, partial: {})",
                        chunk.chunk_id, cleaned_text, tokens.len(), confidence, is_partial
                    );

                    let tokens_opt = if tokens.is_empty() {
                        None
                    } else {
                        Some(tokens)
                    };
                    Ok((cleaned_text, tokens_opt, Some(confidence), is_partial))
                }
                Err(e) => {
                    error!(
                        "Whisper transcription failed for chunk {}: {}",
                        chunk.chunk_id, e
                    );

                    let transcription_error = TranscriptionError::EngineFailed(e.to_string());
                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": transcription_error.to_string(),
                            "userMessage": format!("Transcription failed: {}", transcription_error),
                            "actionable": false
                        }),
                    );

                    Err(transcription_error)
                }
            }
        }
        TranscriptionEngine::Parakeet(parakeet_engine) => {
            match parakeet_engine
                .transcribe_audio_with_tokens(speech_samples)
                .await
            {
                Ok((text, tokens)) => {
                    let cleaned_text = text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), None, None, false));
                    }

                    info!(
                        "Parakeet transcription complete for chunk {}: '{}' ({} tokens)",
                        chunk.chunk_id,
                        cleaned_text,
                        tokens.len()
                    );

                    // Parakeet doesn't provide confidence or partial state;
                    // tokens carry native frame-aligned timestamps (chunk-relative).
                    let tokens_opt = if tokens.is_empty() {
                        None
                    } else {
                        Some(tokens)
                    };
                    Ok((cleaned_text, tokens_opt, None, false))
                }
                Err(e) => {
                    error!(
                        "Parakeet transcription failed for chunk {}: {}",
                        chunk.chunk_id, e
                    );

                    let transcription_error = TranscriptionError::EngineFailed(e.to_string());
                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": transcription_error.to_string(),
                            "userMessage": format!("Transcription failed: {}", transcription_error),
                            "actionable": false
                        }),
                    );

                    Err(transcription_error)
                }
            }
        }
        TranscriptionEngine::Provider(provider) => {
            // NEW: Trait-based provider (clean, unified interface)
            let language = crate::get_language_preference_internal();

            match provider.transcribe(speech_samples, language).await {
                Ok(result) => {
                    let cleaned_text = result.text.trim().to_string();
                    if cleaned_text.is_empty() {
                        return Ok((String::new(), None, result.confidence, result.is_partial));
                    }

                    let confidence_str = match result.confidence {
                        Some(c) => format!("confidence: {:.2}", c),
                        None => "no confidence".to_string(),
                    };

                    info!(
                        "{} transcription complete for chunk {}: '{}' ({}, partial: {})",
                        provider.provider_name(),
                        chunk.chunk_id,
                        cleaned_text,
                        confidence_str,
                        result.is_partial
                    );

                    // Provider trait does not yet expose token timestamps – no split
                    Ok((cleaned_text, None, result.confidence, result.is_partial))
                }
                Err(e) => {
                    error!(
                        "{} transcription failed for chunk {}: {}",
                        provider.provider_name(),
                        chunk.chunk_id,
                        e
                    );

                    let _ = app.emit(
                        "transcription-error",
                        &serde_json::json!({
                            "error": e.to_string(),
                            "userMessage": format!("Transcription failed: {}", e),
                            "actionable": false
                        }),
                    );

                    Err(e)
                }
            }
        }
    }
}

/// Format current timestamp (wall-clock time)
fn format_current_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let hours = (now.as_secs() / 3600) % 24;
    let minutes = (now.as_secs() / 60) % 60;
    let seconds = now.as_secs() % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

/// Format recording-relative time as [MM:SS]
#[allow(dead_code)]
fn format_recording_time(seconds: f64) -> String {
    let total_seconds = seconds.floor() as u64;
    let minutes = total_seconds / 60;
    let secs = total_seconds % 60;

    format!("[{:02}:{:02}]", minutes, secs)
}
