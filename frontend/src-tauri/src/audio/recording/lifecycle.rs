// audio/recording/lifecycle.rs
//
// Recording start orchestration: device resolution, manager start, the
// transcription task and transcript-update listener, and the live
// diarization session.

use log::{error, info, warn};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Emitter, Runtime};

use super::device_recovery::{
    spawn_device_event_processor, MicDeviceSwitchedPayload, SwitchReason, UserEvent,
};
use super::devices::{resolve_microphone_device, resolve_system_audio_device, ResolvedMic};
use crate::audio::diarization::engine::{DiarizationEngine, LiveSessionConfig};
use crate::audio::online_diarization::{clear_stats, DiarizationMode};
use crate::audio::recording_commands::{
    transcript_segment_from_update, IS_RECORDING, RECORDING_MANAGER, SHARED_FOLDER,
    SHARED_SEGMENTS, TRANSCRIPTION_TASK, TRANSCRIPT_LISTENER_ID,
};
use crate::audio::sync_ext::LockRecover;
use crate::audio::transcription::{self, reset_speech_detected_flag, TranscriptUpdate};
use crate::audio::{default_output_device, parse_audio_device, RecordingManager};

/// Tell the user when the start fell back from an unavailable requested or
/// preferred mic to the system default (design D6).
fn notify_mic_unavailable_at_start<R: Runtime>(app: &AppHandle<R>, resolved: &ResolvedMic) {
    if let Some(previous) = &resolved.fell_back_from {
        UserEvent::MicDeviceSwitched(MicDeviceSwitchedPayload {
            device_name: resolved.device.name.clone(),
            previous_device_name: previous.clone(),
            reason: SwitchReason::UnavailableAtStart,
        })
        .emit(app);
    }
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

    let engine_lifecycle_guard = crate::audio::common::acquire_engine_lifecycle_lock().await;

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
        match crate::audio::recording_preferences::load_recording_preferences(&app).await {
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
    let resolved_mic = match resolve_microphone_device(None, preferred_mic_name.as_deref()) {
        Ok(resolved) => resolved,
        Err(no_mic) => {
            error!("❌ No microphone available");
            return Err(match &preferred_mic_name {
                Some(pref_name) => format!(
                    "No microphone device available. Preferred device '{}' not found, and default microphone unavailable: {}",
                    pref_name, no_mic.default_error
                ),
                None => no_mic.message(),
            });
        }
    };
    let microphone_device = Some(resolved_mic.device.clone());

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
    notify_mic_unavailable_at_start(&app, &resolved_mic);

    // The session's device events go to its recovery processor.
    let device_events = manager.take_device_event_receiver();
    let session = manager.get_state().clone();

    // Store the manager globally to keep it alive
    {
        let mut global_manager = RECORDING_MANAGER.lock_or_recover();
        *global_manager = Some(manager);
    }

    // Extract shared transcript state for the event listener.
    // This avoids cross-thread access to RecordingManager (which contains !Send cpal::Stream).
    {
        let manager_guard = RECORDING_MANAGER.lock_or_recover();
        if let Some(ref mgr) = *manager_guard {
            *SHARED_SEGMENTS.lock_or_recover() = Some(mgr.shared_segments());
            *SHARED_FOLDER.lock_or_recover() = mgr.get_meeting_folder();
        }
    }

    // Set recording flag and reset speech detection flag
    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    IS_RECORDING.store(true, Ordering::SeqCst);
    drop(engine_lifecycle_guard);
    reset_speech_detected_flag(); // Reset for new recording session

    // Backend mic recovery for this session (mic-disconnect-recovery).
    if let Some(device_events) = device_events {
        spawn_device_event_processor(app.clone(), device_events, session);
    }

    // Start optimized parallel transcription task and store handle
    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK.lock_or_recover();
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
        let mut global_listener = TRANSCRIPT_LISTENER_ID.lock_or_recover();
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

    let engine_lifecycle_guard = crate::audio::common::acquire_engine_lifecycle_lock().await;

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
        match crate::audio::recording_preferences::load_recording_preferences(&app).await {
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
    let resolved_mic =
        resolve_microphone_device(mic_device_name.as_deref(), preferred_mic_name.as_deref())
            .map_err(|no_mic| no_mic.message())?;
    let mic_device = Some(resolved_mic.device.clone());

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

    // Every new session starts from zero activity: drop any telemetry left
    // behind by a previous run (including one that never reached
    // stop_recording), so a diarization-off or crashed-previous session can
    // never show stale counts.
    clear_stats();
    crate::audio::telemetry::clear_pipeline();
    crate::audio::telemetry::clear_asr();
    crate::audio::telemetry::clear_alignment();

    // The live diarization session owns its own state, models and drain task;
    // the engine hands back the sender the pipeline feeds (None when off).
    let live_chunk_sender = DiarizationEngine::start_live_session(
        &app,
        LiveSessionConfig {
            mode: online_mode,
            max_speakers,
            has_system_device: system_device.is_some(),
            expected_speaker_ids: expected_ids.clone(),
        },
    )
    .await;
    manager.set_embedding_sender(live_chunk_sender);

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
    notify_mic_unavailable_at_start(&app, &resolved_mic);

    // The session's device events go to its recovery processor.
    let device_events = manager.take_device_event_receiver();
    let session = manager.get_state().clone();

    // Store the manager globally to keep it alive
    {
        let mut global_manager = RECORDING_MANAGER.lock_or_recover();
        *global_manager = Some(manager);
    }

    // Extract shared transcript state for the event listener.
    // This avoids cross-thread access to RecordingManager (which contains !Send cpal::Stream).
    {
        let manager_guard = RECORDING_MANAGER.lock_or_recover();
        if let Some(ref mgr) = *manager_guard {
            *SHARED_SEGMENTS.lock_or_recover() = Some(mgr.shared_segments());
            *SHARED_FOLDER.lock_or_recover() = mgr.get_meeting_folder();
        }
    }

    // Set recording flag and reset speech detection flag
    info!("🔍 Setting IS_RECORDING to true and resetting SPEECH_DETECTED_EMITTED");
    IS_RECORDING.store(true, Ordering::SeqCst);
    drop(engine_lifecycle_guard);
    reset_speech_detected_flag(); // Reset for new recording session

    // Backend mic recovery for this session (mic-disconnect-recovery).
    if let Some(device_events) = device_events {
        spawn_device_event_processor(app.clone(), device_events, session);
    }

    // Start optimized parallel transcription task and store handle
    let task_handle = transcription::start_transcription_task(app.clone(), transcription_receiver);
    {
        let mut global_task = TRANSCRIPTION_TASK.lock_or_recover();
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
        let mut global_listener = TRANSCRIPT_LISTENER_ID.lock_or_recover();
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
