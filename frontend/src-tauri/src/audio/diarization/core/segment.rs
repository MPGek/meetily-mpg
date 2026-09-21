//! Segmentation and dense embedding: the per-chunk units, the chunk record
//! they accumulate into, and the chunked core that turns raw audio windows
//! into clustered speaker turns.

use log::{info, warn};
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::super::batch::guard::DIARIZATION_CANCELLED;
use super::super::config::MIN_EMBED_SECS;
use super::super::DIARIZATION_SAMPLE_RATE;
use super::units::{count_unique_speakers, StageTimings};
use super::super::{
    ClusteredEmbedding, DiarizationConfig, DiarizationSegment, PolyvoiceDiarizer,
    RawSegment, TimeRange,
};
use super::turns::assemble_channel_turns;

/// One dense embedding unit: a window slice of a primary segment, tagged with
/// its chunk-local time span, the segment's local speaker index, and the index
/// of its parent segment in the chunk's primary list.
#[derive(Debug, Clone)]
pub(crate) struct DenseUnit {
    pub(crate) time: TimeRange,
    pub(crate) local_idx: u8,
    pub(crate) segment_idx: usize,
    pub(crate) embedding: Vec<f32>,
}

/// Per-chunk accumulation between chunks (bounded): segment metadata with
/// chunk-local times plus the embedded units. `start_secs` offsets to global
/// channel time.
#[derive(Debug, Default)]
pub(crate) struct ChunkRecord {
    pub(crate) start_secs: f32,
    pub(crate) primary: Vec<RawSegment>,
    pub(crate) overlaps: Vec<(TimeRange, u8, u8)>,
    pub(crate) units: Vec<DenseUnit>,
    /// Mixed-voice embeddings (L2-normalized) precomputed during the chunk
    /// pass for overlap regions where at least one local speaker never
    /// appeared as a primary segment — the resegmentation fallback needs them
    /// after global clustering, when the chunk audio buffer is gone.
    pub(crate) mixed_overlaps: Vec<(TimeRange, Vec<f32>)>,
}

/// Expand primary segments into embedding units. Segments longer than `window`
/// are split into `window`-second sub-windows hopped by `window/2` (dense,
/// v2-style); sub-window segments stay whole. Mirrors the vendored
/// `pipeline_v2::expand_embed_units` (private). `window <= 0` keeps one unit
/// per segment (sparse).
fn expand_embed_units(segs: &[RawSegment], window: f32) -> Vec<(TimeRange, u8, usize)> {
    let w = if window > 0.0 { window as f64 } else { 0.0 };
    let mut out = Vec::new();
    for (idx, seg) in segs.iter().enumerate() {
        if w <= 0.0 || seg.time.end - seg.time.start <= w {
            out.push((seg.time, seg.local_speaker_idx, idx));
            continue;
        }
        let hop = (w / 2.0).max(0.05);
        let mut t = seg.time.start;
        loop {
            let end = (t + w).min(seg.time.end);
            out.push((
                TimeRange {
                    start: t,
                    end,
                },
                seg.local_speaker_idx,
                idx,
            ));
            if end >= seg.time.end {
                break;
            }
            t += hop;
        }
    }
    out
}

/// Embed masked unit slices through the batched multi-core path, falling back
/// to per-item embedding when the batch call fails. Returns embeddings aligned
/// 1:1 with `masked` (`None` marks dropped units: fallback failures,
/// non-finite, or dimension-mismatched vectors).
fn embed_unit_slices(
    embedder: &dyn crate::audio::embedder::SpeakerEmbedder,
    masked: &[Vec<f32>],
) -> Vec<Option<Vec<f32>>> {
    let valid = |emb: Vec<f32>| {
        let ok = emb.len() == embedder.input_dim() && emb.iter().all(|v| v.is_finite());
        if !ok {
            warn!("Skipping invalid embedding (dimension or non-finite values)");
        }
        ok.then_some(emb)
    };
    let refs: Vec<&[f32]> = masked.iter().map(Vec::as_slice).collect();
    match embedder.embed_batch(&refs) {
        Ok(batch) => batch.into_iter().map(valid).collect(),
        Err(e) => {
            warn!(
                "Batch embedding failed ({}), falling back to per-segment embedding",
                e
            );
            masked
                .iter()
                .map(|audio| {
                    embedder
                        .embed(audio)
                        .map_err(|e| {
                            warn!(
                                "Embedding extraction failed for a segment ({}), skipping it",
                                e
                            );
                            e
                        })
                        .ok()
                        .and_then(valid)
                })
                .collect()
        }
    }
}

