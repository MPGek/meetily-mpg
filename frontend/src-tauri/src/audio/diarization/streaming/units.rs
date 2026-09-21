//! The units a live session hands back: one transcript-level assignment, a
//! speaker turn, and a channel's buffered cluster embeddings.

use serde::Serialize;

use super::super::ClusteredEmbedding;

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
    /// Model family tag that produced the centroids (for family-aware persistence).
    pub model_tag: Option<String>,
    /// Raw timestamped mic-channel chunk embeddings (`(start, end, embedding)`).
    pub mic_raw: Vec<(f32, f32, Vec<f32>)>,
    /// Raw timestamped system-channel chunk embeddings (`(start, end, embedding)`).
    pub sys_raw: Vec<(f32, f32, Vec<f32>)>,
}
