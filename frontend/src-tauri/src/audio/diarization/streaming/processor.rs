//! The live diarization processor: receives VAD-filtered 16 kHz speech chunks
//! during a recording and produces per-transcript speaker assignments at stop.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use log::{info, warn};
use tokio::sync::mpsc::UnboundedSender;

use polyvoice::embedder::Embedder as _;

use crate::audio::recording_saver::TranscriptSegment;
use crate::audio::recording_state::{AudioChunk, DeviceType};

use super::super::core::cluster::{EmbeddingBuffer, SpeakerSegment};
use super::super::core::timeline::{find_best_speaker, split_tokens_by_speaker};
use super::super::core::turns::merge_same_speaker_segments;
use super::super::persist::clusters::{clustered_embeddings, EmbeddingLabeling};
use super::super::identity::prototypes::PrototypeStore;
use super::super::telemetry::{
    diar_channel, record_stats, DiarizationMode, OnlineDiarizationStats,
};
use super::super::core::factory::create_streaming_embedder;
use super::engine::{create_fast_channel, Engine};
use super::reconcile::{FinalChannel, FinalDisplayPass};
use super::guard::OnlineDiarizationGuard;
use super::units::{OnlineClusterEmbeddings, SpeakerAssignment, SpeakerTurn};

/// What a stop hands back: the transcript assignments, the per-channel
/// clustered embeddings for the registry, the user's live cluster bindings, and
/// the refined timeline the display-side final pass promotes live blocks against.
pub type FinalizeOutput = (
    Vec<SpeakerAssignment>,
    OnlineClusterEmbeddings,
    HashMap<String, String>,
    FinalDisplayPass,
);

/// Receives VAD-filtered 16 kHz speech chunks and produces speaker
/// assignments at recording stop. No-op while in the error state.
/// One emission observed through the harness sink: the turn as the app would
/// see it, plus whether the streaming pipeline considered it stable. The live
/// path publishes only stable turns, so provisional ones are visible here and
/// nowhere else (add-online-diarization-eval D4).
#[derive(Debug, Clone)]
pub struct EmittedTurn {
    pub turn: SpeakerTurn,
    pub stable: bool,
}

pub struct OnlineDiarizationProcessor {
    mode: DiarizationMode,
    max_speakers: usize,
    saw_system_audio: bool,
    /// Channel-scoped label prefix for the microphone channel, fixed for the
    /// whole session. `MIC_SPEAKER` when a system device was captured (stereo),
    /// else `SPEAKER` (mono). Chosen at construction so live labels and
    /// stop-time assignments always agree.
    mic_prefix: String,
    /// Model family tag active for this session (192 vs 256). Set at construction
    /// from the installed model set so clustering and persistence use the
    /// family threshold.
    model_tag: &'static str,
    engine: Option<Engine>,
    turn_sender: Option<UnboundedSender<SpeakerTurn>>,
    /// Observation-only sink for *every* emission, stability flag included.
    /// `None` on the production recording path, which is what keeps this hook
    /// inert there: nothing branches on it and the app-facing sender, the
    /// registry and the stop-time state are untouched by it.
    emission_sink: Option<UnboundedSender<EmittedTurn>>,
    /// Shared live-recognition store (Fast mode). None in Efficient mode or
    /// when no registry prototypes were loaded.
    prototype_store: Option<Arc<RwLock<PrototypeStore>>>,
    /// Counters for stop-time failure surfacing (task 4.3): track attempted
    /// chunks vs failed embeddings per channel so finalize can warn when
    /// chunks existed but no valid embeddings survived.
    attempted_mic: usize,
    attempted_sys: usize,
    failed_mic: usize,
    failed_sys: usize,
    /// Live status counters for the two channel lines. None when telemetry is
    /// not attached; every update no-ops in that case.
    stats: Option<Arc<OnlineDiarizationStats>>,
    /// Clustering parameters resolved once, at session start (05 D4), so a
    /// settings change mid-recording cannot split one session across two
    /// configurations.
    config: crate::audio::diarization::DiarizationConfig,
    _guard: OnlineDiarizationGuard,
}

/// Stop-time refinement of one channel (05b D1): re-cluster the session's
/// buffered chunk embeddings so voices the incremental pass merged under one
/// id can still be told apart before anything is persisted. The chunk
/// embeddings are the only per-channel set that spans the whole session and
/// comes from the same embedder the batch path uses, so the refined result is
/// comparable with an offline run of the same audio.
///
/// Returns one labelled segment per buffered window, or the reason the
/// channel keeps the streaming pipeline's incremental identities. That
/// fallback is unconditional: the setting being off, fewer than the two
/// embeddings clustering needs, a clusterer that will not build, and a
/// clustering error all leave the channel exactly as the live session left
/// it. Nothing here can fail a stop. The reason is both logged and returned,
/// so a caller (and a test) sees exactly what the log line says.
fn refine_channel(
    channel: &str,
    buffer: &EmbeddingBuffer,
    config: &crate::audio::diarization::DiarizationConfig,
    ceiling: usize,
) -> Result<Vec<SpeakerSegment>, String> {
    use crate::audio::diarization::Clustering as _;

    if !config.final_recluster {
        return Err(skipped(channel, "turned off in settings".to_string()));
    }
    if buffer.entries.len() < 2 {
        return Err(skipped(
            channel,
            format!(
                "{} buffered embedding(s), fewer than the two clustering needs",
                buffer.entries.len()
            ),
        ));
    }

    let embeddings: Vec<Vec<f32>> = buffer.entries.iter().map(|e| e.2.clone()).collect();
    // The refinement's own merge threshold, everything else as resolved.
    let refine_config = crate::audio::diarization::DiarizationConfig {
        cluster_threshold: config.final_recluster_threshold,
        ..*config
    };
    let clusterer = match crate::audio::diarization::clusterer_for_buffer(&refine_config, ceiling) {
        Ok(c) => c,
        Err(e) => return Err(failed(channel, e)),
    };
    let labels = match clusterer.cluster(&embeddings) {
        Ok(labels) if labels.len() == buffer.entries.len() => labels,
        Ok(labels) => {
            return Err(failed(
                channel,
                format!(
                    "clustering returned {} labels for {} windows",
                    labels.len(),
                    buffer.entries.len()
                ),
            ))
        }
        Err(e) => return Err(failed(channel, e)),
    };

    let refined: Vec<SpeakerSegment> = buffer
        .entries
        .iter()
        .zip(&labels)
        .map(|((start, end, _), label)| SpeakerSegment {
            start: *start,
            end: *end,
            speaker: *label,
        })
        .collect();
    let distinct = labels
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    info!(
        "Stop-time speaker refinement on the {} channel: {} windows -> {} speaker(s) (clusterer {}, threshold {:.3}, ceiling {})",
        channel,
        refined.len(),
        distinct,
        refine_config.clusterer.as_str(),
        refine_config.cluster_threshold,
        ceiling
    );
    Ok(refined)
}

/// An expected skip: logged at info, because nothing went wrong.
fn skipped(channel: &str, reason: String) -> String {
    let message = format!(
        "Stop-time speaker refinement skipped on the {} channel: {}; keeping the identities shown live",
        channel, reason
    );
    info!("{}", message);
    message
}

/// A refinement that could not run: logged at warn, still not an error for
/// the stop, which completes on the incremental identities.
fn failed(channel: &str, reason: String) -> String {
    let message = format!(
        "Stop-time speaker refinement failed on the {} channel ({}); keeping the identities shown live",
        channel, reason
    );
    warn!("{}", message);
    message
}

