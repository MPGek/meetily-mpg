// audio/recording_commands.rs
//
// Slim Tauri command layer for recording functionality.
// Delegates to transcription and recording modules for actual implementation.

use anyhow::Result;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::task::JoinHandle;

use super::{
    default_input_device,  // Get default microphone
    default_output_device, // Get default system audio
    parse_audio_device,
    DeviceEvent,
    DeviceMonitorType,
    RecordingManager,
};

use super::online_diarization::{
    DiarizationMode, OnlineClusterEmbeddings, OnlineDiarizationProcessor, PrototypeStore,
    SpeakerAssignment, SpeakerTurn,
};
use crate::database::repositories::speaker::SpeakerRepository;

// Import transcription modules
use super::transcription::{self, reset_speech_detected_flag};

// Re-export TranscriptUpdate for backward compatibility
pub use super::transcription::TranscriptUpdate;

// ============================================================================
// GLOBAL STATE
// ============================================================================

// Simple recording state tracking
static IS_RECORDING: AtomicBool = AtomicBool::new(false);

// Global recording manager and transcription task to keep them alive during recording
static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);
static TRANSCRIPTION_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

// Online diarization worker: consumes embedding chunks and returns the
// processor (for finalize) when the channel closes
static ONLINE_DIARIZATION_TASK: Mutex<
    Option<JoinHandle<Result<Option<OnlineDiarizationProcessor>, String>>>,
> = Mutex::new(None);

// Shared prototype store for live Fast-mode recognition (design D6).
// Created at recording start, shared between the processor task and the
// assign_live_speaker command. Cleared at recording stop.
pub(crate) static ONLINE_DIARIZATION_STORE: Mutex<Option<Arc<RwLock<PrototypeStore>>>> =
    Mutex::new(None);

// Session data retained between recording stop and the frontend-initiated
// finalize_online_session call (which needs the meeting_id created by the
// frontend save). Holds cluster embeddings for persistence + enrollment,
// the raw per-channel chunk buffers for ground-truth block enrollment, and
// the expected-speaker list for the meeting row.
pub(crate) struct OnlineSessionData {
    pub cluster_embeddings: OnlineClusterEmbeddings,
    pub live_bindings: std::collections::HashMap<String, String>,
    pub expected_speaker_ids: Vec<String>,
    /// Raw timestamped mic-channel chunk embeddings (`(start, end, embedding)`),
    /// for enrolling user-assigned blocks as ground truth.
    pub mic_embeddings: Vec<(f32, f32, Vec<f32>)>,
    /// Raw timestamped system-channel chunk embeddings.
    pub sys_embeddings: Vec<(f32, f32, Vec<f32>)>,
}

pub(crate) static ONLINE_SESSION_DATA: Mutex<Option<OnlineSessionData>> = Mutex::new(None);

// Expected speaker IDs passed at recording start, stored so stop_recording
// can include them in the session data for finalize_online_session.
static ONLINE_EXPECTED_SPEAKER_IDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A single-turn (single-block) speaker override recorded during Fast-mode
/// recording (design D10): relabels the transcript(s) overlapping
/// [start_secs, end_secs] of the given cluster to a registry speaker.
/// Display-only during recording; applied at stop-time finalize.
#[derive(Debug, Clone)]
pub(crate) struct TurnOverride {
    pub cluster_label: String,
    pub start_secs: f64,
    pub end_secs: f64,
    pub speaker_id: String,
}

// Per-turn overrides recorded mid-recording (scope='block' in
// assign_live_speaker). Consumed and cleared by finalize_online_session, which
// applies them to the meeting's transcripts after the meeting row exists.
pub(crate) static ONLINE_TURN_OVERRIDES: Mutex<Vec<TurnOverride>> = Mutex::new(Vec::new());

// Listener ID for proper cleanup - prevents microphone from staying active after recording stops
static TRANSCRIPT_LISTENER_ID: Mutex<Option<tauri::EventId>> = Mutex::new(None);

// Shared transcript segments and meeting folder for event listener access.
// These bypass RecordingManager to avoid cross-thread access to !Send types (cpal::Stream).
static SHARED_SEGMENTS: Mutex<Option<Arc<Mutex<Vec<super::recording_saver::TranscriptSegment>>>>> =
    Mutex::new(None);
static SHARED_FOLDER: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

// ============================================================================
// PUBLIC TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RecordingArgs {
    pub save_path: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct TranscriptionStatus {
    pub chunks_in_queue: usize,
    pub is_processing: bool,
    pub last_activity_ms: u64,
}

/// Map a `TranscriptUpdate` event payload onto the shared buffered segment.
/// Used by both transcript-update listeners so word-level tokens always
/// propagate into `SHARED_SEGMENTS` / `transcripts.json` (word-level-diarization-alignment D6).
fn transcript_segment_from_update(
    update: &TranscriptUpdate,
) -> super::recording_saver::TranscriptSegment {
    super::recording_saver::TranscriptSegment {
        id: format!("seg_{}", update.sequence_id),
        text: update.text.clone(),
        audio_start_time: update.audio_start_time,
        audio_end_time: update.audio_end_time,
        duration: update.duration,
        display_time: update.timestamp.clone(),
        confidence: update.confidence,
        sequence_id: update.sequence_id,
        source_device: update.source_device.clone(),
        tokens: update.tokens.clone(),
    }
}

/// Resolve a repair-path span source over a meeting's saved audio file
/// (word-level-diarization-alignment 5.6). The mic/system channel mapping is
/// baked into the returned source. `None` when the file/ffmpeg is unavailable.
fn meeting_span_source(
    folder: &std::path::Path,
) -> Option<Box<dyn crate::audio::word_alignment::refine::AudioSpanSource>> {
    use crate::audio::word_alignment::refine::FileSpanSource;
    let audio_path = match crate::audio::audio_file::find_audio_file(folder) {
        Ok(p) => p,
        Err(e) => {
            warn!("Alignment repair: no audio file in meeting folder: {}", e);
            return None;
        }
    };
    // Resolve the layout from the decoded audio (first-packet fallback when the
    // container omits the channel count) so spans are read from the same channel
    // the segment was transcribed from.
    let layout = match crate::audio::decoder::detect_channel_layout(&audio_path) {
        Ok(layout) => layout,
        Err(e) => {
            warn!(
                "Alignment repair: channel layout detection failed for {}: {}",
                audio_path.display(),
                e
            );
            crate::audio::decoder::ChannelLayout::Unknown
        }
    };
    if layout.channels().is_none() {
        warn!(
            "Alignment repair: channel layout unknown for {}; treating spans as mono",
            audio_path.display()
        );
    }
    let stereo = layout.is_stereo();
    match FileSpanSource::new(audio_path, stereo) {
        Ok(s) => Some(Box::new(s)),
        Err(e) => {
            warn!("Alignment repair: span source init failed: {}", e);
            None
        }
    }
}

// ============================================================================
// RECORDING COMMANDS
// ============================================================================

/// Start recording with default devices
pub async fn start_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    start_recording_with_meeting_name(app, None).await
}

