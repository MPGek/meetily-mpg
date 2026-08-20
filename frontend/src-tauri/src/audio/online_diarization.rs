// audio/online_diarization.rs
//
// Online speaker diarization: polyvoice's streaming/clustering machinery fed
// by polyvoice's own ONNX embedder (WeSpeaker ResNet34 INT8 — the same model
// the offline path uses, so labels stay consistent between the two paths).
// Consumes VAD-merged 16 kHz speech chunks from the pipeline's
// `embedding_sender` and produces per-transcript speaker assignments at
// recording stop:
//   - Efficient mode: ResNet34 embedder per speech segment, buffered per
//     channel, clustered with polyvoice `AhcClusterer` at stop.
//   - Fast mode: polyvoice `StreamingPipeline` (arrival-order speaker cache)
//     per channel; stable turns buffered internally, flushed at stop.
// Both modes reuse the offline per-channel label scheme: mic -> MIC_SPEAKER_NN,
// system -> SPEAKER_NN.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use log::{info, warn};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::mpsc::UnboundedSender;

use super::diarization::ClusteredEmbedding;
use super::recording_saver::TranscriptSegment;
use super::recording_state::{AudioChunk, DeviceType};
use super::speaker_recognition::{best_match, MatchResult, Prototype};
use crate::database::repositories::speaker::{SpeakerRepository, SPEAKER_EMBEDDING_MODEL};
use polyvoice::clusterer::Clusterer as _;
use polyvoice::embedder::Embedder as _;

static ONLINE_DIARIZATION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Guards against two online diarization sessions running at once.
pub struct OnlineDiarizationGuard;

impl OnlineDiarizationGuard {
    fn acquire() -> Result<Self, String> {
        if ONLINE_DIARIZATION_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Online diarization already active".to_string());
        }
        Ok(OnlineDiarizationGuard)
    }
}

impl Drop for OnlineDiarizationGuard {
    fn drop(&mut self) {
        ONLINE_DIARIZATION_ACTIVE.store(false, Ordering::SeqCst);
    }
}

pub fn is_online_diarization_active() -> bool {
    ONLINE_DIARIZATION_ACTIVE.load(Ordering::SeqCst)
}

/// Mirrors the frontend `diarizationMode` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiarizationMode {
    Off,
    Efficient,
    Fast,
}

impl DiarizationMode {
    pub fn parse(opt: Option<&str>) -> Self {
        match opt.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("fast") => DiarizationMode::Fast,
            Some("efficient") => DiarizationMode::Efficient,
            _ => DiarizationMode::Off,
        }
    }

    pub fn is_online(self) -> bool {
        matches!(self, DiarizationMode::Efficient | DiarizationMode::Fast)
    }
}

/// One transcript-level speaker assignment, keyed by the transcript's
/// `sequence_id` so the frontend can attach it before the DB save.
#[derive(Debug, Clone, Serialize)]
pub struct SpeakerAssignment {
    pub sequence_id: u64,
    pub speaker: String,
}

/// A live speaker turn emitted to the frontend during Fast-mode recording.
/// `speaker` is the raw cluster label (drives color/side); `display_name` is
/// the recognized registry name, if any, to show instead of the label.
#[derive(Debug, Clone, Serialize)]
pub struct SpeakerTurn {
    pub start_time: f64,
    pub end_time: f64,
    pub speaker: String,
    pub source_device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Whether this turn's identity came from a user binding (`user`), an
    /// automatic registry match (`auto`), or neither (None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_by: Option<String>,
    /// Cosine similarity of the automatic recognition (0..1), when recognized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_score: Option<f32>,
}

/// Per-channel clustered embeddings produced at recording stop, shaped for the
/// shared `persist_and_recognize_session` (centroid + exemplar persistence +
/// auto-recognition). `saw_system_audio` is the online equivalent of the
/// offline `is_stereo` flag. Raw per-chunk buffers are carried so
/// ground-truth enrollment of user-assigned blocks can reuse session audio.
#[derive(Debug, Clone, Default)]
pub struct OnlineClusterEmbeddings {
    pub mic: Vec<ClusteredEmbedding>,
    pub sys: Vec<ClusteredEmbedding>,
    pub saw_system_audio: bool,
    /// Raw timestamped mic-channel chunk embeddings (`(start, end, embedding)`).
    pub mic_raw: Vec<(f32, f32, Vec<f32>)>,
    /// Raw timestamped system-channel chunk embeddings (`(start, end, embedding)`).
    pub sys_raw: Vec<(f32, f32, Vec<f32>)>,
}

