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

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{info, warn};
use serde::Serialize;

use super::recording_saver::TranscriptSegment;
use super::recording_state::{AudioChunk, DeviceType};
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
    },
}

/// Receives VAD-filtered 16 kHz speech chunks and produces speaker
/// assignments at recording stop. No-op while in the error state.
pub struct OnlineDiarizationProcessor {
    mode: DiarizationMode,
    max_speakers: usize,
    saw_system_audio: bool,
    engine: Option<Engine>,
    _guard: OnlineDiarizationGuard,
}

const MIN_SEGMENT_SAMPLES: usize = 3200; // 200 ms at 16 kHz (spec minimum)

impl OnlineDiarizationProcessor {
    /// Initializes polyvoice components based on mode. Model initialization is
    /// blocking — call from a blocking task.
    pub fn new(
        mode: DiarizationMode,
        max_speakers: usize,
        models_dir: &Path,
    ) -> Result<Self, String> {
        if !mode.is_online() {
            return Err("Online diarization disabled".to_string());
        }
        let guard = OnlineDiarizationGuard::acquire()?;

        let embedding_model = super::diarization::diarization_model_paths(models_dir).1;

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
                Engine::Fast { mic, sys }
            }
            DiarizationMode::Off => unreachable!(),
        };

        info!(
            "Online diarization processor initialized (mode: {:?}, max_speakers: {})",
            mode, max_speakers
        );

        Ok(Self {
            mode,
            max_speakers,
            saw_system_audio: false,
            engine: Some(engine),
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
    /// or when the chunk is too short to embed.
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

        match engine {
            Engine::Efficient { extractor, mic, sys } => {
                let embedding = match extractor.embed(&samples) {
                    Ok(emb) => emb,
                    Err(e) => {
                        warn!("Embedding extraction failed ({}), disabling online diarization", e);
                        self.engine = None;
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
            Engine::Fast { mic, sys } => {
                let channel = match chunk.device_type {
                    DeviceType::Microphone => mic,
                    DeviceType::System => sys,
                };
                channel
                    .mapper
                    .push_chunk(chunk.timestamp, samples.len() as f64 / 16000.0);
                match channel.pipeline.feed(&samples) {
                    Ok(turns) => {
                        for turn in turns {
                            if turn.stable {
                                channel.turns.push(SpeakerSegment {
                                    start: turn.time.start as f32,
                                    end: turn.time.end as f32,
                                    speaker: turn.speaker.0 as usize,
                                });
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

    /// Computes speaker assignments for the in-memory transcript segments.
    /// Returns an empty list on "no speech detected" (not an error).
    pub fn finalize(
        &mut self,
        transcripts: &[TranscriptSegment],
    ) -> Result<Vec<SpeakerAssignment>, String> {
        let Some(engine) = self.engine.take() else {
            return Err("Online diarization unavailable (error state)".to_string());
        };

        let (mic_segments, sys_segments) = match engine {
            Engine::Efficient { extractor: _, mic, sys } => {
                let mic_segments = mic.cluster(self.max_speakers);
                let sys_segments = sys.cluster(self.max_speakers);
                (mic_segments, sys_segments)
            }
            Engine::Fast { mut mic, mut sys } => {
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
                (mic_segments, sys_segments)
            }
        };

        if mic_segments.is_empty() && sys_segments.is_empty() {
            info!("Online diarization: no speech segments detected, skipping transcript updates");
            return Ok(Vec::new());
        }

        let mut assignments = Vec::new();
        let mut skipped_no_match = 0usize;
        for transcript in transcripts {
            let t_start = transcript.audio_start_time as f32;
            let t_end = transcript.audio_end_time as f32;

            // Same channel scheme as offline diarization: system-source
            // transcripts match system-channel segments (SPEAKER_NN); all
            // others match mic-channel segments (MIC_SPEAKER_NN). When no
            // system audio was captured, everything matches the single run.
            let (segments, prefix) = if self.saw_system_audio {
                if transcript.source_device.as_str() == "System" {
                    (&sys_segments, "SPEAKER")
                } else {
                    (&mic_segments, "MIC_SPEAKER")
                }
            } else {
                (&mic_segments, "SPEAKER")
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
            "Online diarization finalized: {} assigned, {} no-match skipped",
            assignments.len(),
            skipped_no_match
        );
        Ok(assignments)
    }
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