/// Start recording with default devices and optional meeting name
pub async fn start_recording_with_meeting_name<R: Runtime>(
    app: AppHandle<R>,
    meeting_name: Option<String>,
) -> Result<(), String> {
    info!(
        "Starting recording with default devices, meeting: {:?}",
        meeting_name
    );

    let engine_lifecycle_guard = super::common::acquire_engine_lifecycle_lock().await;

    // Check if already recording
    let current_recording_state = IS_RECORDING.load(Ordering::SeqCst);
    info!("🔍 IS_RECORDING state check: {}", current_recording_state);
    if current_recording_state {
        return Err("Recording already in progress".to_string());
    }

    // Validate that transcription models are available before starting recording
    info!("🔍 Validating transcription model availability before starting recording...");
    if let Err(validation_error) = transcription::validate_transcription_model_ready(&app).await {
        error!("Model validation failed: {}", validation_error);

        // Emit error event for frontend - actionable: false to show toast instead of modal
        // (download progress is already shown in top-right toast)
        let _ = app.emit("transcription-error", serde_json::json!({
            "error": validation_error,
            "userMessage": "Recording cannot start: Transcription model is still downloading. Please wait for the download to complete.",
            "actionable": false
        }));

        return Err(validation_error);
    }
    info!("✅ Transcription model validation passed");

    // Async-first approach - no more blocking operations!
    info!("🚀 Starting async recording initialization");

    // Create new recording manager
    let mut manager = RecordingManager::new();

    // Load recording preferences to get auto_save AND device preferences
    let (auto_save, preferred_mic_name, preferred_system_name) =
        match super::recording_preferences::load_recording_preferences(&app).await {
            Ok(prefs) => {
                info!("📋 Loaded recording preferences: auto_save={}, preferred_mic={:?}, preferred_system={:?}",
                      prefs.auto_save, prefs.preferred_mic_device, prefs.preferred_system_device);
                (
                    prefs.auto_save,
                    prefs.preferred_mic_device,
                    prefs.preferred_system_device,
                )
            }
            Err(e) => {
                warn!(
                    "Failed to load recording preferences, using defaults: {}",
                    e
                );
                (true, None, None)
            }
        };

    // ============================================================================
    // MICROPHONE DEVICE RESOLUTION: Preference → Default → Error
    // ============================================================================
    let microphone_device = match preferred_mic_name {
        Some(pref_name) => {
            info!("🎤 Attempting to use preferred microphone: '{}'", pref_name);
            match parse_audio_device(&pref_name) {
                Ok(device) => {
                    info!("✅ Using preferred microphone: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    warn!(
                        "⚠️ Preferred microphone '{}' not available: {}",
                        pref_name, e
                    );
                    warn!("   Falling back to system default microphone...");
                    match default_input_device() {
                        Ok(device) => {
                            info!("✅ Using default microphone: '{}'", device.name);
                            Some(Arc::new(device))
                        }
                        Err(default_err) => {
                            error!(
                                "❌ No microphone available (preferred and default both failed)"
                            );
                            return Err(format!(
                                "No microphone device available. Preferred device '{}' not found, and default microphone unavailable: {}",
                                pref_name, default_err
                            ));
                        }
                    }
                }
            }
        }
        None => {
            info!("🎤 No microphone preference set, using system default");
            match default_input_device() {
                Ok(device) => {
                    info!("✅ Using default microphone: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    error!("❌ No default microphone available");
                    return Err(format!("No microphone device available: {}", e));
                }
            }
        }
    };

    // ============================================================================
    // SYSTEM AUDIO DEVICE RESOLUTION: Preference → Default → None (optional)
    // ============================================================================
    let system_device = match preferred_system_name {
        Some(pref_name) => {
            info!(
                "🔊 Attempting to use preferred system audio: '{}'",
                pref_name
            );
            match parse_audio_device(&pref_name) {
                Ok(device) => {
                    info!("✅ Using preferred system audio: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    warn!(
                        "⚠️ Preferred system audio '{}' not available: {}",
                        pref_name, e
                    );
                    warn!("   Falling back to system default...");
                    match default_output_device() {
                        Ok(device) => {
                            info!("✅ Using default system audio: '{}'", device.name);
                            Some(Arc::new(device))
                        }
                        Err(default_err) => {
                            warn!("⚠️ No system audio available (preferred and default both failed): {}", default_err);
                            warn!("   Recording will continue with microphone only");
                            None // System audio is optional
                        }
                    }
                }
            }
        }
        None => {
            info!("🔊 No system audio preference set, using system default");
            match default_output_device() {
                Ok(device) => {
                    info!("✅ Using default system audio: '{}'", device.name);
                    Some(Arc::new(device))
                }
                Err(e) => {
                    warn!("⚠️ No default system audio available: {}", e);
                    warn!("   Recording will continue with microphone only");
                    None // System audio is optional
                }
            }
        }
    };

    // Always ensure a meeting name is set so incremental saver initializes
    let effective_meeting_name = meeting_name.clone().unwrap_or_else(|| {
        // Example: Meeting 2025-10-03_08-25
        let now = chrono::Local::now();
        format!("Meeting {}", now.format("%Y-%m-%d_%H-%M"))
    });
    manager.set_meeting_name(Some(effective_meeting_name));

    // Set up error callback
    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit("recording-error", error.user_message());
    });

    // Start recording with resolved devices (replaces start_recording_with_defaults_and_auto_save call)
    let transcription_receiver = manager
        .start_recording(microphone_device, system_device, auto_save)
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))?;

    // Store the manager globally to keep it alive
    {
        let mut global_manager = RECORDING_MANAGER.lock().unwrap();
        *global_manager = Some(manager);
    }

    // Extract shared transcript state for the event listener.
    // This avoids cross-thread access to RecordingManager (which contains !Send cpal::Stream).
    {
        let manager_guard = RECORDING_MANAGER.lock().unwrap();
        if let Some(ref mgr) = *manager_guard {
            *SHARED_SEGMENTS.lock().unwrap() = Some(mgr.shared_segments());
            *SHARED_FOLDER.lock().unwrap() = mgr.get_meeting_folder();
        }
    }

    // Set recording flag and reset speech detection flag
    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    IS_RECORDING.store(true, Ordering::SeqCst);
    drop(engine_lifecycle_guard);
    reset_speech_detected_flag(); // Reset for new recording session

    // Start optimized parallel transcription task and store handle
    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK.lock().unwrap();
        *global_task = Some(task_handle);
    }

    // CRITICAL: Listen for transcript-update events and save to shared transcript state.
    // Uses SHARED_SEGMENTS / SHARED_FOLDER to avoid accessing RecordingManager
    // (which contains !Send cpal::Stream types) from the event dispatch thread.
    {
        use tauri::Listener;
        let listener_id = app.listen("transcript-update", move |event: tauri::Event| {
            if let Ok(update) = serde_json::from_str::<TranscriptUpdate>(event.payload()) {
                let segment = transcript_segment_from_update(&update);

                // Write to shared transcript segments (no RecordingManager access)
                if let Ok(segments_guard) = SHARED_SEGMENTS.lock() {
                    if let Some(ref shared) = *segments_guard {
                        if let Ok(mut segs) = shared.lock() {
                            if let Some(existing) = segs
                                .iter_mut()
                                .find(|s| s.sequence_id == segment.sequence_id)
                            {
                                *existing = segment.clone();
                            } else {
                                segs.push(segment.clone());
                            }
                        }
                    }
                }

                // Persist to disk
                if let Ok(folder_guard) = SHARED_FOLDER.lock() {
                    if let Some(ref folder) = *folder_guard {
                        if let Ok(segments_guard) = SHARED_SEGMENTS.lock() {
                            if let Some(ref shared) = *segments_guard {
                                if let Err(e) =
                                    crate::audio::recording_saver::write_transcripts_to_disk(
                                        folder, shared,
                                    )
                                {
                                    warn!("Failed to write incremental transcript update: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        });
        let mut global_listener = TRANSCRIPT_LISTENER_ID.lock().unwrap();
        *global_listener = Some(listener_id);
        info!("✅ Transcript-update event listener registered for history persistence");
    }

    // Emit success event
    app.emit(
        "recording-started",
        serde_json::json!({
            "message": "Recording started successfully with parallel processing",
            "devices": ["Default Microphone", "Default System Audio"],
            "workers": 3
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect recording state
    crate::tray::update_tray_menu(&app);

    info!("✅ Recording started successfully with async-first approach");

    Ok(())
}

/// Start recording with specific devices
pub async fn start_recording_with_devices<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
) -> Result<(), String> {
    start_recording_with_devices_and_meeting(
        app,
        mic_device_name,
        system_device_name,
        None,
        None,
        None,
        None,
    )
    .await
}

/// Start recording with specific devices and optional meeting name
pub async fn start_recording_with_devices_and_meeting<R: Runtime>(
    app: AppHandle<R>,
    mic_device_name: Option<String>,
    system_device_name: Option<String>,
    meeting_name: Option<String>,
    diarization_mode: Option<String>,
    max_speakers: Option<i32>,
    expected_speaker_ids: Option<Vec<String>>,
) -> Result<(), String> {
    info!(
        "Starting recording with specific devices: mic={:?}, system={:?}, meeting={:?}, diarization_mode={:?}, max_speakers={:?}, expected_speakers={:?}",
        mic_device_name, system_device_name, meeting_name, diarization_mode, max_speakers, expected_speaker_ids
    );

    let engine_lifecycle_guard = super::common::acquire_engine_lifecycle_lock().await;

    // Check if already recording
    let current_recording_state = IS_RECORDING.load(Ordering::SeqCst);
    info!("🔍 IS_RECORDING state check: {}", current_recording_state);
    if current_recording_state {
        return Err("Recording already in progress".to_string());
    }

    // Validate that transcription models are available before starting recording
    info!("🔍 Validating transcription model availability before starting recording...");
    if let Err(validation_error) = transcription::validate_transcription_model_ready(&app).await {
        error!("Model validation failed: {}", validation_error);

        // Emit error event for frontend - actionable: false to show toast instead of modal
        // (download progress is already shown in top-right toast)
        let _ = app.emit("transcription-error", serde_json::json!({
            "error": validation_error,
            "userMessage": "Recording cannot start: Transcription model is still downloading. Please wait for the download to complete.",
            "actionable": false
        }));

        return Err(validation_error);
    }
    info!("✅ Transcription model validation passed");

    // Load recording preferences to resolve devices and the auto-save setting.
    // The settings page persists the user's device choices here; they serve as
    // the fallback when the frontend does not send an explicit device name.
    let (auto_save, preferred_mic_name, preferred_system_name) =
        match super::recording_preferences::load_recording_preferences(&app).await {
            Ok(prefs) => {
                info!("📋 Loaded recording preferences: auto_save={}, preferred_mic={:?}, preferred_system={:?}",
                      prefs.auto_save, prefs.preferred_mic_device, prefs.preferred_system_device);
                (
                    prefs.auto_save,
                    prefs.preferred_mic_device,
                    prefs.preferred_system_device,
                )
            }
            Err(e) => {
                warn!(
                    "Failed to load recording preferences, using defaults: {}",
                    e
                );
                (true, None, None)
            }
        };

    // Resolve devices with fallback: explicit name → saved preference → system default.
    // Microphone is required; system audio is optional (skipped only when no
    // default output device exists).
    let mic_device = Some(resolve_microphone_device(
        mic_device_name.as_deref(),
        preferred_mic_name.as_deref(),
    )?);

    let system_device = resolve_system_audio_device(
        system_device_name.as_deref(),
        preferred_system_name.as_deref(),
    );

    // Async-first approach for custom devices - no more blocking operations!
    info!("🚀 Starting async recording initialization with custom devices");

    // Create new recording manager
    let mut manager = RecordingManager::new();

    // Online diarization: create the embedding channel and spawn the consumer.
    // The processor downloads models on first use (blocking) and then drains
    // VAD-filtered speech chunks; it is returned when the channel closes.
    let online_mode = DiarizationMode::parse(diarization_mode.as_deref());
    let expected_ids = expected_speaker_ids.clone().unwrap_or_default();

    // Store expected speaker IDs so stop_recording can include them in session data.
    {
        let mut stored_ids = ONLINE_EXPECTED_SPEAKER_IDS.lock().unwrap();
        *stored_ids = expected_ids.clone();
    }
    if online_mode.is_online() {
        let (embedding_sender, embedding_receiver) =
            tokio::sync::mpsc::unbounded_channel::<super::recording_state::AudioChunk>();
        manager.set_embedding_sender(Some(embedding_sender));

        // Fix the microphone label prefix for the whole session: when a
        // system device was selected the session is stereo, so mic clusters
        // are namespaced MIC_SPEAKER_NN; otherwise mono, so SPEAKER_NN.
        let has_system_device = system_device.is_some();

        // Channel carrying live speaker turns (Fast mode) from the blocking
        // processor back to the async side for emission to the frontend.
        let (turn_sender, mut turn_receiver) =
            tokio::sync::mpsc::unbounded_channel::<SpeakerTurn>();
        let app_for_turns = app.clone();
        let _turn_forwarder = tokio::spawn(async move {
            while let Some(turn) = turn_receiver.recv().await {
                if let Err(e) = app_for_turns.emit("online-speaker-turn", turn) {
                    warn!("Failed to emit online speaker turn: {}", e);
                }
            }
        });

        // Load prototype store for live recognition (Fast mode) and store
        // it in a static so the assign_live_speaker command can access it.
        let candidate_ids = if expected_ids.is_empty() {
            None
        } else {
            Some(expected_ids.clone())
        };
        // Fresh session: clear any stale per-turn overrides from a previous run.
        ONLINE_TURN_OVERRIDES.lock().unwrap().clear();
        let pool = {
            let state = app.state::<crate::state::AppState>();
            state.db_manager.pool().clone()
        };
        let store_model_tag = crate::audio::embedder::ENHANCED_MODEL_TAG;
        let prototype_store = match PrototypeStore::load_with_model(
            &pool,
            candidate_ids,
            has_system_device,
            store_model_tag,
        )
        .await
        {
            Ok(store) => {
                let arc = Arc::new(RwLock::new(store));
                {
                    let mut global_store = ONLINE_DIARIZATION_STORE.lock().unwrap();
                    *global_store = Some(arc.clone());
                }
                Some(arc)
            }
            Err(e) => {
                warn!("Failed to load prototype store: {}", e);
                None
            }
        };

        let app_for_processor = app.clone();
        let app_for_event = app.clone();
        let task = tokio::task::spawn_blocking(
            move || -> Result<Option<OnlineDiarizationProcessor>, String> {
                let max_speakers_usize = max_speakers.filter(|m| *m > 0).unwrap_or(0) as usize;
                let mut processor = match OnlineDiarizationProcessor::new_with_app(
                    &app_for_processor,
                    online_mode,
                    max_speakers_usize,
                    has_system_device,
                    Some(turn_sender),
                    prototype_store,
                ) {
                    Ok(processor) => processor,
                    Err(e) => {
                        warn!("Online diarization unavailable: {}", e);
                        let _ = app_for_event.emit(
                            "online-diarization-unavailable",
                            serde_json::json!({ "error": e }),
                        );
                        return Err(e);
                    }
                };
                let mut receiver = embedding_receiver;
                while let Some(chunk) = receiver.blocking_recv() {
                    processor.process_chunk(chunk);
                }
                Ok(Some(processor))
            },
        );
        {
            let mut global_task = ONLINE_DIARIZATION_TASK.lock().unwrap();
            *global_task = Some(task);
        }
        info!(
            "🎙️ Online diarization processor spawned (mode: {:?})",
            online_mode
        );
    } else {
        info!("ℹ️ Online diarization disabled (mode: {:?})", online_mode);
    }

    // Always ensure a meeting name is set so incremental saver initializes
    let effective_meeting_name = meeting_name.clone().unwrap_or_else(|| {
        let now = chrono::Local::now();
        format!("Meeting {}", now.format("%Y-%m-%d_%H-%M"))
    });
    manager.set_meeting_name(Some(effective_meeting_name));

    // Set up error callback
    let app_for_error = app.clone();
    manager.set_error_callback(move |error| {
        let _ = app_for_error.emit("recording-error", error.user_message());
    });

    // Start recording with specified devices and auto_save setting
    let transcription_receiver = manager
        .start_recording(mic_device, system_device, auto_save)
        .await
        .map_err(|e| format!("Failed to start recording: {}", e))?;

    // Store the manager globally to keep it alive
    {
        let mut global_manager = RECORDING_MANAGER.lock().unwrap();
        *global_manager = Some(manager);
    }

    // Extract shared transcript state for the event listener.
    // This avoids cross-thread access to RecordingManager (which contains !Send cpal::Stream).
    {
        let manager_guard = RECORDING_MANAGER.lock().unwrap();
        if let Some(ref mgr) = *manager_guard {
            *SHARED_SEGMENTS.lock().unwrap() = Some(mgr.shared_segments());
            *SHARED_FOLDER.lock().unwrap() = mgr.get_meeting_folder();
        }
    }

    // Set recording flag and reset speech detection flag
    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    IS_RECORDING.store(true, Ordering::SeqCst);
    drop(engine_lifecycle_guard);
    reset_speech_detected_flag(); // Reset for new recording session

    // Start optimized parallel transcription task and store handle
    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK.lock().unwrap();
        *global_task = Some(task_handle);
    }

    // CRITICAL: Listen for transcript-update events and save to shared transcript state.
    // Uses SHARED_SEGMENTS / SHARED_FOLDER to avoid accessing RecordingManager
    // (which contains !Send cpal::Stream types) from the event dispatch thread.
    {
        use tauri::Listener;
        let listener_id = app.listen("transcript-update", move |event: tauri::Event| {
            if let Ok(update) = serde_json::from_str::<TranscriptUpdate>(event.payload()) {
                let segment = transcript_segment_from_update(&update);

                // Write to shared transcript segments (no RecordingManager access)
                if let Ok(segments_guard) = SHARED_SEGMENTS.lock() {
                    if let Some(ref shared) = *segments_guard {
                        if let Ok(mut segs) = shared.lock() {
                            if let Some(existing) = segs
                                .iter_mut()
                                .find(|s| s.sequence_id == segment.sequence_id)
                            {
                                *existing = segment.clone();
                            } else {
                                segs.push(segment.clone());
                            }
                        }
                    }
                }

                // Persist to disk
                if let Ok(folder_guard) = SHARED_FOLDER.lock() {
                    if let Some(ref folder) = *folder_guard {
                        if let Ok(segments_guard) = SHARED_SEGMENTS.lock() {
                            if let Some(ref shared) = *segments_guard {
                                if let Err(e) =
                                    crate::audio::recording_saver::write_transcripts_to_disk(
                                        folder, shared,
                                    )
                                {
                                    warn!("Failed to write incremental transcript update: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        });
        let mut global_listener = TRANSCRIPT_LISTENER_ID.lock().unwrap();
        *global_listener = Some(listener_id);
        info!("✅ Transcript-update event listener registered for history persistence");
    }

    // Emit success event
    app.emit(
        "recording-started",
        serde_json::json!({
            "message": "Recording started with custom devices and parallel processing",
            "devices": [
                mic_device_name.unwrap_or_else(|| "Default Microphone".to_string()),
                system_device_name.unwrap_or_else(|| "Default System Audio".to_string())
            ],
            "workers": 3
        }),
    )
    .map_err(|e| e.to_string())?;

    // Update tray menu to reflect recording state
    crate::tray::update_tray_menu(&app);

    info!("✅ Recording started with custom devices using async-first approach");

    Ok(())
}

/// Resolve the microphone device: explicit name → saved preference → system default.
/// Microphone is required; returns an error when no device resolves.
fn resolve_microphone_device(
    explicit_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Result<Arc<super::devices::AudioDevice>, String> {
    if let Some(name) = explicit_name {
        match parse_audio_device(name) {
            Ok(device) => return Ok(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Invalid microphone device '{}': {}, falling back...",
                name, e
            ),
        }
    }

    if let Some(pref_name) = preferred_name {
        match parse_audio_device(pref_name) {
            Ok(device) => return Ok(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Preferred microphone '{}' not available: {}, falling back...",
                pref_name, e
            ),
        }
    }

    default_input_device()
        .map(Arc::new)
        .map_err(|e| format!("No microphone device available: {}", e))
}

/// Resolve the system audio device: explicit name → saved preference → system default.
/// System audio is optional; returns None only when no default output device exists.
fn resolve_system_audio_device(
    explicit_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Option<Arc<super::devices::AudioDevice>> {
    if let Some(name) = explicit_name {
        match parse_audio_device(name) {
            Ok(device) => return Some(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Invalid system device '{}': {}, falling back...",
                name, e
            ),
        }
    }

    if let Some(pref_name) = preferred_name {
        match parse_audio_device(pref_name) {
            Ok(device) => return Some(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Preferred system audio '{}' not available: {}, falling back...",
                pref_name, e
            ),
        }
    }

    match default_output_device() {
        Ok(device) => Some(Arc::new(device)),
        Err(e) => {
            warn!(
                "⚠️ No default system audio available: {}, continuing with microphone only",
                e
            );
            None
        }
    }
}

/// Stop recording with optimized graceful shutdown ensuring NO transcript chunks are lost
pub async fn stop_recording<R: Runtime>(
    app: AppHandle<R>,
    _args: RecordingArgs,
) -> Result<(), String> {
    info!(
        "🛑 Starting optimized recording shutdown - ensuring ALL transcript chunks are preserved"
    );

    // Check if recording is active
    if !IS_RECORDING.load(Ordering::SeqCst) {
        info!("Recording was not active");
        return Ok(());
    }

    // Emit shutdown progress to frontend
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "stopping_audio",
            "message": "Stopping audio capture...",
            "progress": 20
        }),
    );

    // Step 1: Stop audio capture immediately (no more new chunks) with proper error handling
    let manager_for_cleanup = {
        let mut global_manager = RECORDING_MANAGER.lock().unwrap();
        global_manager.take()
    };

    let stop_result = if let Some(mut manager) = manager_for_cleanup {
        // Use FORCE FLUSH to immediately process all accumulated audio - eliminates 30s delay!
        info!("🚀 Using FORCE FLUSH to eliminate pipeline accumulation delays");
        let result = manager.stop_streams_and_force_flush().await;
        // Store manager back for later cleanup
        let manager_for_cleanup = Some(manager);
        (result, manager_for_cleanup)
    } else {
        warn!("No recording manager found to stop");
        (Ok(()), None)
    };

    let (stop_result, manager_for_cleanup) = stop_result;

    match stop_result {
        Ok(_) => {
            info!("✅ Audio streams stopped successfully - no more chunks will be created");
        }
        Err(e) => {
            error!("❌ Failed to stop audio streams: {}", e);
            return Err(format!("Failed to stop audio streams: {}", e));
        }
    }

    // Step 1.5: Clean up transcript listener and shared state
    // Unlisten transcript-update event to prevent lingering references
    {
        use tauri::Listener;
        if let Some(listener_id) = TRANSCRIPT_LISTENER_ID.lock().unwrap().take() {
            app.unlisten(listener_id);
            info!("✅ Transcript-update listener removed");
        }
        // Clear shared transcript state
        *SHARED_SEGMENTS.lock().unwrap() = None;
        *SHARED_FOLDER.lock().unwrap() = None;
    }

    // Step 2: Signal transcription workers to finish processing ALL queued chunks
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "processing_transcripts",
            "message": "Processing remaining transcript chunks...",
            "progress": 40
        }),
    );

    // Wait for transcription task with enhanced progress monitoring (NO TIMEOUT - we must process all chunks)
    let transcription_task = {
        let mut global_task = TRANSCRIPTION_TASK.lock().unwrap();
        global_task.take()
    };

    if let Some(task_handle) = transcription_task {
        info!("⏳ Waiting for ALL transcription chunks to be processed (no timeout - preserving every chunk)");

        // Enhanced progress monitoring during shutdown
        let progress_app = app.clone();
        let progress_task = tokio::spawn(async move {
            let last_update = std::time::Instant::now();

            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                // Emit periodic progress updates during shutdown
                let elapsed = last_update.elapsed().as_secs();
                let _ = progress_app.emit(
                    "recording-shutdown-progress",
                    serde_json::json!({
                        "stage": "processing_transcripts",
                        "message": format!("Processing transcripts... ({}s elapsed)", elapsed),
                        "progress": 40,
                        "detailed": true,
                        "elapsed_seconds": elapsed
                    }),
                );
            }
        });

        // Wait up to 10 minutes for transcription completion to prevent indefinite hangs
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(600), // 10 minutes max
            task_handle,
        )
        .await
        {
            Ok(Ok(())) => {
                info!("✅ ALL transcription chunks processed successfully - no data lost");
            }
            Ok(Err(e)) => {
                warn!("⚠️ Transcription task completed with error: {:?}", e);
                // Continue anyway - the worker may have processed most chunks
            }
            Err(_) => {
                warn!("⏱️ Transcription timeout (10 minutes) reached, continuing shutdown to prevent indefinite hang");
                // Continue shutdown even on timeout - better to lose some chunks than hang forever
            }
        }

        // Stop progress monitoring
        progress_task.abort();
    } else {
        info!("ℹ️ No transcription task found to wait for");
    }

    // Step 2.5: Finalize online diarization (if active) and collect speaker assignments.
    // The embedding channel was closed when the pipeline stopped, so the consumer
    // task has drained all speech chunks and returned the processor.
    let online_task = {
        let mut global_task = ONLINE_DIARIZATION_TASK.lock().unwrap();
        global_task.take()
    };

    let speaker_assignments: Option<Vec<SpeakerAssignment>> = if let Some(task_handle) = online_task
    {
        info!("⏳ Finalizing online diarization...");
        let processor = match task_handle.await {
            Ok(Ok(Some(processor))) => Some(processor),
            Ok(Ok(None)) => None,
            Ok(Err(e)) => {
                warn!("⚠️ Online diarization unavailable: {}", e);
                None
            }
            Err(e) => {
                warn!("⚠️ Online diarization task panicked: {:?}", e);
                None
            }
        };

        // Extract the in-memory transcript segments synchronously; the
        // RecordingManager is !Sync (cpal streams), so no reference may be
        // held across an await.
        let transcripts = manager_for_cleanup
            .as_ref()
            .map(|manager| manager.get_transcript_segments());
        let meeting_folder = manager_for_cleanup
            .as_ref()
            .and_then(|m| m.get_meeting_folder());

        if let (Some(mut processor), Some(mut transcripts)) = (processor, transcripts) {
            // Stop-time repair (word-level-diarization-alignment 5.6): before
            // the N-way split, refine any segment still lacking refined tokens
            // (alignment off/missing during recording, or a block dropped by
            // queue overflow) against the saved post-flush meeting file.
            let align_settings = crate::audio::word_alignment::settings::current();
            match tokio::task::spawn_blocking(move || {
                if align_settings.enabled {
                    if let Some(folder) = &meeting_folder {
                        if let Some(source) = meeting_span_source(folder) {
                            let n = crate::audio::word_alignment::refine::refine_segment_tokens(
                                &mut transcripts,
                                source.as_ref(),
                                &align_settings,
                            );
                            if n > 0 {
                                info!(
                                    "Stop-time alignment repair: refined {} segment(s) before split",
                                    n
                                );
                            }
                        }
                    }
                }
                processor.finalize(&transcripts)
            })
            .await
            {
                Ok(Ok((assignments, cluster_embeddings, live_bindings))) => {
                    info!(
                        "✅ Online diarization finalized: {} speaker assignments, {} live bindings",
                        assignments.len(),
                        live_bindings.len()
                    );
                    // Store cluster embeddings + live bindings for the
                    // frontend-initiated finalize_online_session call, which
                    // persists them once the meeting row exists.
                    let stored_expected = ONLINE_EXPECTED_SPEAKER_IDS.lock().unwrap().clone();
                    {
                        let mut session_data = ONLINE_SESSION_DATA.lock().unwrap();
                        // Move the raw buffers out of the cluster embeddings so
                        // the full chunk set is held exactly once between stop
                        // and finalize_online_session (no double-buffer clone).
                        let OnlineClusterEmbeddings {
                            mic,
                            sys,
                            saw_system_audio,
                            model_tag,
                            mic_raw,
                            sys_raw,
                        } = cluster_embeddings;
                        *session_data = Some(OnlineSessionData {
                            mic_embeddings: mic_raw,
                            sys_embeddings: sys_raw,
                            cluster_embeddings: OnlineClusterEmbeddings {
                                mic,
                                sys,
                                saw_system_audio,
                                model_tag,
                                mic_raw: Vec::new(),
                                sys_raw: Vec::new(),
                            },
                            live_bindings,
                            expected_speaker_ids: stored_expected,
                        });
                    }
                    Some(assignments)
                }
                Ok(Err(e)) => {
                    warn!("⚠️ Online diarization finalize failed: {}", e);
                    None
                }
                Err(e) => {
                    warn!("⚠️ Online diarization finalize panicked: {:?}", e);
                    None
                }
            }
        } else {
            info!("ℹ️ Online diarization processor not available");
            None
        }
    } else {
        info!("ℹ️ No online diarization task was active");
        None
    };

    // Step 3: Now safely unload Whisper model after ALL chunks are processed
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "unloading_model",
            "message": "Unloading speech recognition model...",
            "progress": 70
        }),
    );

    info!("🧠 All transcript chunks processed. Now safely unloading transcription model...");

    // Determine which provider was used and unload the appropriate model (with timeout)
    let config = match tokio::time::timeout(
        tokio::time::Duration::from_secs(30), // 30 seconds max for DB operation
        crate::api::api::api_get_transcript_config(app.clone(), app.clone().state(), None),
    )
    .await
    {
        Ok(Ok(Some(config))) => Some(config.provider),
        Ok(Ok(None)) => None,
        Ok(Err(e)) => {
            warn!("⚠️ Failed to get transcript config: {:?}", e);
            None
        }
        Err(_) => {
            warn!("⏱️ Transcript config timeout (30s), continuing shutdown");
            None
        }
    };

    match config.as_deref() {
        Some("parakeet") => {
            info!("🦜 Unloading Parakeet model...");
            let engine_clone = {
                let engine_guard = crate::parakeet_engine::commands::PARAKEET_ENGINE
                    .lock()
                    .unwrap();
                engine_guard.as_ref().cloned()
            };

            if let Some(engine) = engine_clone {
                let current_model = engine
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());
                info!("Current Parakeet model before unload: '{}'", current_model);

                if engine.unload_model().await {
                    info!(
                        "✅ Parakeet model '{}' unloaded successfully",
                        current_model
                    );
                } else {
                    warn!("⚠️ Failed to unload Parakeet model '{}'", current_model);
                }
            } else {
                warn!("⚠️ No Parakeet engine found to unload model");
            }
        }
        _ => {
            // Default to Whisper
            info!("🎤 Unloading Whisper model...");
            let engine_clone = {
                let engine_guard = crate::whisper_engine::commands::WHISPER_ENGINE
                    .lock()
                    .unwrap();
                engine_guard.as_ref().cloned()
            };

            if let Some(engine) = engine_clone {
                let current_model = engine
                    .get_current_model()
                    .await
                    .unwrap_or_else(|| "unknown".to_string());
                info!("Current Whisper model before unload: '{}'", current_model);

                if engine.unload_model().await {
                    info!("✅ Whisper model '{}' unloaded successfully", current_model);
                } else {
                    warn!("⚠️ Failed to unload Whisper model '{}'", current_model);
                }
            } else {
                warn!("⚠️ No Whisper engine found to unload model");
            }
        }
    }

    // Step 3.5: Track meeting ended analytics with privacy-safe metadata
    // Extract all data from manager BEFORE any async operations to avoid Send issues
    let analytics_data = if let Some(ref manager) = manager_for_cleanup {
        let state = manager.get_state();
        let stats = state.get_stats();

        Some((
            manager.get_recording_duration(),
            manager.get_active_recording_duration().unwrap_or(0.0),
            manager.get_total_pause_duration(),
            manager.get_transcript_segments().len() as u64,
            state.has_fatal_error(),
            state.get_microphone_device().map(|d| d.name.clone()),
            state.get_system_device().map(|d| d.name.clone()),
            stats.chunks_processed,
        ))
    } else {
        None
    };

    // Now perform async analytics tracking without holding manager reference
    if let Some((
        total_duration,
        active_duration,
        pause_duration,
        transcript_segments_count,
        had_fatal_error,
        mic_device_name,
        sys_device_name,
        chunks_processed,
    )) = analytics_data
    {
        info!("📊 Collecting analytics for meeting end");

        // Helper function to classify device type from device name (privacy-safe)
        fn classify_device_type(device_name: &str) -> &'static str {
            let name_lower = device_name.to_lowercase();
            // Check for Bluetooth keywords
            if name_lower.contains("bluetooth")
                || name_lower.contains("airpods")
                || name_lower.contains("beats")
                || name_lower.contains("headphones")
                || name_lower.contains("bt ")
                || name_lower.contains("wireless")
            {
                "Bluetooth"
            } else {
                "Wired"
            }
        }

        // Get transcription model info (already loaded above for model unload)
        let transcription_config = match crate::api::api::api_get_transcript_config(
            app.clone(),
            app.clone().state(),
            None,
        )
        .await
        {
            Ok(Some(config)) => Some((config.provider, config.model)),
            _ => None,
        };

        let (transcription_provider, transcription_model) =
            transcription_config.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        // Get summary model info from API
        let summary_config =
            match crate::api::api::api_get_model_config(app.clone(), app.clone().state(), None)
                .await
            {
                Ok(Some(config)) => Some((config.provider, config.model)),
                _ => None,
            };

        let (summary_provider, summary_model) =
            summary_config.unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));

        // Classify device types (privacy-safe)
        let microphone_device_type = mic_device_name
            .as_ref()
            .map(|name| classify_device_type(name))
            .unwrap_or("Unknown");

        let system_audio_device_type = sys_device_name
            .as_ref()
            .map(|name| classify_device_type(name))
            .unwrap_or("Unknown");

        // Track meeting ended event with privacy-safe data
        match crate::analytics::commands::track_meeting_ended(
            transcription_provider.clone(),
            transcription_model.clone(),
            summary_provider.clone(),
            summary_model.clone(),
            total_duration,
            active_duration,
            pause_duration,
            microphone_device_type.to_string(),
            system_audio_device_type.to_string(),
            chunks_processed,
            transcript_segments_count,
            had_fatal_error,
        )
        .await
        {
            Ok(_) => info!("✅ Analytics tracked successfully for meeting end"),
            Err(e) => warn!("⚠️ Failed to track analytics: {}", e),
        }
    }

    // Step 4: Finalize recording state and cleanup resources safely
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "finalizing",
            "message": "Finalizing recording and cleaning up resources...",
            "progress": 90
        }),
    );

    // Perform final cleanup with the manager if available
    let (meeting_folder, meeting_name) = if let Some(mut manager) = manager_for_cleanup {
        info!("🧹 Performing final cleanup and saving recording data");

        // Extract meeting info BEFORE async operations
        let meeting_folder = manager.get_meeting_folder();
        let meeting_name = manager.get_meeting_name();

        match tokio::time::timeout(
            tokio::time::Duration::from_secs(300), // 5 minutes max for file I/O
            manager.save_recording_only(&app),
        )
        .await
        {
            Ok(Ok(_)) => {
                info!("✅ Recording data saved successfully during cleanup");
            }
            Ok(Err(e)) => {
                warn!(
                    "⚠️ Error during recording cleanup (transcripts preserved): {}",
                    e
                );
                // Don't fail shutdown - transcripts are already preserved
            }
            Err(_) => {
                warn!("⏱️ File I/O timeout (5 minutes) reached during save, continuing shutdown");
                // Don't fail shutdown - transcripts are already preserved
            }
        }

        (meeting_folder, meeting_name)
    } else {
        info!("ℹ️ No recording manager available for cleanup");
        (None, None)
    };

    // Set recording flag to false
    info!("🔍 Setting IS_RECORDING to false");
    IS_RECORDING.store(false, Ordering::SeqCst);

    // Step 4.5: Prepare metadata for frontend (NO database save)
    // NOTE: We do NOT save to database here. The frontend will save after all transcripts are displayed.
    // This ensures the user sees all transcripts streaming in before the database save happens.
    let (folder_path_str, meeting_name_str) = match (&meeting_folder, &meeting_name) {
        (Some(path), Some(name)) => (Some(path.to_string_lossy().to_string()), Some(name.clone())),
        _ => (None, None),
    };

    info!("📤 Preparing recording metadata for frontend save");
    info!("   folder_path: {:?}", folder_path_str);
    info!("   meeting_name: {:?}", meeting_name_str);

    // Database save removed - frontend will handle this after receiving all transcripts
    info!("ℹ️ Skipping database save in Rust - frontend will save after all transcripts received");

    // Step 5: Complete shutdown
    let _ = app.emit(
        "recording-shutdown-progress",
        serde_json::json!({
            "stage": "complete",
            "message": "Recording stopped successfully",
            "progress": 100
        }),
    );

    // Emit final stop event with folder_path and meeting_name for frontend to save
    let mut stopped_payload = serde_json::json!({
        "message": "Recording stopped - frontend will save after all transcripts received",
        "folder_path": folder_path_str,
        "meeting_name": meeting_name_str
    });
    match &speaker_assignments {
        Some(assignments) => {
            stopped_payload["online_diarization_used"] = serde_json::json!(true);
            stopped_payload["speaker_assignments"] =
                serde_json::to_value(assignments).unwrap_or_else(|_| serde_json::json!([]));
        }
        None => {
            stopped_payload["online_diarization_used"] = serde_json::json!(false);
        }
    }
    app.emit("recording-stopped", stopped_payload)
        .map_err(|e| e.to_string())?;

    // Update tray menu to reflect stopped state
    crate::tray::update_tray_menu(&app);

    info!("🎉 Recording stopped successfully with ZERO transcript chunks lost");
    Ok(())
}

/// Check if recording is active
pub async fn is_recording() -> bool {
    IS_RECORDING.load(Ordering::SeqCst)
}

/// Get recording statistics
pub async fn get_transcription_status() -> TranscriptionStatus {
    TranscriptionStatus {
        chunks_in_queue: 0,
        is_processing: IS_RECORDING.load(Ordering::SeqCst),
        last_activity_ms: 0,
    }
}

/// Pause the current recording
#[tauri::command]
pub async fn pause_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Pausing recording");

    // Check if currently recording
    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    // Access the recording manager and pause it
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.pause_recording().map_err(|e| e.to_string())?;

        // Emit pause event to frontend
        app.emit(
            "recording-paused",
            serde_json::json!({
                "message": "Recording paused"
            }),
        )
        .map_err(|e| e.to_string())?;

        // Update tray menu to reflect paused state
        crate::tray::update_tray_menu(&app);

        info!("Recording paused successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

/// Resume the current recording
#[tauri::command]
pub async fn resume_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    info!("Resuming recording");

    // Check if currently recording
    if !IS_RECORDING.load(Ordering::SeqCst) {
        return Err("No recording is currently active".to_string());
    }

    // Access the recording manager and resume it
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.resume_recording().map_err(|e| e.to_string())?;

        // Emit resume event to frontend
        app.emit(
            "recording-resumed",
            serde_json::json!({
                "message": "Recording resumed"
            }),
        )
        .map_err(|e| e.to_string())?;

        // Update tray menu to reflect resumed state
        crate::tray::update_tray_menu(&app);

        info!("Recording resumed successfully");
        Ok(())
    } else {
        Err("No recording manager found".to_string())
    }
}

/// Check if recording is currently paused
#[tauri::command]
pub async fn is_recording_paused() -> bool {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    if let Some(manager) = manager_guard.as_ref() {
        manager.is_paused()
    } else {
        false
    }
}

/// Get detailed recording state
#[tauri::command]
pub async fn get_recording_state() -> serde_json::Value {
    let is_recording = IS_RECORDING.load(Ordering::SeqCst);
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": manager.is_paused(),
            "is_active": manager.is_active(),
            "recording_duration": manager.get_recording_duration(),
            "active_duration": manager.get_active_recording_duration(),
            "total_pause_duration": manager.get_total_pause_duration(),
            "current_pause_duration": manager.get_current_pause_duration()
        })
    } else {
        serde_json::json!({
            "is_recording": is_recording,
            "is_paused": false,
            "is_active": false,
            "recording_duration": null,
            "active_duration": null,
            "total_pause_duration": 0.0,
            "current_pause_duration": null
        })
    }
}