/// The pipeline_v2 chunked core: per-chunk binarized segmentation + dense
/// embedding accumulation, then global clustering, per-chunk Hungarian
/// local→global mapping, overlap-aware two-speaker resegmentation, minimum-
/// speech filtering, and gap-fill. Bounded memory: only embeddings, segment
/// metadata, and small overlap embeddings accumulate across chunks.
pub(crate) struct V2Core<'a> {
    diarizer: &'a PolyvoiceDiarizer,
    config: &'a DiarizationConfig,
    chunks: Vec<ChunkRecord>,
    had_raw_segments: bool,
    timings: StageTimings,
}

impl<'a> V2Core<'a> {
    pub(crate) fn new(diarizer: &'a PolyvoiceDiarizer, config: &'a DiarizationConfig) -> Self {
        Self {
            diarizer,
            config,
            chunks: Vec::new(),
            had_raw_segments: false,
            timings: StageTimings::default(),
        }
    }

    /// Segment one 16 kHz chunk and embed its dense units (chunk-local times).
    pub(crate) fn process_chunk(&mut self, chunk_start_secs: f32, samples: &[f32]) -> Result<(), String> {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let seg_start = Instant::now();
        let raw_segments = match self.diarizer.segmenter.segment(samples) {
            Ok(segments) => segments,
            Err(e) => {
                warn!("Segmentation failed for a chunk ({}), skipping chunk", e);
                return Ok(());
            }
        };
        self.timings.segmentation_secs += seg_start.elapsed().as_secs_f64();
        if raw_segments.is_empty() {
            return Ok(());
        }
        self.had_raw_segments = true;

        let overlaps = polyvoice::resegmentation::extract_overlap_time_ranges(&raw_segments);
        let primary: Vec<RawSegment> = raw_segments
            .iter()
            .filter(|s| !s.is_overlap)
            .cloned()
            .collect();

        let embed_start = Instant::now();
        let mut units: Vec<DenseUnit> = Vec::new();
        if !primary.is_empty() {
            let specs = expand_embed_units(&primary, self.config.embed_window_secs);
            let sample_rate = DIARIZATION_SAMPLE_RATE as f64;
            let mut masked: Vec<Vec<f32>> = Vec::with_capacity(specs.len());
            let mut kept: Vec<(TimeRange, u8, usize)> = Vec::with_capacity(specs.len());
            for (time, local_idx, segment_idx) in specs {
                let start_idx = (time.start * sample_rate) as usize;
                let end_idx = ((time.end * sample_rate) as usize).min(samples.len());
                if end_idx <= start_idx {
                    continue;
                }
                if (end_idx - start_idx) as f64 / sample_rate < MIN_EMBED_SECS {
                    continue;
                }
                // Zero-fill overlap regions inside the unit so two-speaker
                // audio cannot bias the embedding (v2 masking contract).
                let local_overlaps: Vec<(f32, f32)> = overlaps
                    .iter()
                    .filter_map(|(ot, _, _)| {
                        let lo = ot.start.max(time.start);
                        let hi = ot.end.min(time.end);
                        if hi > lo {
                            Some(((lo - time.start) as f32, (hi - time.start) as f32))
                        } else {
                            None
                        }
                    })
                    .collect();
                let chunk = polyvoice::embedder::apply_overlap_mask(
                    &samples[start_idx..end_idx],
                    &local_overlaps,
                    DIARIZATION_SAMPLE_RATE,
                );
                masked.push(chunk);
                kept.push((time, local_idx, segment_idx));
            }
            let embeddings = embed_unit_slices(self.diarizer.embedder.as_ref(), &masked);
            for ((time, local_idx, segment_idx), embedding) in kept.into_iter().zip(embeddings) {
                if let Some(embedding) = embedding {
                    units.push(DenseUnit {
                        time,
                        local_idx,
                        segment_idx,
                        embedding,
                    });
                }
            }
        }

        // Pre-embed mixed-voice overlap regions whose local speakers never
        // appear solo in this chunk (resegmentation fallback after clustering).
        let primary_locals: std::collections::HashSet<u8> =
            primary.iter().map(|s| s.local_speaker_idx).collect();
        let mut mixed_overlaps: Vec<(TimeRange, Vec<f32>)> = Vec::new();
        let unresolved: Vec<(TimeRange, u8, u8)> = overlaps
            .iter()
            .filter(|(_, lo, hi)| !primary_locals.contains(lo) || !primary_locals.contains(hi))
            .cloned()
            .collect();
        let sample_rate = DIARIZATION_SAMPLE_RATE as f64;
        let embeddable: Vec<(TimeRange, Vec<f32>)> = unresolved
            .into_iter()
            .filter_map(|(time, _, _)| {
                let start_idx = (time.start * sample_rate) as usize;
                let end_idx = ((time.end * sample_rate) as usize).min(samples.len());
                if end_idx > start_idx
                    && (end_idx - start_idx) as f64 / sample_rate >= MIN_EMBED_SECS
                {
                    Some((time, samples[start_idx..end_idx].to_vec()))
                } else {
                    None
                }
            })
            .collect();
        if !embeddable.is_empty() {
            let embeddings = embed_unit_slices(
                self.diarizer.embedder.as_ref(),
                &embeddable.iter().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
            );
            for ((time, _), emb) in embeddable.into_iter().zip(embeddings) {
                if let Some(mut emb) = emb {
                    polyvoice::utils::l2_normalize(&mut emb);
                    mixed_overlaps.push((time, emb));
                }
            }
        }

        self.timings.embedding_secs += embed_start.elapsed().as_secs_f64();
        if units.is_empty() && !primary.is_empty() {
            warn!(
                "Chunk at {:.0}s: segmentation found {} primary segments but embedding produced 0 valid vectors (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large)",
                chunk_start_secs,
                primary.len()
            );
        }

        self.chunks.push(ChunkRecord {
            start_secs: chunk_start_secs,
            primary,
            overlaps,
            units,
            mixed_overlaps,
        });
        Ok(())
    }

