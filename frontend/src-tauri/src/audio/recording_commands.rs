// audio/recording_commands.rs
//
// Slim Tauri command layer for recording functionality.
// Delegates to transcription and recording modules for actual implementation.

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::task::JoinHandle;

use super::RecordingManager;
use super::diarization::engine::{DiarizationEngine, LiveSpeakerAssignment};
use super::sync_ext::LockRecover;

use super::online_diarization::{DiarizationMode, OnlineDiarizationStatus};

// Orchestration lives in `audio::recording`; re-exported so existing
// `recording_commands::` caller paths keep resolving.
pub use super::recording::lifecycle::{
    start_recording_with_devices_and_meeting, start_recording_with_meeting_name,
};
pub use super::recording::stop::stop_recording;

// Re-export TranscriptUpdate for backward compatibility
pub use super::transcription::TranscriptUpdate;

// ============================================================================
// GLOBAL STATE
// ============================================================================

// Simple recording state tracking
pub(super) static IS_RECORDING: AtomicBool = AtomicBool::new(false);

// Global recording manager and transcription task to keep them alive during recording
pub(super) static RECORDING_MANAGER: Mutex<Option<RecordingManager>> = Mutex::new(None);
pub(super) static TRANSCRIPTION_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);


// Listener ID for proper cleanup - prevents microphone from staying active after recording stops
pub(super) static TRANSCRIPT_LISTENER_ID: Mutex<Option<tauri::EventId>> = Mutex::new(None);

// Shared transcript segments and meeting folder for event listener access.
// These bypass RecordingManager to avoid cross-thread access to !Send types (cpal::Stream).
pub(super) static SHARED_SEGMENTS: Mutex<Option<Arc<Mutex<Vec<super::recording_saver::TranscriptSegment>>>>> =
    Mutex::new(None);
pub(super) static SHARED_FOLDER: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

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
pub(super) fn transcript_segment_from_update(
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


// ============================================================================
// RECORDING COMMANDS
// ============================================================================

/// Start recording with default devices
pub async fn start_recording<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    start_recording_with_meeting_name(app, None).await
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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();
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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();
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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();
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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();

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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();
    manager_guard
        .as_ref()
        .and_then(|manager| manager.get_meeting_folder())
}

/// Get accumulated transcript segments from current recording session
/// Used for syncing frontend state after page reload during active recording
#[tauri::command]
pub async fn get_transcript_history(
) -> Result<Vec<crate::audio::recording_saver::TranscriptSegment>, String> {
    let manager_guard = RECORDING_MANAGER.lock_or_recover();

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
    let manager_guard = RECORDING_MANAGER.lock_or_recover();

    if let Some(manager) = manager_guard.as_ref() {
        Ok(manager.get_meeting_name())
    } else {
        Ok(None)
    }
}

// ============================================================================
// PLAYBACK DEVICE COMMANDS
// ============================================================================

/// Get information about the active audio output device
/// Used to warn users about Bluetooth playback issues
#[tauri::command]
pub async fn get_active_audio_output() -> Result<super::playback_monitor::AudioOutputInfo, String> {
    super::playback_monitor::get_active_audio_output()
        .await
        .map_err(|e| format!("Failed to get audio output info: {}", e))
}

// ============================================================================
// SPEAKER IDENTITY REGISTRY — online session finalization
// ============================================================================


/// One channel's pipeline buffer fills and voice-activity activity.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineStatus {
    pub sample_rate: u32,
    pub mic: crate::audio::telemetry::ChannelPipelineFill,
    pub sys: crate::audio::telemetry::ChannelPipelineFill,
}

/// Diarization's model identity, readiness and block-queue activity.
#[derive(Debug, Clone, Serialize)]
pub struct DiarizationModelActivity {
    pub mode: DiarizationMode,
    pub model_tag: String,
    pub embedding_dim: usize,
    pub recognition_threshold: f32,
    pub loaded: bool,
    pub prototypes: Option<usize>,
    pub bindings: Option<usize>,
    /// Blocks queued for the diarization engine but not yet consumed.
    pub pending_blocks: u64,
    pub blocks_sent: u64,
    pub blocks_completed: u64,
    /// True while the engine works on a dequeued block.
    pub in_flight: bool,
    /// Blocks were submitted but the engine is not yet consuming them.
    pub requested: bool,
}

/// Every model kind the recording relies on, with readiness and activity.
#[derive(Debug, Clone, Serialize)]
pub struct ModelsActivity {
    pub vad: crate::audio::telemetry::VadActivity,
    pub asr: crate::audio::telemetry::AsrActivity,
    pub alignment: crate::audio::telemetry::AlignmentActivity,
    pub diarization: DiarizationModelActivity,
}

/// Read-only snapshot backing the live status lines and their tooltip.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingTelemetry {
    /// True while an online diarization session is running.
    pub active: bool,
    pub diarization: OnlineDiarizationStatus,
    pub pipeline: PipelineStatus,
    pub models: ModelsActivity,
}