/// In-memory prototype store shared between the online diarization processor
/// (Fast mode) and the `assign_live_speaker` command, so mid-recording renames
/// take effect immediately for the remainder of the session (design D6).
/// Holds the candidate prototypes for live matching, speaker id -> name, the
/// session's cluster -> person bindings, and per-(channel, pipeline-speaker)
/// buffered embeddings used to seed a newly created person's prototypes. Keys
/// are channel-qualified so mic and system voices never mix (design: keep
/// enrollment seeding channel-clean).
pub struct PrototypeStore {
    pub prototypes: Vec<Prototype>,
    pub names: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    /// Session chunk embeddings keyed by `(channel, pipeline_speaker_id)`.
    pub session_embeddings: HashMap<(String, usize), Vec<(Vec<f32>, f32)>>,
    /// Session mic label prefix ("MIC_SPEAKER" for stereo, "SPEAKER" for mono).
    /// Used to resolve a label's channel when seeding prototypes on bind.
    pub mic_prefix: String,
}

impl PrototypeStore {
    pub fn new(mic_prefix: String) -> Self {
        Self {
            prototypes: Vec::new(),
            names: HashMap::new(),
            bindings: HashMap::new(),
            session_embeddings: HashMap::new(),
            mic_prefix,
        }
    }

    /// Load candidate prototypes + speaker names. When `candidate_ids` is
    /// None (empty allowlist), loads prototypes for ALL speakers.
    /// `has_system_device` fixes the session's channel scheme.
    pub async fn load(
        pool: &SqlitePool,
        candidate_ids: Option<Vec<String>>,
        has_system_device: bool,
    ) -> Result<Self, String> {
        let candidates_ref = candidate_ids.as_deref();
        let prototypes: Vec<Prototype> = SpeakerRepository::load_prototypes(
            pool,
            candidates_ref,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .map_err(|e| format!("Failed to load prototypes: {}", e))?
        .into_iter()
        .map(Prototype::from)
        .collect();

        let speakers = SpeakerRepository::list_speakers(pool)
            .await
            .map_err(|e| format!("Failed to list speakers: {}", e))?;
        let names: HashMap<String, String> =
            speakers.into_iter().map(|s| (s.id, s.name)).collect();

        let mic_prefix = if has_system_device {
            "MIC_SPEAKER".to_string()
        } else {
            "SPEAKER".to_string()
        };

        Ok(Self {
            prototypes,
            names,
            bindings: HashMap::new(),
            session_embeddings: HashMap::new(),
            mic_prefix,
        })
    }

    /// The channel and pipeline speaker id encoded in a cluster label. For a
    /// `MIC_SPEAKER_` prefix the channel is always mic. A bare `SPEAKER_`
    /// prefix is the system channel in a stereo session and the mic channel in
    /// a mono session (the session's `mic_prefix` disambiguates).
    pub fn channel_and_id(&self, cluster_label: &str) -> Option<(String, usize)> {
        let id = parse_pipeline_id(cluster_label)?;
        let channel = if cluster_label.starts_with("MIC_SPEAKER_") {
            "mic".to_string()
        } else if cluster_label.starts_with("SPEAKER_") {
            if self.mic_prefix == "MIC_SPEAKER" {
                "system".to_string()
            } else {
                "mic".to_string()
            }
        } else {
            return None;
        };
        Some((channel, id))
    }

    /// Match an embedding against the store; returns the recognized match
    /// (speaker id + score) when above threshold, else None.
    pub fn recognize(&self, embedding: &[f32], channel: &str) -> Option<MatchResult> {
        best_match(embedding, Some(channel), &self.prototypes)
    }

    /// Record a chunk embedding tagged with its channel + pipeline speaker id,
    /// for seeding a newly created person's prototypes on live rename.
    pub fn push_session(
        &mut self,
        pipeline_id: usize,
        embedding: Vec<f32>,
        duration: f32,
        channel: String,
    ) {
        self.session_embeddings
            .entry((channel, pipeline_id))
            .or_default()
            .push((embedding, duration));
    }

    /// Bind a cluster to a person and merge that cluster's session-derived
    /// embeddings as the person's prototypes, so subsequent chunks match. Only
    /// embeddings from the cluster's own channel are seeded (never both).
    pub fn bind(&mut self, cluster_label: &str, speaker_id: &str, name: &str) {
        self.bindings
            .insert(cluster_label.to_string(), speaker_id.to_string());
        self.names
            .insert(speaker_id.to_string(), name.to_string());
        if let Some((channel, pid)) = self.channel_and_id(cluster_label) {
            if let Some(embs) = self.session_embeddings.get(&(channel.clone(), pid)) {
                for (emb, _dur) in embs.iter() {
                    self.prototypes.push(Prototype {
                        speaker_id: speaker_id.to_string(),
                        channel: channel.clone(),
                        embedding: emb.clone(),
                    });
                }
            }
        }
    }

    /// The session binding for a cluster label, if any (used at stop-time
    /// finalize to apply user bindings + enrollment).
    pub fn binding_for(&self, cluster_label: &str) -> Option<String> {
        self.bindings.get(cluster_label).cloned()
    }

    /// All session cluster -> speaker_id bindings (for stop-time finalize).
    pub fn bindings(&self) -> &HashMap<String, String> {
        &self.bindings
    }
}

/// Parse the trailing pipeline speaker index from a cluster label like
/// "MIC_SPEAKER_01" or "SPEAKER_02".
fn parse_pipeline_id(cluster_label: &str) -> Option<usize> {
    let last = cluster_label.rsplit('_').next()?;
    last.parse::<usize>().ok()
}

/// polyvoice `Embedder` backed by the polyvoice ONNX ResNet34 INT8 model
/// (16 kHz, 256 dims). Same embedder family as the offline path.
type DiarizationEmbedder = polyvoice::embedder::ResNet34Adapter;

fn create_resnet34_embedder(embedding_model: &Path) -> Result<DiarizationEmbedder, String> {
    if !embedding_model.exists() {
        return Err(format!(
            "Embedding model not found at {}. Download models in Settings.",
            embedding_model.display()
        ));
    }
    DiarizationEmbedder::new(embedding_model, 1, polyvoice::onnx::ExecutionProvider::Cpu)
        .map_err(|e| format!("Failed to create speaker embedding extractor: {}", e))
}

#[derive(Debug, Clone)]
struct SpeakerSegment {
    start: f32,
    end: f32,
    speaker: usize,
}

/// (start_time, end_time, embedding_vector) buffer per channel (Efficient mode).
#[derive(Default)]
struct EmbeddingBuffer {
    entries: Vec<(f32, f32, Vec<f32>)>,
}

impl EmbeddingBuffer {
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn push(&mut self, start: f32, end: f32, embedding: Vec<f32>) {
        self.entries.push((start, end, embedding));
    }