/// Get the meeting folder path for the current recording
/// Returns the path if a meeting name was set and folder structure initialized
#[tauri::command]
pub async fn get_meeting_folder_path() -> Result<Option<String>, String> {
    Ok(current_meeting_folder().map(|p| p.to_string_lossy().to_string()))
}

/// Meeting folder of the active recording, if any. Shared helper for
/// commands that operate on per-recording `metadata.json` keys.
pub(crate) fn current_meeting_folder() -> Option<std::path::PathBuf> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();
    manager_guard
        .as_ref()
        .and_then(|manager| manager.get_meeting_folder())
}

/// Get accumulated transcript segments from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_transcript_history(
) -> Result<Vec<crate::audio::recording_saver::TranscriptSegment>, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_transcript_segments())
    } else {
        Ok(Vec::new()) // No recording active, return empty
    }
}

/// Get meeting name from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_recording_meeting_name() -> Result<Option<String>, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_meeting_name())
    } else {
        Ok(None)
    }
}

// ============================================================================
// DEVICE MONITORING COMMANDS (AirPods/Bluetooth disconnect/reconnect support)
// ============================================================================

/// Response structure for device events
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum DeviceEventResponse {
    DeviceDisconnected {
        device_name: String,
        device_type: String,
    },
    DeviceReconnected {
        device_name: String,
        device_type: String,
    },
    DeviceListChanged,
}