    /// Global stage: cluster all accumulated units, map per-chunk local
    /// speakers onto global clusters, resegment overlaps, filter, gap-fill.
    pub(crate) fn finish(self) -> Result<
        (
            Vec<DiarizationSegment>,
            Vec<ClusteredEmbedding>,
            StageTimings,
        ),
        String,
    > {
        let mut timings = self.timings;
        let all_units: Vec<&DenseUnit> = self.chunks.iter().flat_map(|c| c.units.iter()).collect();
        if all_units.is_empty() {
            if self.had_raw_segments {
                return Err(
                    "Embedding produced zero valid vectors (audio_signal layout mismatch — expected [B,80,T] for titanet_large, but no embeddings survived; check TitaNet layout)".to_string(),
                );
            }
            return Ok((Vec::new(), Vec::new(), timings));
        }
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let raw_embeddings = self.diarizer.clusterer.wants_raw_embeddings();
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_units.len());
        let mut durations: Vec<f64> = Vec::with_capacity(all_units.len());
        for unit in &all_units {
            let mut emb = unit.embedding.clone();
            if !raw_embeddings {
                polyvoice::utils::l2_normalize(&mut emb);
            }
            durations.push(unit.time.end - unit.time.start);
            embeddings.push(emb);
        }

        let cluster_start = Instant::now();
        let labels = self
            .diarizer
            .clusterer
            .cluster_with_durations(&embeddings, &durations)
            .map_err(|e| format!("Speaker clustering failed: {}", e))?;
        timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

        let reseg_start = Instant::now();
        let (segments, clustered) = assemble_channel_turns(
            &self.chunks,
            &embeddings,
            &labels,
            &self.diarizer.resegmenter,
            self.config,
        )?;
        timings.resegmentation_secs = reseg_start.elapsed().as_secs_f64();