    fn cluster(&self, max_speakers: usize) -> Vec<SpeakerSegment> {
        if self.entries.is_empty() {
            return Vec::new();
        }
        if self.entries.len() == 1 {
            return vec![SpeakerSegment {
                start: self.entries[0].0,
                end: self.entries[0].1,
                speaker: 0,
            }];
        }

        let embeddings: Vec<Vec<f32>> = self.entries.iter().map(|e| e.2.clone()).collect();
        let clusterer = polyvoice::clusterer::MinClusterSizeClusterer::new(
            Box::new(polyvoice::clusterer::AhcClusterer::with_threshold(
                max_speakers,
                polyvoice::DEFAULT_AHC_THRESHOLD,
            )),
            2,
        );

        match clusterer.cluster(&embeddings) {
            Ok(labels) => self
                .entries
                .iter()
                .zip(labels)
                .map(|(e, label)| SpeakerSegment {
                    start: e.0,
                    end: e.1,
                    speaker: label,
                })
                .collect(),
            Err(e) => {
                warn!("AhcClusterer failed ({}), treating all segments as one speaker", e);
                self.entries
                    .iter()
                    .map(|e| SpeakerSegment {
                        start: e.0,
                        end: e.1,
                        speaker: 0,
                    })
                    .collect()
            }
        }
    }
}

/// Maps the compressed pipeline timeline (only fed samples) back to absolute
/// recording time. Each fed chunk records an anchor; silence gaps between
/// chunks are skipped in pipeline time and accounted for here.
#[derive(Default)]
struct TimelineMapper {
    anchors: Vec<(f64, f64, f64, f64)>, // (p_start, a_start, p_end, a_end) per chunk
}

impl TimelineMapper {
    fn push_chunk(&mut self, abs_start: f64, sample_secs: f64) {
        let (p_start, prev_a_end) = self
            .anchors
            .last()
            .map_or((0.0, 0.0), |a| (a.2, a.3));
        let a_start = abs_start.max(prev_a_end);
        self.anchors.push((
            p_start,
            a_start,
            p_start + sample_secs,
            a_start + sample_secs,
        ));
    }