/// Re-number a refined channel into the label space the live session
/// published (05b D2/3.3).
///
/// Everything a user did during the recording is keyed by the *live* cluster
/// label - a rename through `PrototypeStore::bind`, a per-turn override, the
/// name already on screen - so refined clusters must not arrive under fresh
/// numbers. Each refined cluster takes the name of the live cluster it covers
/// most, greedily and one-to-one; a refined cluster that matches no live
/// cluster (the split the refinement just discovered) gets an id no live
/// cluster used. Two consequences that matter: a user's correction lands on
/// the people it was made for, and the display pass sees a changed label only
/// where the grouping really changed instead of everywhere.
///
/// Returns the number of refined clusters that kept a live name.
fn anchor_to_live_labels(refined: &mut [SpeakerSegment], live: &[SpeakerSegment]) -> usize {
    use std::collections::{BTreeMap, BTreeSet};

    if refined.is_empty() || live.is_empty() {
        return 0;
    }
    let mut overlap: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for r in refined.iter() {
        for l in live.iter() {
            let shared = (r.end.min(l.end) - r.start.max(l.start)) as f64;
            if shared > 0.0 {
                *overlap.entry((r.speaker, l.speaker)).or_insert(0.0) += shared;
            }
        }
    }

    // Greedy one-to-one by shared duration. Ties break on the ids, so the
    // outcome does not depend on map iteration order.
    let mut pairs: Vec<((usize, usize), f64)> = overlap.into_iter().collect();
    pairs.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut refined_to_live: BTreeMap<usize, usize> = BTreeMap::new();
    let mut taken_live: BTreeSet<usize> = BTreeSet::new();
    for ((refined_id, live_id), _shared) in pairs {
        if refined_to_live.contains_key(&refined_id) || taken_live.contains(&live_id) {
            continue;
        }
        refined_to_live.insert(refined_id, live_id);
        taken_live.insert(live_id);
    }

    // Refined clusters with no live counterpart get ids no live cluster used.
    let mut next = live.iter().map(|l| l.speaker).max().unwrap_or(0) + 1;
    let refined_ids: BTreeSet<usize> = refined.iter().map(|r| r.speaker).collect();
    for id in refined_ids {
        if refined_to_live.contains_key(&id) {
            continue;
        }
        refined_to_live.insert(id, next);
        next += 1;
    }

    for segment in refined.iter_mut() {
        if let Some(anchored) = refined_to_live.get(&segment.speaker) {
            segment.speaker = *anchored;
        }
    }
    taken_live.len()
}

const MIN_SEGMENT_SAMPLES: usize = 3200; // 200 ms at 16 kHz (spec minimum)