impl From<DeviceEvent> for DeviceEventResponse {
    fn from(event: DeviceEvent) -> Self {
        match event {
            DeviceEvent::DeviceDisconnected {
                device_name,
                device_type,
            } => DeviceEventResponse::DeviceDisconnected {
                device_name,
                device_type: format!("{:?}", device_type),
            },
            DeviceEvent::DeviceReconnected {
                device_name,
                device_type,
            } => DeviceEventResponse::DeviceReconnected {
                device_name,
                device_type: format!("{:?}", device_type),
            },
            DeviceEvent::DeviceListChanged => DeviceEventResponse::DeviceListChanged,
        }
    }
}

/// Reconnection status information
#[derive(Debug, Serialize, Clone)]
pub struct ReconnectionStatus {
    pub is_reconnecting: bool,
    pub disconnected_device: Option<DisconnectedDeviceInfo>,
}

/// Information about a disconnected device
#[derive(Debug, Serialize, Clone)]
pub struct DisconnectedDeviceInfo {
    pub name: String,
    pub device_type: String,
}

/// Poll for audio device events (disconnect/reconnect)
/// Should be called periodically (every 1-2 seconds) by frontend during recording
#[tauri::command]
pub async fn poll_audio_device_events() -> Result<Option<DeviceEventResponse>, String> {
    let mut manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_mut() {
        if let Some(event) = manager.poll_device_events() {
            info!("📱 Device event polled: {:?}", event);
            Ok(Some(event.into()))
        } else {
            Ok(None)
        }
    } else {
        // Not recording, no events
        Ok(None)
    }
}