/// Read-only snapshot of the whole recording: diarization, the pipeline buffer
/// fills that gate its operations, and the activity of every model in use
/// (online-diarization-telemetry). Sampled by the frontend on an interval; the
/// chunk path never emits an event.
///
/// Returns an inactive shape when no session is running, so a stopped session's
/// counters are never presented as live values.
#[tauri::command]
pub async fn get_recording_telemetry() -> Result<RecordingTelemetry, String> {
    let diarization = DiarizationEngine::telemetry_snapshot().await?;

    let pipeline = match crate::audio::telemetry::pipeline() {
        Some(pipeline) => {
            let (mic, sys) = pipeline.snapshots();
            PipelineStatus {
                sample_rate: pipeline.sample_rate(),
                mic,
                sys,
            }
        }
        None => PipelineStatus {
            sample_rate: 0,
            mic: Default::default(),
            sys: Default::default(),
        },
    };

    let models = ModelsActivity {
        vad: crate::audio::telemetry::vad_activity(),
        asr: crate::audio::telemetry::asr_activity(),
        alignment: crate::audio::telemetry::alignment_activity(),
        diarization: DiarizationModelActivity {
            mode: diarization.mode,
            model_tag: diarization.model_tag.clone(),
            embedding_dim: diarization.embedding_dim,
            recognition_threshold: diarization.recognition_threshold,
            loaded: diarization.available,
            prototypes: diarization.prototypes,
            bindings: diarization.bindings,
            pending_blocks: diarization.pending_blocks,
            blocks_sent: diarization.blocks_sent,
            blocks_completed: diarization.blocks_processed,
            in_flight: diarization.blocks_in_flight,
            requested: diarization.pending_blocks > 0 && !diarization.blocks_in_flight,
        },
    };

    Ok(RecordingTelemetry {
        active: diarization.active,
        diarization,
        pipeline,
        models,
    })
}

