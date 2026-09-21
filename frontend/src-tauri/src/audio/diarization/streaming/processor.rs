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
use super::super::persist::clusters::{clustered_embeddings, EmbeddingLabeling};
use super::super::identity::prototypes::PrototypeStore;
use super::super::telemetry::{
    diar_channel, record_stats, DiarizationMode, OnlineDiarizationStats,
};
use super::super::core::factory::create_streaming_embedder;
use super::engine::{create_fast_channel, Engine};
use super::guard::OnlineDiarizationGuard;
use super::units::{OnlineClusterEmbeddings, SpeakerAssignment, SpeakerTurn};

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
                .map(|s| std::sync::Arc::downgrade(s)),
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
    /// `(assignments, cluster_embeddings, live_user_bindings)`. An empty
    /// assignment list on "no speech detected" is not an error.
    pub fn finalize(
        &mut self,
        transcripts: &[TranscriptSegment],
    ) -> Result<
        (
            Vec<SpeakerAssignment>,
            OnlineClusterEmbeddings,
            HashMap<String, String>,
        ),
        String,
    > {
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
                    // Fast mode: buffer entries have no cluster id; group them by
                    // time-overlap with the stable turns (which carry pipeline
                    // speaker ids), using the same find_best_speaker logic.
                    let mic_clustered =
                        clustered_embeddings(&mic_emb.entries, EmbeddingLabeling::ByOverlap(&mic_segments));
                    let sys_clustered =
                        clustered_embeddings(&sys_emb.entries, EmbeddingLabeling::ByOverlap(&sys_segments));
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
            return Ok((Vec::new(), clusters, live_bindings));
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
        Ok((assignments, clusters, live_bindings))
    }
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
        set_clustering_overrides(Some(0.42), Some(9), None, Some(ClustererKindSetting::Ahc));
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
                set_clustering_overrides(None, None, None, None);
                return;
            }
        };
        assert_eq!(processor.config.cluster_threshold, 0.42);
        assert_eq!(processor.config.cluster_ceiling, 9);

        // A settings change after the session started must not reach it.
        set_clustering_overrides(Some(0.81), Some(3), None, Some(ClustererKindSetting::Nmesc));
        assert_eq!(
            processor.config.cluster_threshold, 0.42,
            "session config is a snapshot taken at construction"
        );
        assert_eq!(processor.config.cluster_ceiling, 9);
        assert_eq!(processor.config.clusterer, ClustererKindSetting::Ahc);
        // ... while a newly resolved config does see it.
        assert_eq!(DiarizationConfig::resolved().cluster_threshold, 0.81);

        drop(processor);
        set_clustering_overrides(
            Some(saved.cluster_threshold),
            Some(saved.cluster_ceiling),
            Some(saved.gap_merge_secs),
            Some(saved.clusterer),
        );
        set_clustering_overrides(None, None, None, None);
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