        info!(
            "Chunked v2 diarization found {} segments with {} unique speakers ({} embedding units)",
            segments.len(),
            count_unique_speakers(&segments),
            all_units.len()
        );
        Ok((segments, clustered, timings))
    }
}

#[cfg(test)]
mod tests {
    use polyvoice::embedder::Embedder as _;

    use super::super::cluster::build_clusterer;
    use super::super::fixtures::raw_seg;
    use super::*;

    /// A test embedder that returns a deterministic vector whose first element
    /// identifies the input slice length. This lets us verify that
    /// `embed_batch` preserves ordering and that the fallback path handles
    /// mismatched dimensions gracefully.
    struct OrderedTestEmbedder {
        dim: usize,
    }

    impl polyvoice::embedder::Embedder for OrderedTestEmbedder {
        fn dim(&self) -> usize {
            self.dim
        }

        fn embed(&self, audio: &[f32]) -> Result<Vec<f32>, polyvoice::embedder::EmbedderError> {
            let mut v = vec![0.0f32; self.dim];
            if let Some(first) = v.first_mut() {
                *first = audio.len() as f32;
            }
            Ok(v)
        }

        fn embed_batch(
            &self,
            audios: &[&[f32]],
        ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
            audios.iter().map(|a| self.embed(a)).collect()
        }
    }

    #[test]
    fn embed_batch_preserves_ordering_and_handles_errors() {
        let embedder = OrderedTestEmbedder { dim: 4 };
        let inputs: Vec<Vec<f32>> = vec![vec![1.0; 10], vec![2.0; 20], vec![3.0; 30]];
        let refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();

        let batch = embedder.embed_batch(&refs).expect("batch should succeed");
        assert_eq!(batch.len(), inputs.len());
        for (i, emb) in batch.iter().enumerate() {
            assert_eq!(
                emb[0],
                inputs[i].len() as f32,
                "embedding order mismatch at index {}",
                i
            );
        }

        // Empty batch should return an empty result, not an error.
        let empty: Vec<&[f32]> = Vec::new();
        assert!(embedder.embed_batch(&empty).unwrap().is_empty());
    }

