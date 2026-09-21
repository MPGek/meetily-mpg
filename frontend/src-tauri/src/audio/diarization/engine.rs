//! `DiarizationEngine`: the entry point every other module uses for speaker
//! diarization (05 D2). It owns the live session's process-wide state and
//! wraps the offline pass, so no caller reaches into the tree's internals.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use log::{info, warn};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use sqlx::SqlitePool;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use super::identity::prototypes::PrototypeStore;
use super::telemetry::{
    begin_stats, clear_stats, current_stats, ChannelStatusLine, DiarChannel, DiarChannelState,
    DiarizationMode, OnlineDiarizationStatus,
};
use crate::audio::recording_state::AudioChunk;
use super::streaming::units::{OnlineClusterEmbeddings, SpeakerAssignment, SpeakerTurn};
use crate::audio::embedder::{
    ENHANCED_EMBEDDING_DIM, ENHANCED_MODEL_TAG, TITANET_RECOGNITION_THRESHOLD,
};
use crate::audio::recording_saver::TranscriptSegment;
use crate::database::repositories::speaker::SpeakerRepository;
use super::streaming::processor::OnlineDiarizationProcessor;
use crate::audio::sync_ext::LockRecover;

/// The diarization capability's public surface.
pub struct DiarizationEngine;

// Online diarization worker: consumes embedding chunks and returns the
// processor (for finalize) when the channel closes
static ONLINE_DIARIZATION_TASK: Mutex<
    Option<JoinHandle<Result<Option<OnlineDiarizationProcessor>, String>>>,
> = Mutex::new(None);

// Shared prototype store for live Fast-mode recognition (design D6).
// Created at recording start, shared between the processor task and the
// assign_live_speaker command. Cleared at recording stop.
static ONLINE_DIARIZATION_STORE: Mutex<Option<Arc<RwLock<PrototypeStore>>>> =
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

/// What a live session needs to start. Fixed for the whole session: a
/// mid-recording settings change must not split it (05 D4).
pub struct LiveSessionConfig {
    /// Parsed from the frontend's `diarizationMode` string.
    pub mode: DiarizationMode,
    /// The user's speaker maximum; `None` means the configured ceiling.
    pub max_speakers: Option<i32>,
    /// True when a system-audio device was selected, which fixes the session's
    /// microphone label prefix.
    pub has_system_device: bool,
    pub expected_speaker_ids: Vec<String>,
}

impl DiarizationEngine {
    /// Read access to the live prototype store while a session runs, for the
    /// speaker commands that clear or inspect it. Non-blocking: a speaker
    /// command must never wait on a live session, so a contended lock reports
    /// "no store" rather than blocking (05 task 5.7).
    pub fn live_prototype_store() -> Option<Arc<RwLock<PrototypeStore>>> {
        ONLINE_DIARIZATION_STORE
            .try_lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Start a live diarization session: clear state from any previous run,
    /// load the prototype store for live recognition, construct the processor
    /// and spawn the task that drains speech chunks until the channel closes.
    ///
    /// Returns the sender the recording pipeline should feed, or `None` when
    /// diarization is off for this session or its models are unavailable (the
    /// frontend is told through `online-diarization-unavailable`, as before).
    pub async fn start_live_session<R: Runtime>(
        app: &AppHandle<R>,
        cfg: LiveSessionConfig,
    ) -> Option<Sender<AudioChunk>> {
        // Every new session starts from zero diarization activity: drop any
        // counters left behind by a previous run (including one that never
        // reached stop), so a crashed or diarization-off session can never
        // show stale values.
        clear_stats();
        let mut chunk_sender: Option<Sender<AudioChunk>> = None;

        // Store expected speaker IDs so stop_recording can include them in session data.
        {
            let mut stored_ids = ONLINE_EXPECTED_SPEAKER_IDS.lock_or_recover();
            *stored_ids = cfg.expected_speaker_ids.clone();
        }

        // Fresh live word-diarization session: clear live turns + the provisional
        // block set so no state from a previous recording leaks in
        // (live-word-level-diarization).
        super::streaming::reconcile::reset_session();

        if cfg.mode.is_online() {
            // Bounded: same reasoning as `transcription_sender` (already-coalesced
            // segments, so 32 pending is a large multi-minute backlog).
            let (embedding_sender, embedding_receiver) =
                tokio::sync::mpsc::channel::<AudioChunk>(32);

            // The microphone label prefix is fixed for the whole session by the
            // caller's device choice: a stereo session namespaces mic clusters
            // as MIC_SPEAKER_NN, a mono one uses SPEAKER_NN.
            let has_system_device = cfg.has_system_device;

            // Live status counters for this session (online-diarization-telemetry).
            // Installed before the processor starts, so the status lines report
            // unavailable (not stale values) if model initialization fails.
            let stats = begin_stats(cfg.mode, has_system_device);

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
            let candidate_ids = if cfg.expected_speaker_ids.is_empty() {
                None
            } else {
                Some(cfg.expected_speaker_ids.clone())
            };
            // Fresh session: clear any stale per-turn overrides from a previous run.
            ONLINE_TURN_OVERRIDES.lock_or_recover().clear();
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
                        let mut global_store = ONLINE_DIARIZATION_STORE.lock_or_recover();
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
                    let max_speakers_usize = cfg.max_speakers.filter(|m| *m > 0).unwrap_or(0) as usize;
                    let mut processor = match OnlineDiarizationProcessor::new_with_app(
                        &app_for_processor,
                        cfg.mode,
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
                    // Attach the live status counters so the two status lines can
                    // report this channel's progress while the recording runs.
                    processor.attach_stats(stats);
                    let mut receiver = embedding_receiver;
                    while let Some(chunk) = receiver.blocking_recv() {
                        processor.process_chunk(chunk);
                    }
                    Ok(Some(processor))
                },
            );
            {
                let mut global_task = ONLINE_DIARIZATION_TASK.lock_or_recover();
                *global_task = Some(task);
            }
            info!(
                "🎙️ Online diarization processor spawned (mode: {:?})",
                cfg.mode
            );
            chunk_sender = Some(embedding_sender);
        } else {
            info!("ℹ️ Online diarization disabled (mode: {:?})", cfg.mode);
        }

        chunk_sender
    }
}