/// Get current reconnection status
/// Returns whether the system is attempting to reconnect and which device
#[tauri::command]
pub async fn get_reconnection_status() -> Result<ReconnectionStatus, String> {
    let manager_guard = RECORDING_MANAGER.lock().unwrap();

    if let Some(manager) = manager_guard.as_ref() {
        let state = manager.get_state();
        let disconnected_device = state
            .get_disconnected_device()
            .map(|(device, device_type)| DisconnectedDeviceInfo {
                name: device.name.clone(),
                device_type: format!("{:?}", device_type),
            });

        Ok(ReconnectionStatus {
            is_reconnecting: manager.is_reconnecting(),
            disconnected_device,
        })
    } else {
        // Not recording, no reconnection in progress
        Ok(ReconnectionStatus {
            is_reconnecting: false,
            disconnected_device: None,
        })
    }
}

/// Get information about the active audio output device
/// Used to warn users about Bluetooth playback issues
#[tauri::command]
pub async fn get_active_audio_output() -> Result<super::playback_monitor::AudioOutputInfo, String> {
    super::playback_monitor::get_active_audio_output()
        .await
        .map_err(|e| format!("Failed to get audio output info: {}", e))
}

/// Manually trigger device reconnection attempt
/// Useful for UI "Retry" button
#[tauri::command]
pub async fn attempt_device_reconnect(
    device_name: String,
    device_type: String,
) -> Result<bool, String> {
    // Parse device type first
    let monitor_type = match device_type.as_str() {
        "Microphone" => DeviceMonitorType::Microphone,
        "SystemAudio" => DeviceMonitorType::SystemAudio,
        _ => return Err(format!("Invalid device type: {}", device_type)),
    };

    // Check if recording is active
    {
        let manager_guard = RECORDING_MANAGER.lock().unwrap();
        if manager_guard.is_none() {
            return Err("Recording not active".to_string());
        }
    } // Release lock

    // Spawn blocking task to handle the async reconnection
    let result = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            let mut manager_guard = RECORDING_MANAGER.lock().unwrap();
            if let Some(manager) = manager_guard.as_mut() {
                manager
                    .attempt_device_reconnect(&device_name, monitor_type)
                    .await
            } else {
                Err(anyhow::anyhow!("Recording not active"))
            }
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    match result {
        Ok(success) => {
            if success {
                info!("✅ Manual reconnection successful");
            } else {
                warn!("❌ Manual reconnection failed - device not available");
            }
            Ok(success)
        }
        Err(e) => {
            error!("Manual reconnection error: {}", e);
            Err(e.to_string())
        }
    }
}

