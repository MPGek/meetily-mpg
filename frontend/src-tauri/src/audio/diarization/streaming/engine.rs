//! The per-channel live engines: the Fast-mode streaming pipeline with its
//! timeline mapper, and the Efficient-mode embedding buffer behind one enum.

use std::path::Path;

use super::super::core::cluster::{EmbeddingBuffer, SpeakerSegment};

/// Maps the compressed pipeline timeline (only fed samples) back to absolute
/// recording time. Each fed chunk records an anchor; silence gaps between
/// chunks are skipped in pipeline time and accounted for here.
#[derive(Default)]
pub(crate) struct TimelineMapper {
    pub(crate) anchors: Vec<(f64, f64, f64, f64)>, // (p_start, a_start, p_end, a_end) per chunk
}

impl TimelineMapper {
    pub(crate) fn push_chunk(&mut self, abs_start: f64, sample_secs: f64) {
        let (p_start, prev_a_end) = self.anchors.last().map_or((0.0, 0.0), |a| (a.2, a.3));
        let a_start = abs_start.max(prev_a_end);
        self.anchors.push((
            p_start,
            a_start,
            p_start + sample_secs,
            a_start + sample_secs,
        ));
    }

    pub(crate) fn to_abs(&self, t: f64) -> f64 {
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

pub(crate) struct FastChannel {
    pub(crate) pipeline:
        polyvoice::streaming::StreamingPipeline<polyvoice::vad::EnergyVad, DiarizationEmbedder>,
    pub(crate) mapper: TimelineMapper,
    /// Stable turns in pipeline time, translated at finalize.
    pub(crate) turns: Vec<SpeakerSegment>,
}

pub(crate) enum Engine {
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

/// TitaNet-Large embedder (16 kHz, 192 dims) with layout-correct `[B, 80, T]`
/// adapter. Same embedder family as the offline path.
pub(crate) type DiarizationEmbedder = crate::audio::embedder::TitanetAdapter;

pub(crate) fn create_enhanced_embedder(embedding_model: &Path) -> Result<DiarizationEmbedder, String> {
    if !embedding_model.exists() {
        return Err(format!(
            "Enhanced embedding model not found at {}. The enhanced diarization models (segmentation-3.0 + TitaNet-Large) are bundled at build time; rebuild with network or install a build that includes them.",
            embedding_model.display()
        ));
    }
    DiarizationEmbedder::new(embedding_model, 1)
        .map_err(|e| format!("Failed to create enhanced TitaNet embedder: {}", e))
}

pub(crate) fn create_fast_channel(embedding_model: &Path) -> Result<FastChannel, String> {
    use polyvoice::streaming::{LatencyPreset, StreamingPipeline};
    use polyvoice::vad::{EnergyVad, VadConfig};

    let extractor = create_enhanced_embedder(embedding_model)?;
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