impl DiarizationEngine {
    /// Finish a live session at recording stop: join the drain task, run the
    /// stop-time token repair against the saved meeting audio, and produce the
    /// per-transcript speaker assignments. The cluster embeddings and live
    /// bindings are held for `persist_session`, which runs once the frontend
    /// has created the meeting row.
    ///
    /// `transcripts` and `meeting_folder` are read from the recording manager
    /// by the caller: the manager is `!Sync` (it owns cpal streams), so no
    /// reference to it may be held across this await.
    ///
    /// `None` when no session was active or the processor never became
    /// available; every failure mode is logged, never surfaced as an error,
    /// exactly as at the call site this came from.
    pub async fn finalize_session(
        transcripts: Option<Vec<TranscriptSegment>>,
        meeting_folder: Option<PathBuf>,
    ) -> Option<Vec<SpeakerAssignment>> {
        // The embedding channel was closed when the pipeline stopped, so the consumer
        // task has drained all speech chunks and returned the processor.
        let online_task = {
            let mut global_task = ONLINE_DIARIZATION_TASK.lock_or_recover();
            global_task.take()
        };

        // The recording is over: stop serving this session's live counters so
        // a stopped session's values are never presented as live
        // (online-diarization-telemetry). The caller clears the pipeline, ASR
        // and alignment telemetry it owns.
        clear_stats();

        if let Some(task_handle) = online_task {
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
                        let stored_expected = ONLINE_EXPECTED_SPEAKER_IDS.lock_or_recover().clone();
                        {
                            let mut session_data = ONLINE_SESSION_DATA.lock_or_recover();
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
        }
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

/// One live speaker correction, as the command receives it.
pub struct LiveSpeakerAssignment {
    pub cluster_label: String,
    pub speaker_id: Option<String>,
    pub new_name: Option<String>,
    /// `"block"` records a single-turn override; anything else binds the
    /// whole cluster for the rest of the session.
    pub scope: Option<String>,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
}

impl DiarizationEngine {
    /// Persist a finalized live session once the meeting row exists: cluster
    /// centroids and exemplar caches, auto-recognition, enrollment of the
    /// session's embeddings, the user's live bindings and per-turn overrides,
    /// and the expected-speaker allowlist.
    ///
    /// No-op (all-zero summary) when no session is pending, so the frontend
    /// may finalize unconditionally.
    pub async fn persist_session(
        pool: &SqlitePool,
        meeting_id: String,
    ) -> Result<serde_json::Value, String> {

        // Take the pending session data. When none is present (e.g. a non-online
        // recording that still navigates through finalize), no-op gracefully so the
        // frontend may finalize unconditionally and never drop live bindings.
        let Some(session_data) = ONLINE_SESSION_DATA.lock_or_recover().take() else {
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
            super::persist_and_recognize_session(
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
            let mut store = ONLINE_DIARIZATION_STORE.lock_or_recover();
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

    /// Read-only snapshot of the running session's per-channel counters, or
    /// an inactive shape when nothing is running.
    pub async fn telemetry_snapshot() -> Result<OnlineDiarizationStatus, String> {
        let stats = current_stats();
        let registry = super::streaming::reconcile::registry();

        let (prototypes, bindings) = {
            let store = ONLINE_DIARIZATION_STORE.lock_or_recover();
            let read = store.as_ref().and_then(|s| s.read().ok());
            match read {
                Some(read) => (
                    Some(read.prototypes.len()),
                    Some(read.bindings().len()),
                ),
                None => (None, None),
            }
        };

        let (active, mode, available, mic, sys) = match stats.as_deref() {
            Some(stats) => (
                stats.mode().is_online(),
                stats.mode(),
                stats.is_available(),
                stats.line(DiarChannel::Microphone, &registry),
                stats.line(DiarChannel::System, &registry),
            ),
            None => {
                let unavailable = |channel| ChannelStatusLine {
                    channel,
                    state: DiarChannelState::Unavailable,
                    chunks: 0,
                    embed_ok: 0,
                    embed_failed: 0,
                    buffered: 0,
                    buffered_secs: 0.0,
                    turns: 0,
                    ordered: true,
                    last_turn: None,
                };
                (
                    false,
                    DiarizationMode::Off,
                    false,
                    unavailable(DiarChannel::Microphone),
                    unavailable(DiarChannel::System),
                )
            }
        };

        let (blocks_sent, blocks_completed, blocks_in_flight) = match stats.as_deref() {
            Some(stats) => (
                stats.blocks_sent_total(),
                stats.blocks_completed_total(),
                stats.blocks_in_flight_now(),
            ),
            None => (0, 0, false),
        };

        Ok(OnlineDiarizationStatus {
            active,
            mode,
            available,
            model_tag: ENHANCED_MODEL_TAG.to_string(),
            embedding_dim: ENHANCED_EMBEDDING_DIM,
            recognition_threshold: TITANET_RECOGNITION_THRESHOLD,
            prototypes,
            bindings,
            pending_blocks: blocks_sent.saturating_sub(blocks_completed),
            blocks_sent,
            blocks_processed: blocks_completed,
            blocks_in_flight,
            mic,
            sys,
        })
    }

    /// Apply a live speaker correction: bind the whole cluster, or record a
    /// single-turn override when `scope == "block"`. Fails loudly when no live
    /// prototype store is active, so a correction is never silently dropped.
    pub async fn assign_live_speaker(
        pool: &SqlitePool,
        req: LiveSpeakerAssignment,
    ) -> Result<crate::database::speaker_commands::AssignedSpeaker, String> {
        let LiveSpeakerAssignment {
            cluster_label,
            speaker_id,
            new_name,
            scope,
            start_time,
            end_time,
        } = req;

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
            ONLINE_TURN_OVERRIDES.lock_or_recover().push(TurnOverride {
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
            let store_guard = ONLINE_DIARIZATION_STORE.lock_or_recover();
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
}

/// One offline batch run over a saved meeting.
pub struct BatchRequest {
    pub meeting_id: String,
    /// The user's speaker maximum; `None` means the configured ceiling.
    pub max_speakers: Option<i32>,
}

impl DiarizationEngine {
    /// Run the offline pass over a saved meeting's audio (05 task 5.8): the
    /// one symbol a caller outside this tree needs for batch diarization.
    pub async fn start_batch<R: Runtime>(
        app: AppHandle<R>,
        req: BatchRequest,
        state: tauri::State<'_, crate::state::AppState>,
    ) -> Result<super::DiarizationResult, String> {
        super::batch::orchestrator::run_offline_diarization(
            app,
            req.meeting_id,
            req.max_speakers,
            state,
        )
        .await
    }

    /// Ask a running batch pass to stop at its next cancellation point.
    pub fn cancel_batch() {
        super::commands::cancel_diarization();
    }

    /// Whether a batch pass is running (only one runs at a time).
    pub fn is_batch_running() -> bool {
        super::commands::is_diarization_in_progress()
    }

    /// Override the stored clustering parameters for subsequent runs. A live
    /// session keeps the values it resolved at its start (D4).
    pub fn set_clustering_settings(
        cluster_threshold: Option<f32>,
        cluster_ceiling: Option<usize>,
        gap_merge_secs: Option<f32>,
        clusterer: Option<super::ClustererKindSetting>,
    ) {
        super::config::set_clustering_overrides(
            cluster_threshold,
            cluster_ceiling,
            gap_merge_secs,
            clusterer,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    /// A live correction must never be silently dropped: with no session
    /// running there is no prototype store to bind into, so the command has to
    /// fail rather than report success and let the label revert at stop.
    #[tokio::test]
    async fn assign_live_speaker_without_a_session_is_an_error() {
        let pool = setup_pool().await;
        {
            // No live session: the store slot is empty.
            let mut store = ONLINE_DIARIZATION_STORE.lock_or_recover();
            *store = None;
        }

        let result = DiarizationEngine::assign_live_speaker(
            &pool,
            LiveSpeakerAssignment {
                cluster_label: "SPEAKER_00".to_string(),
                speaker_id: None,
                new_name: Some("Someone".to_string()),
                scope: None,
                start_time: None,
                end_time: None,
            },
        )
        .await;

        let err = match result {
            Err(err) => err,
            Ok(_) => panic!("a cluster binding with no live session must fail"),
        };
        assert!(
            err.contains("No live diarization session active"),
            "error must name the missing session, got: {err}"
        );
    }
}