// ============================================================================
// SPEAKER IDENTITY REGISTRY — online session finalization
// ============================================================================

/// Finalize an online diarization session after the meeting row has been
/// created by the frontend. Persists cluster centroids + exemplar caches,
/// runs auto-recognition, enrolls session embeddings, and persists the
/// expected-speaker allowlist. Called once per recording stop.
#[tauri::command]
pub async fn finalize_online_session(
    meeting_id: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<serde_json::Value, String> {
    let pool = state.db_manager.pool();

    // Take the pending session data. When none is present (e.g. a non-online
    // recording that still navigates through finalize), no-op gracefully so the
    // frontend may finalize unconditionally and never drop live bindings.
    let Some(session_data) = ONLINE_SESSION_DATA.lock().unwrap().take() else {
        return Ok(serde_json::json!({
            "meeting_id": meeting_id,
            "live_bindings": 0,
            "enrolled": 0,
        }));
    };

    // Persist the expected-speaker allowlist FIRST (empty list = match all),
    // so the stop-time auto-recognition below is restricted to the session's
    // expected speakers instead of falling back to all registry speakers.
    if !session_data.expected_speaker_ids.is_empty() {
        SpeakerRepository::set_expected_speakers(
            pool,
            &meeting_id,
            &session_data.expected_speaker_ids,
        )
        .await
        .map_err(|e| format!("Failed to persist expected speakers: {}", e))?;
    }

    // Persist cluster centroids + exemplar caches and auto-assign recognized speakers.
    // Matching is enhanced-only; the session tag is informational.
    if !session_data.cluster_embeddings.mic.is_empty()
        || !session_data.cluster_embeddings.sys.is_empty()
    {
        super::diarization::persist_and_recognize_session(
            pool,
            &meeting_id,
            &session_data.cluster_embeddings.mic,
            &session_data.cluster_embeddings.sys,
            session_data.cluster_embeddings.saw_system_audio,
        )
        .await?;
    }

    // Enroll session embeddings for user-assigned clusters (best-8 reparenting,
    // per-person cap). This covers live renames and post-stop manual bindings.
    let mut enrolled = 0usize;
    for (cluster_label, speaker_id) in &session_data.live_bindings {
        // Re-binding (demote a previous speaker's prototypes, then enroll the
        // best-K) is shared with the offline commands so both behave the same.
        let channel = if cluster_label.starts_with("MIC_SPEAKER_") {
            Some("mic")
        } else if cluster_label.starts_with("SPEAKER_") {
            if session_data.cluster_embeddings.saw_system_audio {
                Some("system")
            } else {
                Some("mic")
            }
        } else {
            None
        };
        match SpeakerRepository::rebind_cluster(
            pool,
            &meeting_id,
            cluster_label,
            channel,
            speaker_id,
        )
        .await
        {
            Ok(n) => enrolled += n,
            Err(e) => warn!(
                "Failed to enroll session cluster {} → {}: {}",
                cluster_label, speaker_id, e
            ),
        }
    }

    // Persist live user bindings as matched_by='user' so a cluster renamed
    // mid-recording is NEVER overwritten later by auto-recognition or
    // re-match (design D8: user bindings always win). Only binding rows are
    // written here; enrollment above already reparented the embeddings.
    for (cluster_label, speaker_id) in &session_data.live_bindings {
        if let Err(e) =
            SpeakerRepository::set_user_binding(pool, &meeting_id, cluster_label, speaker_id).await
        {
            warn!(
                "Failed to persist live user binding {label} → {speaker_id}: {e}",
                label = cluster_label
            );
        }
        // Also write the user's identity onto the stored transcript rows of the
        // cluster, so each row resolves to the user via the override join even
        // if the meeting_speakers render-time join is unavailable.
        if let Err(e) = SpeakerRepository::apply_cluster_binding_overrides(
            pool,
            &meeting_id,
            cluster_label,
            speaker_id,
        )
        .await
        {
            warn!("Failed to persist cluster binding overrides for {cluster_label}: {e}");
        }
    }

    // Apply live per-turn overrides (single-block relabels recorded during
    // Fast-mode recording) to the meeting's transcripts. Applied after
    // auto-recognition so the user's explicit choice always wins.
    let turn_overrides: Vec<(String, f64, f64, String)> = ONLINE_TURN_OVERRIDES
        .lock()
        .unwrap()
        .drain(..)
        .map(|o| (o.cluster_label, o.start_secs, o.end_secs, o.speaker_id))
        .collect();

    // Ground-truth enrollment for each single-block override: the chunk
    // embeddings overlapping the relabeled block's time window become
    // prototypes of the chosen speaker, improving the global registry. A user
    // pick is ground truth — it should strengthen the person's identity.
    for (cluster_label, start, end, speaker_id) in &turn_overrides {
        let channel = if cluster_label.starts_with("MIC_SPEAKER_") {
            Some("mic")
        } else if cluster_label.starts_with("SPEAKER_") {
            if session_data.cluster_embeddings.saw_system_audio {
                Some("system")
            } else {
                Some("mic")
            }
        } else {
            None
        };
        let Some(channel) = channel else { continue };
        let buffer = if channel == "mic" {
            &session_data.mic_embeddings
        } else {
            &session_data.sys_embeddings
        };
        match SpeakerRepository::enroll_embeddings_from_buffer(
            pool,
            speaker_id,
            channel,
            buffer,
            (*start as f32, *end as f32),
            &meeting_id,
            cluster_label,
        )
        .await
        {
            Ok(n) => {
                if n > 0 {
                    enrolled += n;
                    info!(
                        "Ground-truth enrollment: {} embeddings for {} from override {} [{:.1}s-{:.1}s]",
                        n, speaker_id, cluster_label, start, end
                    );
                }
            }
            Err(e) => warn!(
                "Failed to enroll ground-truth embeddings for {} from {}: {}",
                speaker_id, cluster_label, e
            ),
        }
    }

    if !turn_overrides.is_empty() {
        SpeakerRepository::apply_turn_overrides(pool, &meeting_id, &turn_overrides)
            .await
            .map_err(|e| format!("Failed to apply turn overrides: {}", e))?;
    }

    // Clear the global prototype store (session ended).
    {
        let mut store = ONLINE_DIARIZATION_STORE.lock().unwrap();
        *store = None;
    }

    info!(
        "✅ Online session finalized for {}: {} live bindings, {} enrolled embeddings, {} expected speakers",
        meeting_id,
        session_data.live_bindings.len(),
        enrolled,
        session_data.expected_speaker_ids.len()
    );

    Ok(serde_json::json!({
        "meeting_id": meeting_id,
        "live_bindings": session_data.live_bindings.len(),
        "enrolled": enrolled,
    }))
}

/// Assign a live speaker during Fast-mode recording.
///
/// Two scopes (design D10):
/// - `scope` unset or "cluster": update the in-memory prototype store binding
///   so subsequent chunks of the cluster are recognized and labeled for the
///   remainder of the session; persisted at stop-time finalize.
/// - `scope == "block"` together with `start_time`/`end_time`: relabel only
///   the turn(s) overlapping that time range via a per-turn override (no
///   cluster binding, no prototype merge); applied to the matched transcript
///   at stop-time finalize.
#[tauri::command]
pub async fn assign_live_speaker(
    cluster_label: String,
    speaker_id: Option<String>,
    new_name: Option<String>,
    scope: Option<String>,
    start_time: Option<f64>,
    end_time: Option<f64>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<crate::database::speaker_commands::AssignedSpeaker, String> {
    let pool = state.db_manager.pool();

    // Find or create the registry speaker.
    let speaker = match (speaker_id.as_ref(), new_name.as_ref()) {
        (Some(id), _) => SpeakerRepository::get_speaker(pool, id)
            .await
            .map_err(|e| format!("Failed to load speaker: {}", e))?
            .ok_or_else(|| format!("Speaker {} not found", id))?,
        (None, Some(name)) => SpeakerRepository::find_or_create_by_name(pool, name)
            .await
            .map_err(|e| format!("Failed to find-or-create speaker: {}", e))?,
        (None, None) => {
            return Err("Either speaker_id or new_name must be provided".to_string());
        }
    };

    let is_block_scope = scope.as_deref() == Some("block") && start_time.is_some();

    if is_block_scope {
        // Single-turn override: record only (no cluster binding). Applied to
        // the matched transcript at stop-time finalize.
        let start = start_time.unwrap_or(0.0);
        let end = end_time.filter(|e| *e > start).unwrap_or(start + 1.0);
        ONLINE_TURN_OVERRIDES.lock().unwrap().push(TurnOverride {
            cluster_label: cluster_label.clone(),
            start_secs: start,
            end_secs: end,
            speaker_id: speaker.id.clone(),
        });
        info!(
            "Live per-turn override: {} [{:.1}s-{:.1}s] -> {}",
            cluster_label, start, end, speaker.name
        );
    } else {
        // Cluster-wide binding: update the in-memory prototype store so
        // subsequent chunks of this cluster match. Fail loudly when no live
        // prototype store is active so a correction cannot silently disappear
        // and later revert to a predicted label at stop.
        let store_guard = ONLINE_DIARIZATION_STORE.lock().unwrap();
        let store_arc = store_guard.as_ref().ok_or_else(|| {
            "No live diarization session active; cannot assign a live speaker".to_string()
        })?;
        store_arc
            .write()
            .map_err(|_| "Live prototype store is locked".to_string())?
            .bind(&cluster_label, &speaker.id, &speaker.name);
    }

    // The actual DB persistence (meeting_speakers + enrollment / transcript
    // overrides) happens at stop-time via finalize_online_session.

    Ok(crate::database::speaker_commands::AssignedSpeaker {
        meeting_id: String::new(), // No meeting_id yet during live recording
        cluster_label,
        speaker_id: speaker.id,
        name: speaker.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::token_assignment::Token;

    fn sample_update() -> TranscriptUpdate {
        TranscriptUpdate {
            text: "hello world".to_string(),
            timestamp: "[00:01]".to_string(),
            source: "Audio".to_string(),
            sequence_id: 7,
            chunk_start_time: 1.0,
            is_partial: false,
            confidence: 0.9,
            audio_start_time: 1.0,
            audio_end_time: 3.5,
            duration: 2.5,
            source_device: "Microphone".to_string(),
            speaker: None,
            tokens: Some(vec![
                Token {
                    text: "hello".to_string(),
                    start: 1.0,
                    end: 1.8,
                    refined: false,
                },
                Token {
                    text: "world".to_string(),
                    start: 1.8,
                    end: 2.4,
                    refined: false,
                },
            ]),
        }
    }

    #[test]
    fn transcript_update_tokens_survive_event_payload_roundtrip() {
        // Mirrors the listener path: serialize the emitted update, parse it back
        // from the event payload, and map it onto a buffered segment.
        let update = sample_update();
        let payload = serde_json::to_string(&update).unwrap();
        let parsed: TranscriptUpdate = serde_json::from_str(&payload).unwrap();
        let segment = transcript_segment_from_update(&parsed);
        let tokens = segment.tokens.expect("tokens must be forwarded");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].text, "hello");
        assert_eq!(tokens[1].end, 2.4);
        assert_eq!(segment.sequence_id, 7);
    }

    #[test]
    fn transcript_segment_from_update_without_tokens_stays_none() {
        let mut update = sample_update();
        update.tokens = None;
        let segment = transcript_segment_from_update(&update);
        assert!(segment.tokens.is_none());
    }
}