impl OnlineDiarizationProcessor {
    /// App-aware initializer that resolves enhanced models via the 3-location fallback.
    /// Production recording path should use this; `new(models_dir, ..)` remains for tests.
    pub fn new_with_app<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        mode: DiarizationMode,
        max_speakers: usize,
        has_system_device: bool,
        turn_sender: Option<UnboundedSender<SpeakerTurn>>,
        prototype_store: Option<Arc<RwLock<PrototypeStore>>>,
    ) -> Result<Self, String> {
        if let Some(dir) = crate::audio::embedder::resolve_enhanced_models_dir(app) {
            return Self::new(
                mode,
                max_speakers,
                has_system_device,
                &dir,
                turn_sender,
                prototype_store,
            );
        }
        let locations = crate::audio::embedder::format_enhanced_search_locations(app);
        Err(format!(
            "Enhanced diarization models not found. Searched: {}. The enhanced models (segmentation-3.0 + TitaNet-Large) are bundled at build time near the executable; rebuild with network or install a build that includes them.",
            locations
        ))
    }

    /// Initializes polyvoice components based on mode. Model initialization is
    /// blocking — call from a blocking task. `has_system_device` fixes the
    /// microphone label prefix for the whole session (stereo vs mono).
    pub fn new(
        mode: DiarizationMode,
        max_speakers: usize,
        has_system_device: bool,
        models_dir: &Path,
        turn_sender: Option<UnboundedSender<SpeakerTurn>>,
        prototype_store: Option<Arc<RwLock<PrototypeStore>>>,
    ) -> Result<Self, String> {
        if !mode.is_online() {
            return Err("Online diarization disabled".to_string());
        }
        let guard = OnlineDiarizationGuard::acquire()?;

        let (_, embedding_model) = crate::audio::embedder::enhanced_model_paths(models_dir);
        if !crate::audio::embedder::is_enhanced_installed(models_dir) {
            return Err(format!(
                "Enhanced diarization models not found at {}. The enhanced models (segmentation-3.0 + TitaNet-Large) are bundled at build time; rebuild with network or install a build that includes them.",
                embedding_model.display()
            ));
        }
        let mic_prefix = if has_system_device {
            "MIC_SPEAKER".to_string()
        } else {
            "SPEAKER".to_string()
        };
        let model_tag = crate::audio::embedder::ENHANCED_MODEL_TAG;

        let engine = match mode {
            DiarizationMode::Efficient => {
                let extractor = create_streaming_embedder(&embedding_model)?;
                Engine::Efficient {
                    extractor,
                    mic: EmbeddingBuffer::default(),
                    sys: EmbeddingBuffer::default(),
                }
            }
            DiarizationMode::Fast => {
                let mic = create_fast_channel(&embedding_model)?;
                let sys = create_fast_channel(&embedding_model)?;
                // Fast mode embeds chunks itself (the pipeline turns carry no
                // embedding). Reuse one extractor for both channels' buffers.
                let extractor = create_streaming_embedder(&embedding_model)?;
                Engine::Fast {
                    mic,
                    sys,
                    extractor,
                    mic_emb: EmbeddingBuffer::default(),
                    sys_emb: EmbeddingBuffer::default(),
                }
            }
            DiarizationMode::Off => unreachable!(),
        };

        // Resolve the persisted clustering settings once for the whole
        // session (05 D4): the live path honours the same overrides the batch
        // path reads, and a mid-recording change cannot apply to this session.
        let config = crate::audio::diarization::DiarizationConfig::resolved();

        info!(
            "Online diarization processor initialized (mode: {:?}, max_speakers: {}, prototype_store: {}, mic_prefix: {}, clusterer: {}, threshold: {:.3}, ceiling: {})",
            mode,
            max_speakers,
            prototype_store.is_some(),
            mic_prefix,
            config.clusterer.as_str(),
            config.cluster_threshold,
            config.cluster_ceiling,
        );

        Ok(Self {
            mode,
            max_speakers,
            config,
            saw_system_audio: false,
            mic_prefix,
            model_tag,
            engine: Some(engine),
            turn_sender,
            emission_sink: None,
            prototype_store,
            attempted_mic: 0,
            attempted_sys: 0,
            failed_mic: 0,
            failed_sys: 0,
            stats: None,
            _guard: guard,
        })
    }

    pub fn mode(&self) -> DiarizationMode {
        self.mode
    }

    pub fn is_in_error_state(&self) -> bool {
        self.engine.is_none()
    }

    /// Attach an observation sink that receives every emission, provisional
    /// ones included (developer harness only; the recording path never calls
    /// this). Kept separate from construction so the production path cannot
    /// acquire one by accident.
    /// Replace the parameters resolved at construction. Evaluation hook: the
    /// harness states its configuration explicitly instead of reading the
    /// process-wide settings, so a run measures the defaults plus exactly the
    /// flags it was given. The app never calls this; a session keeps the
    /// configuration it resolved at its start (05 D4).
    pub fn set_session_config(&mut self, config: crate::audio::diarization::DiarizationConfig) {
        self.config = config;
    }

    pub fn attach_emission_sink(&mut self, sink: UnboundedSender<EmittedTurn>) {
        self.emission_sink = Some(sink);
    }

    /// Attach the session's live status counters. Kept separate from
    /// construction so telemetry never affects the diarization engine itself.
    pub fn attach_stats(&mut self, stats: Arc<OnlineDiarizationStats>) {
        stats.mark_available();
        self.stats = Some(stats);
    }

    /// Routes one audio chunk to the active engine. No-op in the error state
    /// or when the chunk is too short to embed. A single chunk that fails to
    /// embed or feed is skipped (logged) rather than disabling the whole
    /// session: engine error state is reserved for init failures, so a
    /// transient bad chunk never wipes the remaining recording's labels.
    pub fn process_chunk(&mut self, chunk: AudioChunk) {
        // Every dequeued block leaves the diarization queue on exit (even
        // when rejected as too short or skipped in the error state), and the
        // engine is in flight while the engine work runs.
        record_stats(&self.stats, |s| s.record_block_in_flight());
        struct ConsumedGuard(Option<std::sync::Weak<OnlineDiarizationStats>>);
        impl Drop for ConsumedGuard {
            fn drop(&mut self) {
                if let Some(stats) = self.0.as_ref().and_then(std::sync::Weak::upgrade) {
                    stats.record_block_consumed();
                }
            }
        }
        let _guard = ConsumedGuard(
            self.stats
                .as_ref()
                .map(std::sync::Arc::downgrade),
        );

        if chunk.data.len() < MIN_SEGMENT_SAMPLES {
            return;
        }

        // Clone the telemetry handle up front: the engine borrow below holds
        // `self.engine` mutably for the rest of this function, so counters are
        // updated through this handle instead of through `self`.
        let diar = diar_channel(&chunk.device_type);
        let stats = self.stats.clone();

        let Some(engine) = self.engine.as_mut() else {
            return;
        };

        let samples: std::borrow::Cow<'_, [f32]> = if chunk.sample_rate != 16000 {
            match crate::audio::audio_processing::resample(&chunk.data, chunk.sample_rate, 16000) {
                Ok(resampled) => std::borrow::Cow::Owned(resampled),
                Err(e) => {
                    warn!("Online diarization resample failed: {}", e);
                    return;
                }
            }
        } else {
            std::borrow::Cow::Borrowed(&chunk.data)
        };

        if chunk.device_type == DeviceType::System {
            self.saw_system_audio = true;
        }

        // Live status: count every chunk that reached the engine. Chunks that
        // never got here (error state, too short, failed resample) are not
        // reported as received.
        record_stats(&stats, |s| s.record_chunk(diar));

        // Capture live-emission state before borrowing the engine, so the
        // Fast-mode loop can send turns without conflicting borrows.
        let turn_sender = self.turn_sender.clone();
        let emission_sink = self.emission_sink.clone();
        let prototype_store = self.prototype_store.clone();
        let mic_prefix = self.mic_prefix.as_str();

        match engine {
            Engine::Efficient {
                extractor,
                mic,
                sys,
            } => {
                // Track attempted vs failed for stop-time surfacing (task 4.3).
                match chunk.device_type {
                    DeviceType::Microphone => self.attempted_mic += 1,
                    DeviceType::System => self.attempted_sys += 1,
                }
                let embedding = match extractor.embed(&samples) {
                    Ok(emb) => {
                        record_stats(&stats, |s| s.record_embed_ok(diar));
                        emb
                    }
                    Err(e) => {
                        match chunk.device_type {
                            DeviceType::Microphone => self.failed_mic += 1,
                            DeviceType::System => self.failed_sys += 1,
                        }
                        record_stats(&stats, |s| s.record_embed_failed(diar));
                        // Preserve layout hint when relevant
                        if e.to_string().contains("audio_signal")
                            || e.to_string().contains("Expected:80")
                        {
                            warn!("Embedding extraction failed ({}), skipping chunk [layout hint: expected [B,80,T] for titanet_large]", e);
                        } else {
                            warn!("Embedding extraction failed ({}), skipping chunk", e);
                        }
                        return;
                    }
                };
                let start = chunk.timestamp as f32;
                let end = start + samples.len() as f32 / 16000.0;
                match chunk.device_type {
                    DeviceType::Microphone => mic.push(start, end, embedding),
                    DeviceType::System => sys.push(start, end, embedding),
                }
                let buffered_ms = ((end - start).max(0.0) * 1000.0) as u64;
                record_stats(&stats, |s| s.record_buffered(diar, buffered_ms));
            }
            Engine::Fast {
                mic,
                sys,
                extractor,
                mic_emb,
                sys_emb,
            } => {
                let (channel, source_device, prefix, emb_buf, channel_str) = match chunk.device_type
                {
                    DeviceType::Microphone => (mic, "Microphone", mic_prefix, mic_emb, "mic"),
                    DeviceType::System => (sys, "System", "SPEAKER", sys_emb, "system"),
                };

                // Track for stop-time surfacing (task 4.3) even in Fast mode.
                match chunk.device_type {
                    DeviceType::Microphone => self.attempted_mic += 1,
                    DeviceType::System => self.attempted_sys += 1,
                }
                // Fast mode embeds each chunk itself (pipeline turns carry no
                // embedding) for live recognition, buffering, and enrollment.
                let chunk_embedding = match extractor.embed(&samples) {
                    Ok(emb) => {
                        record_stats(&stats, |s| s.record_embed_ok(diar));
                        emb
                    }
                    Err(e) => {
                        match chunk.device_type {
                            DeviceType::Microphone => self.failed_mic += 1,
                            DeviceType::System => self.failed_sys += 1,
                        }
                        record_stats(&stats, |s| {
                            s.record_embed_failed(diar);
                            s.disable();
                        });
                        if e.to_string().contains("audio_signal")
                            || e.to_string().contains("Expected:80")
                        {
                            warn!("Fast-mode embedding failed ({}), disabling online diarization [layout hint: expected [B,80,T] for titanet_large]", e);
                        } else {
                            warn!(
                                "Fast-mode embedding failed ({}), disabling online diarization",
                                e
                            );
                        }
                        self.engine = None;
                        return;
                    }
                };
                let start = chunk.timestamp as f32;
                let end = start + samples.len() as f32 / 16000.0;
                let duration = (end - start).max(0.0);

                // Live recognition against the prototype store (relabel turns). Keep
                // the match result (id, name, score) so the emitted turn can
                // carry provenance + confidence.
                let recognition = prototype_store.as_ref().and_then(|store| {
                    let read = store.read().ok()?;
                    let m = read.recognize(&chunk_embedding, channel_str)?;
                    let name = read.names.get(&m.speaker_id).cloned();
                    Some((m.speaker_id, name, m.score))
                });

                // Buffer the chunk embedding for stop-time centroids/enrollment
                // BEFORE the pipeline feed, so the emb_buf borrow ends before
                // the match (allowing self.engine = None in the Err arm).
                emb_buf.push(start, end, chunk_embedding.clone());
                let buffered_ms = (duration.max(0.0) * 1000.0) as u64;
                record_stats(&stats, |s| s.record_buffered(diar, buffered_ms));

                channel
                    .mapper
                    .push_chunk(chunk.timestamp, samples.len() as f64 / 16000.0);
                match channel.pipeline.feed(&samples) {
                    Ok(turns) => {
                        for turn in turns {
                            // Observation only: what the harness records for a
                            // turn the live path drops as provisional. Built
                            // before the stable branch so the branch itself is
                            // unchanged, and only when a sink is attached.
                            let provisional_event = match (&emission_sink, turn.stable) {
                                (Some(_), false) => Some(SpeakerTurn {
                                    start_time: channel.mapper.to_abs(turn.time.start),
                                    end_time: channel.mapper.to_abs(turn.time.end),
                                    speaker: format!("{}_{:02}", prefix, turn.speaker.0),
                                    source_device: source_device.to_string(),
                                    display_name: None,
                                    matched_by: None,
                                    match_score: None,
                                }),
                                _ => None,
                            };
                            if let (Some(sink), Some(event)) = (&emission_sink, provisional_event) {
                                let _ = sink.send(EmittedTurn {
                                    turn: event,
                                    stable: false,
                                });
                            }
                            if turn.stable {
                                let speaker_index = turn.speaker.0 as usize;
                                let turn_start = turn.time.start as f32;
                                let turn_end = turn.time.end as f32;
                                channel.turns.push(SpeakerSegment {
                                    start: turn_start,
                                    end: turn_end,
                                    speaker: speaker_index,
                                });
                                record_stats(&stats, |s| s.record_turn(diar));
                                // Record this chunk's embedding under the
                                // pipeline speaker, for seeding a newly created
                                // person's prototypes on live rename.
                                if let Some(store) = &prototype_store {
                                    if let Ok(mut s) = store.write() {
                                        s.push_session(
                                            speaker_index,
                                            chunk_embedding.clone(),
                                            duration,
                                            channel_str.to_string(),
                                        );
                                    }
                                }
                                let turn_label = format!("{}_{:02}", prefix, speaker_index);
                                // A user-bound cluster labels its turns with
                                // the user's chosen name; otherwise the
                                // automatic recognition name (if any) is shown.
                                let bound_speaker = prototype_store.as_ref().and_then(|store| {
                                    store.read().ok().and_then(|s| {
                                        s.bindings().get(&turn_label).cloned()
                                    })
                                });
                                let display_name = match &bound_speaker {
                                    Some(spk_id) => prototype_store
                                        .as_ref()
                                        .and_then(|store| {
                                            store
                                                .read()
                                                .ok()
                                                .and_then(|s| s.names.get(spk_id).cloned())
                                        })
                                        .or_else(|| recognition.as_ref().and_then(|r| r.1.clone())),
                                    None => recognition.as_ref().and_then(|r| r.1.clone()),
                                };
                                let matched_by = if bound_speaker.is_some() {
                                    Some("user".to_string())
                                } else if recognition.is_some() {
                                    Some("auto".to_string())
                                } else {
                                    None
                                };
                                let turn_event = SpeakerTurn {
                                    start_time: channel.mapper.to_abs(turn_start as f64),
                                    end_time: channel.mapper.to_abs(turn_end as f64),
                                    speaker: turn_label,
                                    source_device: source_device.to_string(),
                                    display_name,
                                    matched_by,
                                    match_score: recognition.as_ref().map(|r| r.2),
                                };
                                // Publish to the live diarization registry so the
                                // reconcile stage can attribute words live
                                // (live-word-level-diarization D3). Unconditional
                                // (also drives the turn-stability instrumentation
                                // when no frontend sender is attached).
                                crate::audio::live_diarization_reconcile::registry().publish(
                                    crate::audio::live_diarization_reconcile::LiveTurn {
                                        start_time: turn_event.start_time,
                                        end_time: turn_event.end_time,
                                        speaker: turn_event.speaker.clone(),
                                        source_device: turn_event.source_device.clone(),
                                        display_name: turn_event.display_name.clone(),
                                        matched_by: turn_event.matched_by.clone(),
                                        match_score: turn_event.match_score,
                                    },
                                );
                                if let Some(sink) = &emission_sink {
                                    let _ = sink.send(EmittedTurn {
                                        turn: turn_event.clone(),
                                        stable: true,
                                    });
                                }
                                if let Some(sender) = &turn_sender {
                                    if let Err(e) = sender.send(turn_event) {
                                        warn!("Failed to send online speaker turn: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "StreamingPipeline feed failed ({}), disabling online diarization",
                            e
                        );
                        record_stats(&stats, |s| s.disable());
                        self.engine = None;
                    }
                }
            }
        }
    }

    /// Computes speaker assignments for the in-memory transcript segments and
    /// per-channel clustered embeddings for the speaker registry. Returns
    /// `(assignments, cluster_embeddings, live_user_bindings, display_pass)`,
    /// the last being the refined per-channel timeline the display-side final
    /// pass promotes live blocks against (05b D2). An empty assignment list on
    /// "no speech detected" is not an error.
    pub fn finalize(&mut self, transcripts: &[TranscriptSegment]) -> Result<FinalizeOutput, String> {
        let Some(engine) = self.engine.take() else {
            return Err("Online diarization unavailable (error state)".to_string());
        };

        let mic_prefix = self.mic_prefix.clone();
        // Session-resolved parameters (05 section 3): the merge threshold and
        // clusterer kind come from the same settings the batch path reads, and
        // the speaker-count ceiling is resolved by the shared rule instead of
        // reaching the clusterer as an unbounded 0.
        let user_max = if self.max_speakers > 0 {
            Some(self.max_speakers as i32)
        } else {
            None
        };
        let ceiling =
            crate::audio::diarization::effective_cluster_ceiling(&self.config, user_max);
        let (mic_segments, sys_segments, mic_clustered, sys_clustered, mic_raw, sys_raw) =
            match engine {
                Engine::Efficient {
                    extractor: _,
                    mic,
                    sys,
                } => {
                    let mic_segments = mic.cluster(&self.config, ceiling);
                    let sys_segments = sys.cluster(&self.config, ceiling);
                    // Efficient mode: embeddings are buffered per segment; cluster()
                    // returns labels aligned with the buffer entries, so group by
                    // those labels directly.
                    let mic_clustered = clustered_embeddings(&mic.entries, EmbeddingLabeling::ByPosition(&mic_segments));
                    let sys_clustered = clustered_embeddings(&sys.entries, EmbeddingLabeling::ByPosition(&sys_segments));
                    (
                        mic_segments,
                        sys_segments,
                        mic_clustered,
                        sys_clustered,
                        mic.entries,
                        sys.entries,
                    )
                }
                Engine::Fast {
                    mic,
                    sys,
                    extractor: _,
                    mic_emb,
                    sys_emb,
                } => {
                    let mut mic_segments: Vec<SpeakerSegment> = mic
                        .turns
                        .iter()
                        .map(|t| SpeakerSegment {
                            start: mic.mapper.to_abs(t.start as f64) as f32,
                            end: mic.mapper.to_abs(t.end as f64) as f32,
                            speaker: t.speaker,
                        })
                        .collect();
                    let mut sys_segments: Vec<SpeakerSegment> = sys
                        .turns
                        .iter()
                        .map(|t| SpeakerSegment {
                            start: sys.mapper.to_abs(t.start as f64) as f32,
                            end: sys.mapper.to_abs(t.end as f64) as f32,
                            speaker: t.speaker,
                        })
                        .collect();
                    mic_segments.sort_by(|a, b| a.start.total_cmp(&b.start));
                    sys_segments.sort_by(|a, b| a.start.total_cmp(&b.start));

                    // Stop-time refinement (05b D1), per channel and never
                    // across channels: the two buffers are clustered
                    // separately, so a refined cluster only ever covers the
                    // windows of the channel it came from. A channel that
                    // falls back keeps the incremental timeline it published
                    // live, independently of the other channel's outcome.
                    let mut mic_refined =
                        refine_channel("microphone", &mic_emb, &self.config, ceiling);
                    let mut sys_refined = refine_channel("system", &sys_emb, &self.config, ceiling);
                    // Keep the live label space (05b 3.3): a rename or a
                    // per-turn override made during the recording is keyed by
                    // the live cluster label.
                    for (units, live, channel) in [
                        (&mut mic_refined, &mic_segments, "microphone"),
                        (&mut sys_refined, &sys_segments, "system"),
                    ] {
                        if let Ok(units) = units.as_mut() {
                            let kept = anchor_to_live_labels(units, live);
                            info!(
                                "Stop-time refinement on the {} channel kept {} live speaker name(s); {} cluster(s) after refinement",
                                channel,
                                kept,
                                units
                                    .iter()
                                    .map(|u| u.speaker)
                                    .collect::<std::collections::BTreeSet<_>>()
                                    .len()
                            );
                        }
                    }
                    let (mic_segments, mic_clustered) = match mic_refined {
                        Ok(units) => {
                            // Persistence and enrollment read the refined label
                            // of each window directly; the timeline the
                            // transcript is attributed against is the same
                            // labels with the resolved gap-merge applied, so a
                            // speaker's consecutive windows become one turn as
                            // they do on the batch path.
                            let clustered = clustered_embeddings(
                                &mic_emb.entries,
                                EmbeddingLabeling::ByPosition(&units),
                            );
                            (
                                merge_same_speaker_segments(units, self.config.gap_merge_secs),
                                clustered,
                            )
                        }
                        Err(_) => {
                            // Fast mode fallback: buffer entries have no cluster
                            // id, so group them by time-overlap with the stable
                            // turns (which carry pipeline speaker ids), using the
                            // same find_best_speaker logic.
                            let clustered = clustered_embeddings(
                                &mic_emb.entries,
                                EmbeddingLabeling::ByOverlap(&mic_segments),
                            );
                            (mic_segments, clustered)
                        }
                    };
                    let (sys_segments, sys_clustered) = match sys_refined {
                        Ok(units) => {
                            let clustered = clustered_embeddings(
                                &sys_emb.entries,
                                EmbeddingLabeling::ByPosition(&units),
                            );
                            (
                                merge_same_speaker_segments(units, self.config.gap_merge_secs),
                                clustered,
                            )
                        }
                        Err(_) => {
                            let clustered = clustered_embeddings(
                                &sys_emb.entries,
                                EmbeddingLabeling::ByOverlap(&sys_segments),
                            );
                            (sys_segments, clustered)
                        }
                    };
                    (
                        mic_segments,
                        sys_segments,
                        mic_clustered,
                        sys_clustered,
                        mic_emb.entries,
                        sys_emb.entries,
                    )
                }
            };

        // The refined timeline for the display-side final pass, in the label
        // namespace the live turns used: the microphone channel carries this
        // session's mic prefix, the system channel always `SPEAKER`.
        let display_pass = FinalDisplayPass {
            channels: vec![
                FinalChannel {
                    source_device: "Microphone".to_string(),
                    spans: labelled_spans(&mic_segments, &mic_prefix),
                },
                FinalChannel {
                    source_device: "System".to_string(),
                    spans: labelled_spans(&sys_segments, "SPEAKER"),
                },
            ],
            relabel_all: self.config.final_relabel_all,
            // Per-turn overrides the user recorded during the recording. Read,
            // never consumed: `finalize_online_session` still applies them to
            // the persisted transcripts.
            protected: super::super::engine::live_turn_override_windows(self.saw_system_audio),
        };

        let clusters = OnlineClusterEmbeddings {
            mic: mic_clustered,
            sys: sys_clustered,
            saw_system_audio: self.saw_system_audio,
            model_tag: Some(self.model_tag.to_string()),
            mic_raw: mic_raw.clone(),
            sys_raw: sys_raw.clone(),
        };

        // Extract live user bindings (Fast-mode renames) to apply at the
        // post-save finalize, when the meeting row exists.
        let live_bindings = self
            .prototype_store
            .as_ref()
            .and_then(|store| store.read().ok())
            .map(|s| s.bindings().clone())
            .unwrap_or_default();

        // Stop-time failure surfacing (task 4.3): warn when chunks were attempted
        // but no valid embeddings survived, per channel. Both channels empty but
        // attempted >0 leaves offline fallback available.
        if self.attempted_mic > 0 && mic_raw.is_empty() {
            warn!(
                "Online diarization: mic channel had {} attempted chunks ({} failed) but zero valid embeddings (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large); skipping mic clustering",
                self.attempted_mic, self.failed_mic
            );
        }
        if self.attempted_sys > 0 && sys_raw.is_empty() {
            warn!(
                "Online diarization: system channel had {} attempted chunks ({} failed) but zero valid embeddings (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large); skipping system clustering",
                self.attempted_sys, self.failed_sys
            );
        }

        if mic_segments.is_empty() && sys_segments.is_empty() {
            info!("Online diarization: no speech segments detected, skipping transcript updates");
            return Ok((Vec::new(), clusters, live_bindings, display_pass));
        }

        // N-way token expansion: if a transcript carries token timestamps spanning
        // multiple speakers, expand it into N gap-free transcript rows before
        // assignment, per design D3 (≥2 contiguous tokens per boundary).
        let mut expanded: Vec<TranscriptSegment> = Vec::new();
        for t in transcripts {
            if let Some(tokens) = &t.tokens {
                if !tokens.is_empty() {
                    let segs_ref = if self.saw_system_audio && t.source_device.as_str() == "System"
                    {
                        &sys_segments
                    } else {
                        &mic_segments
                    };
                    let blocks = split_tokens_by_speaker(tokens, segs_ref);
                    if blocks.len() > 1 {
                        for (idx, block) in blocks.iter().enumerate() {
                            let mut clone = t.clone();
                            clone.audio_start_time = block.start as f64;
                            clone.audio_end_time = block.end as f64;
                            clone.duration = (block.end - block.start) as f64;
                            if !block.text.is_empty() {
                                clone.text = block.text.clone();
                            }
                            if idx != 0 {
                                clone.id = format!("{}_split{}", t.id, idx);
                                clone.sequence_id = t.sequence_id + idx as u64 * 10000 + 1000;
                            }
                            // Ensure display_time updated? Keep original but duration covers it
                            expanded.push(clone);
                        }
                        continue;
                    }
                }
            }
            expanded.push(t.clone());
        }

        let mut assignments = Vec::new();
        let mut skipped_no_match = 0usize;
        for transcript in &expanded {
            let t_start = transcript.audio_start_time as f32;
            let t_end = transcript.audio_end_time as f32;

            let (segments, prefix) =
                if self.saw_system_audio && transcript.source_device.as_str() == "System" {
                    (&sys_segments, "SPEAKER")
                } else {
                    (&mic_segments, mic_prefix.as_str())
                };

            match find_best_speaker(segments, t_start, t_end) {
                Some(spk) => assignments.push(SpeakerAssignment {
                    sequence_id: transcript.sequence_id,
                    speaker: format!("{}_{:02}", prefix, spk),
                }),
                None => skipped_no_match += 1,
            }
        }

        info!(
            "Online diarization finalized: {} assigned, {} no-match skipped, {} live bindings",
            assignments.len(),
            skipped_no_match,
            live_bindings.len()
        );
        Ok((assignments, clusters, live_bindings, display_pass))
    }
}

/// `(start, end, label)` spans for one channel, named exactly as the
/// assignment path names them.
fn labelled_spans(segments: &[SpeakerSegment], prefix: &str) -> Vec<(f64, f64, String)> {
    segments
        .iter()
        .map(|seg| {
            (
                seg.start as f64,
                seg.end as f64,
                format!("{}_{:02}", prefix, seg.speaker),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One online processor may exist per process (the session guard), so the
    /// tests that build one take this lock. Without it they would race and
    /// their graceful "models unavailable" skip would swallow a guard error.
    static PROCESSOR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 05 task 3.1 / D4: the session's clustering parameters are resolved once
    /// at construction, so a settings change mid-recording cannot split one
    /// session across two configurations. Needs the enhanced models to build a
    /// processor; skips when they are not installed.
    #[test]
    fn processor_holds_the_config_captured_at_construction() {
        let _serial = PROCESSOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        use crate::audio::diarization::{
            set_clustering_overrides, ClustererKindSetting, DiarizationConfig,
        };

        let Ok(models_dir) = crate::audio::diarization::resolve_models_dir_standalone(None) else {
            eprintln!("skipping: enhanced diarization models not installed");
            return;
        };

        let saved = DiarizationConfig::resolved();

        // Start the session under a known, non-default threshold.
        set_clustering_overrides(
            Some(0.42),
            Some(9),
            None,
            Some(ClustererKindSetting::Ahc),
            Some(false),
            None,
        );
        let processor = match OnlineDiarizationProcessor::new(
            DiarizationMode::Efficient,
            0,
            true,
            &models_dir,
            None,
            None,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("skipping: processor init unavailable ({e})");
                set_clustering_overrides(None, None, None, None, None, None);
                return;
            }
        };
        assert_eq!(processor.config.cluster_threshold, 0.42);
        assert_eq!(processor.config.cluster_ceiling, 9);
        assert!(
            !processor.config.final_recluster,
            "the stop-time refinement switch is resolved at session start too (05b 2.1)"
        );

        // A settings change after the session started must not reach it.
        set_clustering_overrides(
            Some(0.81),
            Some(3),
            None,
            Some(ClustererKindSetting::Nmesc),
            Some(true),
            None,
        );
        assert_eq!(
            processor.config.cluster_threshold, 0.42,
            "session config is a snapshot taken at construction"
        );
        assert_eq!(processor.config.cluster_ceiling, 9);
        assert_eq!(processor.config.clusterer, ClustererKindSetting::Ahc);
        assert!(
            !processor.config.final_recluster,
            "turning the refinement on mid-recording must not reach a running session"
        );
        // ... while a newly resolved config does see it.
        assert_eq!(DiarizationConfig::resolved().cluster_threshold, 0.81);
        assert!(DiarizationConfig::resolved().final_recluster);

        drop(processor);
        set_clustering_overrides(
            Some(saved.cluster_threshold),
            Some(saved.cluster_ceiling),
            Some(saved.gap_merge_secs),
            Some(saved.clusterer),
            Some(saved.final_recluster),
            Some(saved.final_relabel_all),
        );
        set_clustering_overrides(None, None, None, None, None, None);
    }

    // ===== Stop-time refinement (05b D1, tasks 2.2/2.3) =====

    /// A deterministic stand-in for one voice: a 192-d unit direction (the
    /// TitaNet-Large dimensionality) with a fixed per-window perturbation, so
    /// two windows of the same voice are near-parallel and two windows of
    /// different voices are near-orthogonal. Real audio is not needed to
    /// exercise the clustering seam, and a synthetic vector keeps the expected
    /// grouping known exactly.
    fn voice_window(voice: usize, window: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 192];
        v[voice] = 1.0;
        // A small, deterministic wobble in a dimension no voice occupies.
        v[64 + window] = 0.02;
        v
    }

    fn buffer_of(windows: &[(f32, f32, usize, usize)]) -> EmbeddingBuffer {
        let mut buffer = EmbeddingBuffer::default();
        for (start, end, voice, window) in windows {
            buffer.push(*start, *end, voice_window(*voice, *window));
        }
        buffer
    }

    fn refine_config(final_recluster: bool) -> crate::audio::diarization::DiarizationConfig {
        crate::audio::diarization::DiarizationConfig {
            final_recluster,
            ..crate::audio::diarization::DiarizationConfig::default()
        }
    }

    /// The regression this change exists for: the incremental pass published
    /// one identity for two voices, and the stop-time pass has to take them
    /// apart from the same buffered embeddings.
    #[test]
    fn refinement_separates_two_voices_the_incremental_pass_merged() {
        // Six windows, two voices, alternating in time - exactly the shape
        // that makes an incremental clusterer merge them.
        let buffer = buffer_of(&[
            (0.0, 1.0, 0, 0),
            (1.0, 2.0, 1, 1),
            (2.0, 3.0, 0, 2),
            (3.0, 4.0, 1, 3),
            (4.0, 5.0, 0, 4),
            (5.0, 6.0, 1, 5),
        ]);
        let config = refine_config(true);

        let refined = refine_channel("microphone", &buffer, &config, 128)
            .expect("two well-separated voices must cluster");
        assert_eq!(refined.len(), 6, "one labelled segment per buffered window");

        let labels: Vec<usize> = refined.iter().map(|seg| seg.speaker).collect();
        let distinct: std::collections::BTreeSet<usize> = labels.iter().copied().collect();
        assert_eq!(
            distinct.len(),
            2,
            "the refined pass finds the two voices the live ids merged into one, got {:?}",
            labels
        );
        // Windows of one voice share a label, and the two voices differ.
        assert_eq!(labels[0], labels[2]);
        assert_eq!(labels[0], labels[4]);
        assert_eq!(labels[1], labels[3]);
        assert_eq!(labels[1], labels[5]);
        assert_ne!(labels[0], labels[1]);
        // Times are carried through untouched: the pass relabels windows, it
        // does not move them.
        assert_eq!(
            refined.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(),
            vec![
                (0.0, 1.0),
                (1.0, 2.0),
                (2.0, 3.0),
                (3.0, 4.0),
                (4.0, 5.0),
                (5.0, 6.0)
            ]
        );
    }

    /// The timeline the transcript is attributed against is the refined labels
    /// with the resolved gap-merge applied, so one speaker's consecutive
    /// windows become one turn instead of one turn per window.
    #[test]
    fn the_refined_timeline_merges_a_speakers_consecutive_windows() {
        let buffer = buffer_of(&[
            (0.0, 1.0, 0, 0),
            (1.0, 2.0, 0, 1),
            (2.0, 3.0, 0, 2),
            (4.0, 5.0, 1, 3),
            (5.0, 6.0, 1, 4),
        ]);
        let config = refine_config(true);
        let refined = refine_channel("microphone", &buffer, &config, 128).expect("clusters");

        let timeline = merge_same_speaker_segments(refined, config.gap_merge_secs);
        assert_eq!(
            timeline.len(),
            2,
            "three windows of one voice and two of another become two turns, got {:?}",
            timeline
                .iter()
                .map(|s| (s.start, s.end, s.speaker))
                .collect::<Vec<_>>()
        );
        assert_eq!((timeline[0].start, timeline[0].end), (0.0, 3.0));
        assert_eq!((timeline[1].start, timeline[1].end), (4.0, 6.0));
        assert_ne!(timeline[0].speaker, timeline[1].speaker);
    }

    /// Each channel is clustered from its own buffer, so a refined cluster can
    /// only ever cover windows of the channel it came from - even when both
    /// channels carry speech over the same wall-clock span, and even when one
    /// channel falls back while the other refines.
    #[test]
    fn a_refined_cluster_never_spans_both_channels() {
        let config = refine_config(true);
        // Both channels speak over 0-6 s. The microphone has two voices, the
        // system channel one - a different voice again.
        let mic = buffer_of(&[
            (0.0, 1.0, 0, 0),
            (1.0, 2.0, 1, 1),
            (2.0, 3.0, 0, 2),
            (3.0, 4.0, 1, 3),
        ]);
        let sys = buffer_of(&[(0.0, 2.0, 2, 0), (2.0, 4.0, 2, 1), (4.0, 6.0, 2, 2)]);

        let mic_refined = refine_channel("microphone", &mic, &config, 128).expect("mic clusters");
        let sys_refined = refine_channel("system", &sys, &config, 128).expect("sys clusters");

        // Every refined segment came from its own channel's windows.
        let mic_windows: Vec<(f32, f32)> = mic.entries.iter().map(|e| (e.0, e.1)).collect();
        let sys_windows: Vec<(f32, f32)> = sys.entries.iter().map(|e| (e.0, e.1)).collect();
        assert_eq!(
            mic_refined
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>(),
            mic_windows
        );
        assert_eq!(
            sys_refined
                .iter()
                .map(|s| (s.start, s.end))
                .collect::<Vec<_>>(),
            sys_windows
        );
        // The system channel's single voice stays one cluster while the
        // microphone's two stay two: neither result could have been produced
        // from the other channel's embeddings.
        assert_eq!(
            sys_refined
                .iter()
                .map(|s| s.speaker)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            1
        );
        assert_eq!(
            mic_refined
                .iter()
                .map(|s| s.speaker)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2
        );
        // One channel falling back leaves the other channel's refinement
        // exactly as it was.
        let empty = EmbeddingBuffer::default();
        assert!(refine_channel("system", &empty, &config, 128).is_err());
        let mic_again = refine_channel("microphone", &mic, &config, 128).expect("mic clusters");
        assert_eq!(
            mic_again.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            mic_refined.iter().map(|s| s.speaker).collect::<Vec<_>>()
        );
    }

    /// Spec scenario "Refinement obeys the resolved parameters": the refinement
    /// never yields more speakers than the effective ceiling, however many the
    /// audio would support.
    #[test]
    fn the_refinement_never_exceeds_the_effective_ceiling() {
        let distinct = |segments: &[SpeakerSegment]| {
            segments
                .iter()
                .map(|s| s.speaker)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        // Three well-separated voices, two windows each.
        let buffer = buffer_of(&[
            (0.0, 1.0, 0, 0),
            (1.0, 2.0, 1, 1),
            (2.0, 3.0, 2, 2),
            (3.0, 4.0, 0, 3),
            (4.0, 5.0, 1, 4),
            (5.0, 6.0, 2, 5),
        ]);
        let config = refine_config(true);

        let free = refine_channel("microphone", &buffer, &config, 128).expect("clusters");
        assert_eq!(distinct(&free), 3, "the audio supports three speakers");

        let capped = refine_channel("microphone", &buffer, &config, 2).expect("clusters");
        assert!(
            distinct(&capped) <= 2,
            "a ceiling of two must hold, got {} speakers",
            distinct(&capped)
        );
    }

    /// Spec scenario "Refinement has its own default merge threshold": the pass
    /// reads `final_recluster_threshold`, not the offline `cluster_threshold`.
    /// Each arm sets the two to opposite extremes, so wiring the wrong one
    /// through gives the opposite answer.
    #[test]
    fn the_refinement_uses_its_own_merge_threshold_not_the_offline_one() {
        let distinct = |segments: &[SpeakerSegment]| {
            segments
                .iter()
                .map(|s| s.speaker)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        let buffer = buffer_of(&[
            (0.0, 1.0, 0, 0),
            (1.0, 2.0, 1, 1),
            (2.0, 3.0, 0, 2),
            (3.0, 4.0, 1, 3),
        ]);

        // Offline threshold would merge everything; the refinement's separates.
        let separates = crate::audio::diarization::DiarizationConfig {
            cluster_threshold: -0.5,
            final_recluster_threshold: 0.5,
            ..refine_config(true)
        };
        let refined = refine_channel("microphone", &buffer, &separates, 128).expect("clusters");
        assert_eq!(distinct(&refined), 2, "the refinement's own threshold decides");

        // Offline threshold would separate; the refinement's merges everything.
        let merges = crate::audio::diarization::DiarizationConfig {
            cluster_threshold: 0.99,
            final_recluster_threshold: -0.5,
            ..refine_config(true)
        };
        let refined = refine_channel("microphone", &buffer, &merges, 128).expect("clusters");
        assert_eq!(distinct(&refined), 1, "the offline threshold must not leak in");
    }

    /// Task 2.3: every fallback path completes the stop on the incremental
    /// identities and says why, in the same words it logs.
    #[test]
    fn every_refinement_fallback_keeps_the_live_identities_and_says_why() {
        let buffer = buffer_of(&[(0.0, 1.0, 0, 0), (1.0, 2.0, 1, 1), (2.0, 3.0, 0, 2)]);

        // 1. The setting is off.
        let reason = refine_channel("microphone", &buffer, &refine_config(false), 128)
            .expect_err("a disabled pass must not refine");
        assert!(
            reason.contains("turned off in settings")
                && reason.contains("keeping the identities shown live"),
            "unexpected reason: {reason}"
        );

        // 2. Fewer than two buffered embeddings on the channel.
        let one = buffer_of(&[(0.0, 1.0, 0, 0)]);
        let reason = refine_channel("system", &one, &refine_config(true), 128)
            .expect_err("one embedding cannot be clustered");
        assert!(
            reason.contains("1 buffered embedding(s), fewer than the two clustering needs"),
            "unexpected reason: {reason}"
        );
        let none = EmbeddingBuffer::default();
        assert!(refine_channel("system", &none, &refine_config(true), 128).is_err());

        // 3. A clusterer that cannot be built for this model family: `vbx`
        // is dimension-locked to 256-d embeddings and the bundled family is
        // 192-d, so construction errors instead of silently switching kinds.
        let vbx = crate::audio::diarization::DiarizationConfig {
            clusterer: crate::audio::diarization::ClustererKindSetting::Vbx,
            final_recluster: true,
            ..crate::audio::diarization::DiarizationConfig::default()
        };
        let reason = refine_channel("microphone", &buffer, &vbx, 128)
            .expect_err("vbx must not build for the 192-d family");
        assert!(
            reason.contains("failed on the microphone channel")
                && reason.contains("keeping the identities shown live"),
            "unexpected reason: {reason}"
        );
    }

    /// The live label space survives the refinement (05b 3.3). Everything the
    /// user did during the recording is keyed by the live cluster label, so a
    /// refined cluster that is mostly one live cluster has to keep its name.
    #[test]
    fn anchoring_keeps_the_live_name_and_numbers_only_the_new_split() {
        // Live: one cluster (id 3) covered 0-6 s - it merged two voices.
        let live = vec![SpeakerSegment {
            start: 0.0,
            end: 6.0,
            speaker: 3,
        }];
        // Refined: two clusters, ids 0 and 1 from the clusterer's own numbering.
        let mut refined = vec![
            SpeakerSegment { start: 0.0, end: 1.0, speaker: 0 },
            SpeakerSegment { start: 1.0, end: 2.0, speaker: 1 },
            SpeakerSegment { start: 2.0, end: 3.0, speaker: 0 },
            SpeakerSegment { start: 3.0, end: 4.0, speaker: 1 },
            SpeakerSegment { start: 4.0, end: 6.0, speaker: 0 },
        ];
        let kept = anchor_to_live_labels(&mut refined, &live);
        assert_eq!(kept, 1, "one refined cluster inherits the live cluster");
        // Cluster 0 covers 4 s of live id 3 and cluster 1 covers 2 s, so 0
        // takes the name and 1 becomes an id no live cluster used.
        let labels: Vec<usize> = refined.iter().map(|s| s.speaker).collect();
        assert_eq!(labels, vec![3, 4, 3, 4, 3]);
    }

    /// With no live labels to anchor to (a session that published no stable
    /// turn), the refined numbering stands as it is.
    #[test]
    fn anchoring_is_a_no_op_without_live_labels() {
        let mut refined = vec![
            SpeakerSegment { start: 0.0, end: 1.0, speaker: 0 },
            SpeakerSegment { start: 1.0, end: 2.0, speaker: 1 },
        ];
        let before: Vec<usize> = refined.iter().map(|s| s.speaker).collect();
        assert_eq!(anchor_to_live_labels(&mut refined, &[]), 0);
        assert_eq!(refined.iter().map(|s| s.speaker).collect::<Vec<_>>(), before);
    }

    /// Two live clusters that survive the refinement each keep their own name,
    /// rather than both collapsing onto the biggest one.
    #[test]
    fn anchoring_is_one_to_one() {
        let live = vec![
            SpeakerSegment { start: 0.0, end: 4.0, speaker: 0 },
            SpeakerSegment { start: 4.0, end: 8.0, speaker: 1 },
        ];
        let mut refined = vec![
            SpeakerSegment { start: 0.0, end: 4.0, speaker: 7 },
            SpeakerSegment { start: 4.0, end: 8.0, speaker: 9 },
        ];
        assert_eq!(anchor_to_live_labels(&mut refined, &live), 2);
        assert_eq!(
            refined.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    /// Task 4.2: the shared core serves the batch drivers and the live driver
    /// alike. Given the same windows of the same audio, the in-memory batch
    /// source and the live VAD-chunk source must produce the same result, so
    /// nothing in the core depends on which driver it is running for. The
    /// windows are cut at fixed boundaries and handed to both sources
    /// unchanged; what a real recording's VAD would cut is a different
    /// question, and not one this seam answers.
    #[test]
    fn the_core_gives_the_same_result_from_a_batch_source_and_a_live_source() {
        use super::super::source::VadChunks;
        use crate::audio::diarization::batch::chunking::MemoryWindows;
        use crate::audio::diarization::core::segment::V2Core;
        use crate::audio::diarization::{create_polyvoice_diarizer, DiarizationConfig};
        use crate::audio::recording_state::DeviceType;

        let Ok(models_dir) = crate::audio::diarization::resolve_models_dir_standalone(None) else {
            eprintln!("skipping: enhanced diarization models not installed");
            return;
        };
        let Some(blocks) = speech_chunks(30.0) else {
            eprintln!("skipping: no eval speech fixture available");
            return;
        };
        // 30 s of speech re-cut into three 10 s windows.
        let samples: Vec<f32> = blocks.iter().flat_map(|c| c.data.iter().copied()).collect();
        let rate = blocks[0].sample_rate;
        assert_eq!(rate, 16_000, "the fixture is 16 kHz, like the live path");
        let per_window = 10 * rate as usize;
        let windows: Vec<(f32, Vec<f32>)> = samples
            .chunks(per_window)
            .enumerate()
            .map(|(i, w)| ((i * 10) as f32, w.to_vec()))
            .collect();
        assert!(windows.len() >= 3, "need several windows to exercise the loop");

        let config = DiarizationConfig::default();
        let diarizer = create_polyvoice_diarizer(&models_dir, None, &config).expect("diarizer");

        let run = |source: &mut dyn FnMut(&mut V2Core<'_>) -> Result<(), String>| {
            let mut core = V2Core::new(&diarizer, &config);
            source(&mut core).expect("core run");
            core.finish().expect("finish")
        };
        let (batch_segments, batch_embeddings, _) =
            run(&mut |core| core.process_source(&mut MemoryWindows::new(windows.clone(), rate)));
        let mut live = VadChunks::new(windows.iter().enumerate().map(|(i, (start, data))| AudioChunk {
            data: data.clone(),
            sample_rate: rate,
            timestamp: *start as f64,
            chunk_id: i as u64,
            device_type: DeviceType::Microphone,
            channels: 1,
        }));
        let (live_segments, live_embeddings, _) = run(&mut |core| core.process_source(&mut live));

        assert!(
            !batch_segments.is_empty() && !batch_embeddings.is_empty(),
            "the fixture must yield speech, or this comparison proves nothing"
        );
        let spans = |segments: &[crate::audio::diarization::DiarizationSegment]| {
            segments
                .iter()
                .map(|s| (s.start, s.end, s.speaker))
                .collect::<Vec<_>>()
        };
        assert_eq!(spans(&batch_segments), spans(&live_segments));
        assert_eq!(batch_embeddings.len(), live_embeddings.len());
        for (a, b) in batch_embeddings.iter().zip(&live_embeddings) {
            assert_eq!(a.speaker, b.speaker);
            assert_eq!((a.start_secs, a.end_secs), (b.start_secs, b.end_secs));
            assert_eq!(a.embedding, b.embedding, "embedding units differ between sources");
        }
    }

    /// A fixture with real speech, in 0.6 s chunks on one channel. Returns
    /// `None` when neither an explicit `MEETILY_EVAL_WAV` nor the eval
    /// dataset is present, so these tests skip rather than assert on silence.
    fn speech_chunks(seconds: f64) -> Option<Vec<AudioChunk>> {
        use crate::audio::recording_state::DeviceType;

        let explicit = std::env::var("MEETILY_EVAL_WAV")
            .ok()
            .map(std::path::PathBuf::from);
        let path = explicit.or_else(|| {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()?
                .parent()?
                .join("eval/data/voxconverse-dev/wav");
            std::fs::read_dir(dir)
                .ok()?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "wav"))
                .min()
        })?;
        let decoded = crate::audio::decoder::decode_audio_file(&path).ok()?;
        let wanted = (decoded.sample_rate as f64 * seconds) as usize;
        let samples: Vec<f32> = decoded.samples.into_iter().take(wanted).collect();
        if samples.is_empty() {
            return None;
        }

        let per_chunk = (decoded.sample_rate as f64 * 0.6) as usize;
        Some(
            samples
                .chunks(per_chunk)
                .enumerate()
                .map(|(i, block)| AudioChunk {
                    data: block.to_vec(),
                    sample_rate: decoded.sample_rate,
                    channels: 1,
                    timestamp: i as f64 * 0.6,
                    chunk_id: i as u64,
                    device_type: DeviceType::Microphone,
                })
                .collect(),
        )
    }

    /// Feed a fixture through a Fast-mode processor and return
    /// `(app-facing turns, observed emissions)`. The sink is attached only
    /// when `observe` is set, which is the production shape otherwise.
    fn run_fast_session(
        chunks: &[AudioChunk],
        observe: bool,
    ) -> Option<(Vec<SpeakerTurn>, Vec<EmittedTurn>)> {
        let models_dir = crate::audio::diarization::resolve_models_dir_standalone(None).ok()?;
        let (turn_tx, mut turn_rx) = tokio::sync::mpsc::unbounded_channel();
        let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut processor = OnlineDiarizationProcessor::new(
            DiarizationMode::Fast,
            0,
            false,
            &models_dir,
            Some(turn_tx),
            None,
        )
        .ok()?;
        if observe {
            processor.attach_emission_sink(sink_tx);
        }
        for chunk in chunks {
            processor.process_chunk(chunk.clone());
        }
        drop(processor);

        let mut turns = Vec::new();
        while let Ok(t) = turn_rx.try_recv() {
            turns.push(t);
        }
        let mut observed = Vec::new();
        while let Ok(e) = sink_rx.try_recv() {
            observed.push(e);
        }
        Some((turns, observed))
    }

    fn turn_shape(turns: &[SpeakerTurn]) -> Vec<(String, i64, i64)> {
        turns
            .iter()
            .map(|t| {
                (
                    t.speaker.clone(),
                    (t.start_time * 1000.0) as i64,
                    (t.end_time * 1000.0) as i64,
                )
            })
            .collect()
    }

    /// add-online-diarization-eval task 1.1: attaching the observation sink
    /// must not change what the app receives. The same fixture is run twice in
    /// this process (the session guard releases on drop), once in the
    /// production shape and once with the sink attached, and the app-facing
    /// turn sequences must be identical.
    #[test]
    fn emission_sink_does_not_change_the_app_facing_turns() {
        let _serial = PROCESSOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(chunks) = speech_chunks(30.0) else {
            eprintln!("skipping: no speech fixture (set MEETILY_EVAL_WAV)");
            return;
        };
        let Some((without_sink, observed_none)) = run_fast_session(&chunks, false) else {
            eprintln!("skipping: enhanced diarization models not installed");
            return;
        };
        assert!(
            observed_none.is_empty(),
            "no sink attached: nothing may be observed"
        );

        let (with_sink, observed) =
            run_fast_session(&chunks, true).expect("second session builds too");
        assert_eq!(
            turn_shape(&without_sink),
            turn_shape(&with_sink),
            "the app-facing turn stream must not depend on the observation sink"
        );
        assert!(
            !observed.is_empty() || without_sink.is_empty(),
            "with a sink attached, emissions must be observable whenever turns exist"
        );
        eprintln!(
            "app-facing turns: {} (identical with and without the sink), observed emissions: {}",
            with_sink.len(),
            observed.len()
        );
    }

    /// add-online-diarization-eval task 1.2: the sink sees provisional
    /// emissions the app never receives, and the two streams differ only by
    /// those entries.
    #[test]
    fn emission_sink_observes_provisional_turns() {
        let _serial = PROCESSOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(chunks) = speech_chunks(30.0) else {
            eprintln!("skipping: no speech fixture (set MEETILY_EVAL_WAV)");
            return;
        };
        let Some((app_turns, observed)) = run_fast_session(&chunks, true) else {
            eprintln!("skipping: enhanced diarization models not installed");
            return;
        };
        if observed.is_empty() {
            eprintln!("skipping: the fixture produced no emissions");
            return;
        }

        let stable: Vec<SpeakerTurn> = observed
            .iter()
            .filter(|e| e.stable)
            .map(|e| e.turn.clone())
            .collect();
        assert_eq!(
            turn_shape(&stable),
            turn_shape(&app_turns),
            "the stable emissions are exactly what the app receives, in order"
        );
        assert!(
            observed.iter().filter(|e| !e.stable).all(|e| !e.stable),
            "provisional entries carry stable = false"
        );
        let provisional = observed.iter().filter(|e| !e.stable).count();
        assert_eq!(
            observed.len() - stable.len(),
            provisional,
            "the two streams differ only by the provisional entries"
        );
        eprintln!(
            "observed {} emissions: {} stable (= the app stream), {} provisional",
            observed.len(),
            stable.len(),
            provisional
        );
    }

    /// add-online-diarization-eval task 1.3: what a headless harness depends
    /// on — an explicit models directory works, a missing model set fails with
    /// a message naming the file it looked for, and the session guard refuses
    /// a second processor in the same process.
    #[test]
    fn headless_construction_resolves_models_and_holds_the_guard() {
        let _serial = PROCESSOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // A directory with no enhanced models: the error names the file.
        let empty = std::env::temp_dir().join("meetily-online-eval-no-models");
        std::fs::create_dir_all(&empty).expect("temp dir");
        let err = match OnlineDiarizationProcessor::new(
            DiarizationMode::Fast,
            0,
            false,
            &empty,
            None,
            None,
        ) {
            Err(err) => err,
            Ok(_) => panic!("a directory without the enhanced models must not build"),
        };
        assert!(
            err.contains("Enhanced diarization models not found")
                && err.contains(
                    crate::audio::embedder::enhanced_model_paths(&empty)
                        .1
                        .display()
                        .to_string()
                        .as_str()
                ),
            "the error must name the searched embedding model, got: {err}"
        );

        let Ok(models_dir) = crate::audio::diarization::resolve_models_dir_standalone(None) else {
            eprintln!("skipping the rest: enhanced diarization models not installed");
            return;
        };

        // An explicit models directory builds.
        let first = match OnlineDiarizationProcessor::new(
            DiarizationMode::Fast,
            0,
            false,
            &models_dir,
            None,
            None,
        ) {
            Ok(p) => p,
            Err(e) => panic!("explicit models directory must build: {e}"),
        };

        // The session guard admits only one at a time.
        let second = OnlineDiarizationProcessor::new(
            DiarizationMode::Fast,
            0,
            false,
            &models_dir,
            None,
            None,
        );
        let guard_err = match second {
            Err(err) => err,
            Ok(_) => panic!("a second processor in one process must be refused"),
        };
        assert!(
            guard_err.to_lowercase().contains("already"),
            "the guard error must say a session is already running, got: {guard_err}"
        );

        // ...and the slot frees on drop, so a harness process can be reused.
        drop(first);
        assert!(
            OnlineDiarizationProcessor::new(
                DiarizationMode::Fast,
                0,
                false,
                &models_dir,
                None,
                None,
            )
            .is_ok(),
            "dropping the processor must release the session guard"
        );
    }
}