    fn to_abs(&self, t: f64) -> f64 {
        let mut anchor = self.anchors.last();
        for a in &self.anchors {
            if t < a.2 {
                anchor = Some(a);
                break;
            }
        }
        match anchor {
            Some((p_start, a_start, ..)) => a_start + (t - p_start),
            None => t,
        }
    }
}

struct FastChannel {
    pipeline: polyvoice::streaming::StreamingPipeline<
        polyvoice::vad::EnergyVad,
        DiarizationEmbedder,
    >,
    mapper: TimelineMapper,
    /// Stable turns in pipeline time, translated at finalize.
    turns: Vec<SpeakerSegment>,
}

enum Engine {
    Efficient {
        extractor: DiarizationEmbedder,
        mic: EmbeddingBuffer,
        sys: EmbeddingBuffer,
    },
    Fast {
        mic: FastChannel,
        sys: FastChannel,
        /// Own ResNet34 extractor: polyvoice's StreamingPipeline turns carry
        /// no embedding, so Fast mode embeds each chunk itself (design D5).
        extractor: DiarizationEmbedder,
        /// Per-channel chunk embeddings buffered for stop-time centroids and
        /// enrollment, grouped by pipeline speaker at stop via time overlap.
        mic_emb: EmbeddingBuffer,
        sys_emb: EmbeddingBuffer,
    },
}

/// Receives VAD-filtered 16 kHz speech chunks and produces speaker
/// assignments at recording stop. No-op while in the error state.
pub struct OnlineDiarizationProcessor {
    mode: DiarizationMode,
    max_speakers: usize,
    saw_system_audio: bool,
    /// Channel-scoped label prefix for the microphone channel, fixed for the
    /// whole session. `MIC_SPEAKER` when a system device was captured (stereo),
    /// else `SPEAKER` (mono). Chosen at construction so live labels and
    /// stop-time assignments always agree.
    mic_prefix: String,
    engine: Option<Engine>,
    turn_sender: Option<UnboundedSender<SpeakerTurn>>,
    /// Shared live-recognition store (Fast mode). None in Efficient mode or
    /// when no registry prototypes were loaded.
    prototype_store: Option<Arc<RwLock<PrototypeStore>>>,
    _guard: OnlineDiarizationGuard,
}

const MIN_SEGMENT_SAMPLES: usize = 3200; // 200 ms at 16 kHz (spec minimum)

impl OnlineDiarizationProcessor {
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

        let embedding_model = super::diarization::diarization_model_paths(models_dir).1;
        let mic_prefix = if has_system_device {
            "MIC_SPEAKER".to_string()
        } else {
            "SPEAKER".to_string()
        };

