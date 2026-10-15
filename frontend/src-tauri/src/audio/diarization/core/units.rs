//! The units the diarization stages exchange: a labeled timeline segment, a
//! clustered embedding, a channel's clusters, the assembled pipeline, and the
//! progress/result payloads the frontend sees.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct DiarizationSegment {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
}

/// An embedding tagged with its cluster id and source-segment duration,
/// produced after clustering. Used to compute per-cluster centroids and
/// exemplar caches for the speaker identity registry. Public so the online
/// diarization path can build the same shape at recording stop.
#[derive(Debug, Clone)]
pub struct ClusteredEmbedding {
    pub speaker: i32,
    pub embedding: Vec<f32>,
    pub duration_secs: f32,
    pub start_secs: Option<f32>,
    pub end_secs: Option<f32>,
}

/// Per-channel diarization output: labeled segments plus the clustered
/// embeddings aligned to them (captured before the start-time sort).
#[derive(Debug, Clone, Default)]
pub struct ChannelClusters {
    pub segments: Vec<DiarizationSegment>,
    pub embeddings: Vec<ClusteredEmbedding>,
}

/// Polyvoice diarization engine (pipeline_v2 architecture): enhanced
/// segmentation-3.0 with calibrated binarization + TitaNet-Large embedding +
/// kind-selected clusterer + overlap resegmenter, loaded once per run.
pub struct PolyvoiceDiarizer {
    pub(crate) segmenter: Box<dyn polyvoice::segmentation::Segmenter>,
    pub(crate) embedder: Box<dyn crate::audio::embedder::SpeakerEmbedder>,
    pub(crate) clusterer: Box<dyn polyvoice::clusterer::Clusterer>,
    pub(crate) resegmenter: polyvoice::resegmentation::OverlapResegmenter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub status: String,
    pub progress: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub segments_labeled: usize,
    pub speakers_found: usize,
}

/// Per-stage wall-clock accounting for one channel's pass, reported in the
/// run summary.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StageTimings {
    pub(crate) decode_secs: f64,
    pub(crate) segmentation_secs: f64,
    pub(crate) embedding_secs: f64,
    pub(crate) clustering_secs: f64,
    pub(crate) resegmentation_secs: f64,
    pub(crate) matching_secs: f64,
}

impl StageTimings {}

pub(crate) fn count_unique_speakers(segments: &[DiarizationSegment]) -> usize {
    let mut speakers: Vec<i32> = segments.iter().map(|s| s.speaker).collect();
    speakers.sort();
    speakers.dedup();
    speakers.len()
}

pub(crate) type TimeRange = polyvoice::types::TimeRange;
pub(crate) type RawSegment = polyvoice::segmentation::RawSegment;
pub(crate) type SpeakerTurn = polyvoice::types::SpeakerTurn;