    #[test]
    fn embed_unit_slices_batch_and_fallback_are_position_aligned() {
        let inputs: Vec<Vec<f32>> = vec![vec![0.0; 10], vec![0.0; 20], vec![0.0; 30]];
        let batch = embed_unit_slices(&EchoEmbedder { dim: 4, fail_batch: false }, &inputs);
        let fallback = embed_unit_slices(&EchoEmbedder { dim: 4, fail_batch: true }, &inputs);
        assert_eq!(batch.len(), inputs.len());
        assert_eq!(fallback.len(), inputs.len(), "fallback keeps alignment");
        for (b, f) in batch.iter().zip(fallback.iter()) {
            assert_eq!(
                b.as_ref().map(|v| v[0]),
                f.as_ref().map(|v| v[0]),
                "batch and fallback results must agree"
            );
        }
        assert_eq!(
            batch.iter().map(|o| o.as_ref().map(|v| v[0]).unwrap()).collect::<Vec<_>>(),
            vec![10.0, 20.0, 30.0],
            "input order preserved"
        );
        // Wrong-dimension batch output is dropped in place (None), not shifted.
        struct BadDim;
        impl crate::audio::embedder::SpeakerEmbedder for BadDim {
            fn embed_batch(
                &self,
                _audios: &[&[f32]],
            ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
                Ok(vec![vec![1.0; 2]; 3])
            }
            fn input_dim(&self) -> usize {
                4
            }
            fn model_tag(&self) -> &'static str {
                crate::audio::embedder::ENHANCED_MODEL_TAG
            }
            fn family_threshold(&self) -> f32 {
                crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
            }
        }
        let out = embed_unit_slices(&BadDim, &inputs);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(Option::is_none), "dimension mismatches drop in place");
    }

    #[test]
    fn expand_embed_units_dense_windowing() {
        // 12 s segment, 5 s window, 2.5 s hop -> windows [0,5],[2.5,7.5],[5,10],[7.5,12].
        let segs = vec![raw_seg(0.0, 12.0, 0), raw_seg(20.0, 23.0, 1)];
        let units = expand_embed_units(&segs, 5.0);
        assert_eq!(units.len(), 5, "4 dense windows + 1 short whole segment");
        assert_eq!((units[0].0.start, units[0].0.end), (0.0, 5.0));
        assert_eq!((units[1].0.start, units[1].0.end), (2.5, 7.5));
        assert_eq!((units[2].0.start, units[2].0.end), (5.0, 10.0));
        assert_eq!((units[3].0.start, units[3].0.end), (7.5, 12.0));
        assert_eq!((units[4].0.start, units[4].0.end), (20.0, 23.0));
        // Ordering + parent linkage.
        for w in units.windows(2) {
            assert!(w[0].0.start <= w[1].0.start, "units sorted by start");
        }
        assert_eq!(
            (units[0].2, units[3].2, units[4].2),
            (0, 0, 1),
            "segment_idx ties units to their parent"
        );
        assert_eq!(units[4].1, 1, "local speaker inherited");
        // Sparse mode: one unit per segment.
        let sparse = expand_embed_units(&segs, 0.0);
        assert_eq!(sparse.len(), 2);
    }

    #[test]
    fn per_segment_embedding_is_l2_normalized_window_mean() {
        // One 12 s primary segment split into 4 dense windows by the clusterer
        // path; the segment's ClusteredEmbedding must be the L2-normalized
        // mean of its window embeddings.
        let chunk = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 12.0, 0)],
            overlaps: Vec::new(),
            units: (0..4)
                .map(|i| DenseUnit {
                    time: polyvoice::types::TimeRange {
                        start: i as f64 * 2.5,
                        end: i as f64 * 2.5 + 5.0,
                    },
                    local_idx: 0,
                    segment_idx: 0,
                    embedding: vec![1.0, (i as f32) * 0.5],
                })
                .collect(),
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> = chunk.units.iter().map(|u| u.embedding.clone()).collect();
        let config = DiarizationConfig::default();
        let clusterer = build_clusterer(&config, 128).expect("default kind builds");
        let durations: Vec<f64> = vec![5.0; 4];
        let labels = clusterer
            .cluster_with_durations(&embeddings, &durations)
            .expect("cluster");
        let (segments, clustered) = assemble_channel_turns(
            &[chunk],
            &embeddings,
            &labels,
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        assert_eq!(clustered.len(), 1, "one per-segment aggregate");
        assert_eq!(segments.len(), 1);
        let emb = &clustered[0].embedding;
        let expected_mean = vec![1.0f32, 0.75]; // mean of [0, .5, 1, 1.5]
        let norm = expected_mean.iter().map(|x| x * x).sum::<f32>().sqrt();
        for (got, want) in emb.iter().zip(&expected_mean) {
            assert!(
                (got - want / norm).abs() < 1e-5,
                "aggregate must be the L2-normalized window mean"
            );
        }
        let unit_norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((unit_norm - 1.0).abs() < 1e-5);
    }

    /// SpeakerEmbedder double for the v2 embed path: echoes input length in
    /// every dim, batch failure is injectable to exercise the fallback.
    struct EchoEmbedder {
        dim: usize,
        fail_batch: bool,
    }

    impl crate::audio::embedder::SpeakerEmbedder for EchoEmbedder {
        fn embed_batch(
            &self,
            audios: &[&[f32]],
        ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
            if self.fail_batch {
                return Err(polyvoice::embedder::EmbedderError::Legacy(
                    "injected batch failure".to_string(),
                ));
            }
            Ok(audios
                .iter()
                .map(|a| vec![a.len() as f32; self.dim])
                .collect())
        }
        fn embed(&self, audio: &[f32]) -> Result<Vec<f32>, polyvoice::embedder::EmbedderError> {
            Ok(vec![audio.len() as f32; self.dim])
        }
        fn input_dim(&self) -> usize {
            self.dim
        }
        fn model_tag(&self) -> &'static str {
            crate::audio::embedder::ENHANCED_MODEL_TAG
        }
        fn family_threshold(&self) -> f32 {
            crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
        }
    }
}