        let engine = match mode {
            DiarizationMode::Efficient => {
                let extractor = create_resnet34_embedder(&embedding_model)?;
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
                let extractor = create_resnet34_embedder(&embedding_model)?;
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

        info!(
            "Online diarization processor initialized (mode: {:?}, max_speakers: {}, prototype_store: {}, mic_prefix: {})",
            mode,
            max_speakers,
            prototype_store.is_some(),
            mic_prefix
        );

        Ok(Self {
            mode,
            max_speakers,
            saw_system_audio: false,
            mic_prefix,
            engine: Some(engine),
            turn_sender,
            prototype_store,
            _guard: guard,
        })
    }

    pub fn mode(&self) -> DiarizationMode {
        self.mode
    }

    pub fn is_in_error_state(&self) -> bool {
        self.engine.is_none()
    }

    /// Routes one audio chunk to the active engine. No-op in the error state
    /// or when the chunk is too short to embed. A single chunk that fails to
    /// embed or feed is skipped (logged) rather than disabling the whole
    /// session: engine error state is reserved for init failures, so a
    /// transient bad chunk never wipes the remaining recording's labels.
    pub fn process_chunk(&mut self, chunk: AudioChunk) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if chunk.data.len() < MIN_SEGMENT_SAMPLES {
            return;
        }

        let samples: std::borrow::Cow<'_, [f32]> = if chunk.sample_rate != 16000 {
            match super::audio_processing::resample(&chunk.data, chunk.sample_rate, 16000) {
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

        // Capture live-emission state before borrowing the engine, so the
        // Fast-mode loop can send turns without conflicting borrows.
        let turn_sender = self.turn_sender.clone();
        let prototype_store = self.prototype_store.clone();
        let mic_prefix = self.mic_prefix.as_str();

        match engine {
            Engine::Efficient { extractor, mic, sys } => {
                let embedding = match extractor.embed(&samples) {
                    Ok(emb) => emb,
                    Err(e) => {
                        warn!("Embedding extraction failed ({}), skipping chunk", e);
                        return;
                    }
                };
                let start = chunk.timestamp as f32;
                let end = start + samples.len() as f32 / 16000.0;
                match chunk.device_type {
                    DeviceType::Microphone => mic.push(start, end, embedding),
                    DeviceType::System => sys.push(start, end, embedding),
                }
            }
            Engine::Fast { mic, sys, extractor, mic_emb, sys_emb } => {
                let (channel, source_device, prefix, emb_buf, channel_str) = match chunk.device_type
                {
                    DeviceType::Microphone => (
                        mic,
                        "Microphone",
                        mic_prefix,
                        mic_emb,
                        "mic",
                    ),
                    DeviceType::System => (sys, "System", "SPEAKER", sys_emb, "system"),
                };

                // Fast mode embeds each chunk itself (pipeline turns carry no
                // embedding) for live recognition, buffering, and enrollment.
                let chunk_embedding = match extractor.embed(&samples) {
                    Ok(emb) => emb,
                    Err(e) => {
                        warn!("Fast-mode embedding failed ({}), disabling online diarization", e);
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

                channel
                    .mapper
                    .push_chunk(chunk.timestamp, samples.len() as f64 / 16000.0);
                match channel.pipeline.feed(&samples) {
                    Ok(turns) => {
                        for turn in turns {
                            if turn.stable {
                                let speaker_index = turn.speaker.0 as usize;
                                let turn_start = turn.time.start as f32;
                                let turn_end = turn.time.end as f32;
                                channel.turns.push(SpeakerSegment {
                                    start: turn_start,
                                    end: turn_end,
                                    speaker: speaker_index,
                                });
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
                                if let Some(sender) = &turn_sender {
                                    let turn_label = format!("{}_{:02}", prefix, speaker_index);
                                    // A user-bound cluster labels its turns with
                                    // the user's chosen name; otherwise the
                                    // automatic recognition name (if any) is shown.
                                    let bound_speaker =
                                        prototype_store.as_ref().and_then(|store| {
                                            store
                                                .read()
                                                .ok()
                                                .and_then(|s| s.bindings().get(&turn_label).cloned())
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
                                    if let Err(e) = sender.send(turn_event) {
                                        warn!("Failed to send online speaker turn: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("StreamingPipeline feed failed ({}), disabling online diarization", e);
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
    ) -> Result<(Vec<SpeakerAssignment>, OnlineClusterEmbeddings, HashMap<String, String>), String>
    {
        let Some(engine) = self.engine.take() else {
            return Err("Online diarization unavailable (error state)".to_string());
        };

        let mic_prefix = self.mic_prefix.clone();
        let (mic_segments, sys_segments, mic_clustered, sys_clustered, mic_raw, sys_raw) =
            match engine {
                Engine::Efficient { extractor: _, mic, sys } => {
                    let mic_segments = mic.cluster(self.max_speakers);
                    let sys_segments = sys.cluster(self.max_speakers);
                    // Efficient mode: embeddings are buffered per segment; cluster()
                    // returns labels aligned with the buffer entries, so group by
                    // those labels directly.
                    let mic_clustered = cluster_embeddings_by_labels(&mic.entries, &mic_segments);
                    let sys_clustered = cluster_embeddings_by_labels(&sys.entries, &sys_segments);
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
                    mut mic,
                    mut sys,
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
                        cluster_embeddings_by_overlap(&mic_emb.entries, &mic_segments);
                    let sys_clustered =
                        cluster_embeddings_by_overlap(&sys_emb.entries, &sys_segments);
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
            mic_raw,
            sys_raw,
        };

        // Extract live user bindings (Fast-mode renames) to apply at the
        // post-save finalize, when the meeting row exists.
        let live_bindings = self
            .prototype_store
            .as_ref()
            .and_then(|store| store.read().ok())
            .map(|s| s.bindings().clone())
            .unwrap_or_default();

        if mic_segments.is_empty() && sys_segments.is_empty() {
            info!("Online diarization: no speech segments detected, skipping transcript updates");
            return Ok((Vec::new(), clusters, live_bindings));
        }

        let mut assignments = Vec::new();
        let mut skipped_no_match = 0usize;
        for transcript in transcripts {
            let t_start = transcript.audio_start_time as f32;
            let t_end = transcript.audio_end_time as f32;

            // Same channel scheme as offline diarization: system-source
            // transcripts match system-channel segments (SPEAKER_NN); all
            // others match mic-channel segments. When no system audio was
            // captured, everything matches the single run. The mic prefix
            // always comes from the session-stable `mic_prefix`.
            let (segments, prefix) = if self.saw_system_audio
                && transcript.source_device.as_str() == "System"
            {
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

/// Group Efficient-mode buffered embeddings by their cluster labels (from
/// `EmbeddingBuffer::cluster`, aligned with `entries` by position).
fn cluster_embeddings_by_labels(
    entries: &[(f32, f32, Vec<f32>)],
    segments: &[SpeakerSegment],
) -> Vec<ClusteredEmbedding> {
    entries
        .iter()
        .zip(segments.iter())
        .map(|((start, end, emb), seg)| ClusteredEmbedding {
            speaker: seg.speaker as i32,
            embedding: emb.clone(),
            duration_secs: (end - start).max(0.0),
            start_secs: Some(*start),
            end_secs: Some(*end),
        })
        .collect()
}

/// Group Fast-mode buffered embeddings by pipeline speaker id via time-overlap
/// with the stable turns (which carry speaker ids). Embeddings with no
/// overlapping turn are dropped (no cluster to attribute).
fn cluster_embeddings_by_overlap(
    entries: &[(f32, f32, Vec<f32>)],
    segments: &[SpeakerSegment],
) -> Vec<ClusteredEmbedding> {
    entries
        .iter()
        .filter_map(|(start, end, emb)| {
            find_best_speaker(segments, *start, *end).map(|spk| ClusteredEmbedding {
                speaker: spk as i32,
                embedding: emb.clone(),
                duration_secs: (end - start).max(0.0),
                start_secs: Some(*start),
                end_secs: Some(*end),
            })
        })
        .collect()
}

fn create_fast_channel(embedding_model: &Path) -> Result<FastChannel, String> {
    use polyvoice::streaming::{LatencyPreset, StreamingPipeline};
    use polyvoice::vad::{EnergyVad, VadConfig};

    let extractor = create_resnet34_embedder(embedding_model)?;
    let vad = EnergyVad::new(-100.0, 16000, 512);
    let pipeline = StreamingPipeline::with_latency_preset(
        vad,
        extractor,
        LatencyPreset::Balanced,
        VadConfig::default(),
    )
    .map_err(|e| format!("Failed to create StreamingPipeline: {}", e))?;

    Ok(FastChannel {
        pipeline,
        mapper: TimelineMapper::default(),
        turns: Vec::new(),
    })
}

fn find_best_speaker(segments: &[SpeakerSegment], t_start: f32, t_end: f32) -> Option<usize> {
    let mut best_speaker: Option<usize> = None;
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

    if best_speaker.is_some() {
        return best_speaker;
    }
    if segments.is_empty() {
        return None;
    }

    let first = segments[0].speaker;
    if segments.iter().all(|s| s.speaker == first) {
        return Some(first);
    }

    const MAX_GAP_SECS: f32 = 30.0;
    let mut nearest: Option<(f32, usize)> = None;
    for seg in segments {
        let gap = if seg.end < t_start {
            t_start - seg.end
        } else if seg.start > t_end {
            seg.start - t_end
        } else {
            0.0
        };
        if gap <= MAX_GAP_SECS && nearest.map_or(true, |(g, _)| gap < g) {
            nearest = Some((gap, seg.speaker));
        }
    }
    nearest.map(|(_, spk)| spk)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(mic_prefix: &str) -> PrototypeStore {
        PrototypeStore::new(mic_prefix.to_string())
    }

    #[test]
    fn parse_pipeline_id_parses_trailing_index() {
        assert_eq!(parse_pipeline_id("MIC_SPEAKER_03"), Some(3));
        assert_eq!(parse_pipeline_id("SPEAKER_02"), Some(2));
        assert_eq!(parse_pipeline_id("SPEAKER_00"), Some(0));
        assert_eq!(parse_pipeline_id("SPEAKER"), None);
        assert_eq!(parse_pipeline_id("SystemAudio"), None);
    }

    #[test]
    fn channel_and_id_stereo_resolves_mic_and_system() {
        let store = store_with("MIC_SPEAKER");
        assert_eq!(store.channel_and_id("MIC_SPEAKER_03"), Some(("mic".to_string(), 3)));
        assert_eq!(store.channel_and_id("SPEAKER_02"), Some(("system".to_string(), 2)));
        assert_eq!(store.channel_and_id("SPEAKER_00"), Some(("system".to_string(), 0)));
    }

    #[test]
    fn channel_and_id_mono_resolves_bare_prefix_as_mic() {
        let store = store_with("SPEAKER");
        assert_eq!(store.channel_and_id("SPEAKER_01"), Some(("mic".to_string(), 1)));
        assert_eq!(store.channel_and_id("SPEAKER_00"), Some(("mic".to_string(), 0)));
    }

    #[test]
    fn channel_and_id_rejects_unknown_prefixes() {
        let store = store_with("MIC_SPEAKER");
        assert_eq!(store.channel_and_id("SystemAudio"), None);
        assert_eq!(store.channel_and_id("AVATAR_3"), None);
    }

    #[test]
    fn bind_seeds_only_the_bound_channels_embeddings() {
        // Stereo session: mic speaker 0 and system speaker 0 share the same
        // numeric id; binding the mic cluster must NOT pull in system embeds.
        let mut store = store_with("MIC_SPEAKER");
        store.push_session(0, vec![1.0, 0.0, 0.0, 0.0], 2.0, "mic".to_string());
        store.push_session(0, vec![0.0, 1.0, 0.0, 0.0], 2.0, "system".to_string());
        store.push_session(1, vec![0.0, 0.0, 1.0, 0.0], 1.0, "mic".to_string());

        store.bind("MIC_SPEAKER_00", "speaker-alice", "Alice");

        // The single mic-0 embedding seeds as Alice's prototype; the system-0
        // embedding must NOT be among them.
        let alice_protos: Vec<&Prototype> = store
            .prototypes
            .iter()
            .filter(|p| p.speaker_id == "speaker-alice")
            .collect();
        assert_eq!(alice_protos.len(), 1, "system embedding leaked into mic binding");
        assert!(alice_protos.iter().all(|p| p.channel == "mic"));
        assert!(alice_protos.iter().all(|p| (p.embedding[0] - 1.0).abs() < 1e-9));
    }

    #[test]
    fn bind_system_cluster_seeds_only_system_embeddings() {
        let mut store = store_with("MIC_SPEAKER");
        store.push_session(0, vec![1.0, 0.0, 0.0, 0.0], 2.0, "mic".to_string());
        store.push_session(0, vec![0.0, 1.0, 0.0, 0.0], 2.0, "system".to_string());

        store.bind("SPEAKER_00", "speaker-bob", "Bob");

        let bob_protos: Vec<&Prototype> = store
            .prototypes
            .iter()
            .filter(|p| p.speaker_id == "speaker-bob")
            .collect();
        assert_eq!(bob_protos.len(), 1, "mic embedding leaked into system binding");
        assert_eq!(bob_protos[0].channel, "system");
        assert!((bob_protos[0].embedding[1] - 1.0).abs() < 1e-9);
    }
}