/// Finalize an online diarization session after the meeting row has been
/// created by the frontend. The work lives in the engine; this is the command
/// surface (05 task 5.4).
#[tauri::command]
pub async fn finalize_online_session(
    meeting_id: String,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<serde_json::Value, String> {
    DiarizationEngine::persist_session(state.db_manager.pool(), meeting_id).await
}

/// Assign a speaker to a live cluster (or to a single turn with
/// `scope == "block"`) while a recording runs. The work lives in the engine;
/// this is the command surface (05 task 5.5).
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
#[allow(clippy::too_many_arguments)]
pub async fn assign_live_speaker(
    cluster_label: String,
    speaker_id: Option<String>,
    new_name: Option<String>,
    scope: Option<String>,
    start_time: Option<f64>,
    end_time: Option<f64>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<crate::database::speaker_commands::AssignedSpeaker, String> {
    DiarizationEngine::assign_live_speaker(
        state.db_manager.pool(),
        LiveSpeakerAssignment {
            cluster_label,
            speaker_id,
            new_name,
            scope,
            start_time,
            end_time,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::embedder::{
        ENHANCED_EMBEDDING_DIM, ENHANCED_MODEL_TAG, TITANET_RECOGNITION_THRESHOLD,
    };
    use crate::audio::online_diarization::{begin_stats, DiarChannel, DiarChannelState};
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

    #[tokio::test]
    async fn recording_telemetry_reports_inactive_and_active_shapes() {
        use crate::audio::online_diarization::clear_stats;
        use crate::audio::recording_state::DeviceType;
        use crate::audio::telemetry::{
            clear_alignment, clear_asr, clear_pipeline, install_pipeline, VAD_MODEL_IDENTITY,
            TELEMETRY_TEST_LOCK,
        };

        let _guard = TELEMETRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // No session: an inactive shape, never zeros presented as live values.
        clear_stats();
        clear_pipeline();
        clear_asr();
        clear_alignment();

        let idle = get_recording_telemetry().await.expect("idle telemetry");
        assert!(!idle.active);
        assert!(!idle.diarization.available);
        assert_eq!(idle.diarization.mode, DiarizationMode::Off);
        assert_eq!(idle.diarization.mic.state, DiarChannelState::Unavailable);
        assert_eq!(idle.diarization.sys.state, DiarChannelState::Unavailable);
        assert!(idle.diarization.mic.last_turn.is_none());
        // Every model kind is named even when nothing is running.
        assert_eq!(idle.models.vad.identity, VAD_MODEL_IDENTITY);
        assert!(!idle.models.vad.loaded);
        assert!(!idle.models.asr.loaded);
        assert_eq!(idle.models.asr.engine, None);
        assert!(!idle.models.alignment.loaded);
        // The global context needed to read a score is always present.
        assert_eq!(idle.models.diarization.model_tag, ENHANCED_MODEL_TAG);
        assert_eq!(
            idle.models.diarization.embedding_dim,
            ENHANCED_EMBEDDING_DIM
        );
        assert_eq!(
            idle.models.diarization.recognition_threshold,
            TITANET_RECOGNITION_THRESHOLD
        );
        // No pipeline: no fills are claimed.
        assert_eq!(idle.pipeline.sample_rate, 0);
        assert_eq!(idle.pipeline.mic.vad_dispatch.threshold, 0);

        // A running session: diarization counters, pipeline fills, model activity.
        let stats = begin_stats(DiarizationMode::Fast, true);
        stats.mark_available();
        stats.record_chunk(DiarChannel::Microphone);
        stats.record_embed_ok(DiarChannel::Microphone);
        stats.record_buffered(DiarChannel::Microphone, 2400);

        let pipeline = install_pipeline(48000);
        let mic = pipeline.channel(&DeviceType::Microphone);
        mic.vad_dispatch.set_threshold(9600);
        mic.vad_dispatch.set_fill(4800);
        mic.set_vad_frames(40);
        mic.set_vad_speaking(true);
        mic.mix.set_threshold(28800);

        let running = get_recording_telemetry().await.expect("running telemetry");
        assert!(running.active);
        assert_eq!(running.diarization.mic.chunks, 1);
        assert_eq!(running.diarization.mic.embed_ok, 1);
        assert_eq!(running.diarization.mic.buffered, 1);
        assert_eq!(running.diarization.sys.chunks, 0);
        // Embedded but no stable turn yet: waiting, not a warning.
        assert_eq!(
            running.diarization.mic.state,
            DiarChannelState::Accumulating
        );
        // Fill is reported against the operation it gates, per channel.
        assert_eq!(running.pipeline.sample_rate, 48000);
        assert!((running.pipeline.mic.vad_dispatch.fraction - 0.5).abs() < 0.001);
        assert!(!running.pipeline.mic.vad_dispatch.fired);
        assert_eq!(running.pipeline.mic.vad_dispatch.threshold, 9600);
        assert_eq!(running.pipeline.sys.vad_dispatch.fill, 0);
        assert_eq!(running.pipeline.sys.mix.threshold, 0);
        // Voice activity is visible per channel.
        assert!(running.models.vad.loaded);
        assert_eq!(running.models.vad.mic_frames, 40);
        assert!(running.models.vad.mic_speaking);
        assert!(!running.models.vad.sys_speaking);

        // The wire contract the frontend renders from.
        let json = serde_json::to_value(&running).expect("serialize telemetry");
        assert_eq!(json["active"], true);
        assert_eq!(json["diarization"]["mode"], "fast");
        assert_eq!(json["diarization"]["mic"]["state"], "accumulating");
        assert!(json["pipeline"]["mic"]["vad_dispatch"]["threshold"].is_number());
        // The level meter is part of the wire contract, with its sample age.
        assert!(json["pipeline"]["mic"]["level"]["age_ms"].is_number());
        assert!(json["pipeline"]["mic"]["level"]["rms"].is_number());
        assert_eq!(json["models"]["vad"]["identity"], VAD_MODEL_IDENTITY);

        // Stopped: inactive again, with no stale counters or fills.
        clear_stats();
        clear_pipeline();
        let stopped = get_recording_telemetry().await.expect("stopped telemetry");
        assert!(!stopped.active);
        assert_eq!(stopped.pipeline.sample_rate, 0);
        assert_eq!(stopped.diarization.mic.chunks, 0);
    }

    /// `RecordingManager` cannot be constructed cheaply in a unit test (its
    /// `new()` wires up real device monitoring), so this proves the
    /// short-lock/drop-lock/await/re-lock shape that the mid-recording mic
    /// swap (`device_recovery::attempt_mic_fallback`: Phase 1 takes the mic
    /// stream under a short lock, teardown and Phase 2 stream creation await
    /// with no lock held, Phase 3 re-locks to install) and `stop_recording`'s
    /// Step 1 both use: a slow operation running on an owned, taken-out value
    /// must not block a concurrent lock acquisition on the slot it was taken
    /// from.
    #[tokio::test]
    async fn take_drop_await_restore_does_not_hold_the_lock_across_the_await() {
        use std::time::{Duration, Instant};
        use tokio::sync::Notify;

        let manager_slot: Arc<Mutex<Option<i32>>> = Arc::new(Mutex::new(Some(42)));
        let taken = Arc::new(Notify::new());

        let reconnect_slot = Arc::clone(&manager_slot);
        let reconnect_taken = Arc::clone(&taken);
        let reconnect = tokio::spawn(async move {
            // Mirrors the mic swap phases: take the value out under
            // the lock, drop the lock, then `.await` a slow operation on the
            // owned value before restoring it.
            let value = {
                let mut guard = reconnect_slot.lock_or_recover();
                guard.take()
            };
            reconnect_taken.notify_one();
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut guard = reconnect_slot.lock_or_recover();
            *guard = value;
        });

        // Wait until the reconnect task has taken the value (so it is
        // mid-sleep with the lock already dropped), then prove a concurrent
        // lock acquisition is not blocked by that sleep.
        taken.notified().await;
        let start = Instant::now();
        {
            let _guard = manager_slot.lock_or_recover();
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(50),
            "lock acquisition took {:?}, expected well under the 200ms simulated reconnect",
            elapsed
        );

        reconnect.await.unwrap();
        assert_eq!(*manager_slot.lock_or_recover(), Some(42));
    }
}
