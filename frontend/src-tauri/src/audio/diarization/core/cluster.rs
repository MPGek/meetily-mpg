//! Clustering for the global stage: the speaker-count ceiling every path
//! shares, the offline clusterer factory, and the `Clustering` seam the batch
//! pass and a live session's stop-time pass both construct through.

use log::warn;

use super::super::config::MAX_CLUSTERERS;
use super::super::{ClustererKindSetting, DiarizationConfig};

/// Build the offline clusterer for the resolved kind. The effective ceiling is
/// clamped to the pipeline's 255 maximum (logged). `vbx` is gated: the vendored
/// PLDA parameters require 256-d embeddings while the enhanced TitaNet-Large
/// family is 192-d, so selecting it errors actionably — no silent kind switch.
pub(crate) fn build_clusterer(
    config: &DiarizationConfig,
    max_clusters: usize,
) -> Result<Box<dyn polyvoice::clusterer::Clusterer>, String> {
    let ceiling = max_clusters.min(MAX_CLUSTERERS);
    if ceiling < max_clusters {
        warn!(
            "Speaker-count ceiling {} exceeds the clustering backend maximum; clamped to {}",
            max_clusters, ceiling
        );
    }
    match config.clusterer {
        ClustererKindSetting::Ahc => {
            log::info!(
                "Diarization clusterer: ahc (threshold={:.3}, ceiling={})",
                config.cluster_threshold,
                ceiling
            );
            Ok(Box::new(polyvoice::clusterer::AhcClusterer::with_threshold(
                ceiling,
                config.cluster_threshold,
            )))
        }
        ClustererKindSetting::Nmesc => {
            log::info!(
                "Diarization clusterer: nmesc (automatic count, ceiling={}; merge threshold inert)",
                ceiling
            );
            Ok(Box::new(polyvoice::clusterer::NmeScClusterer::new(ceiling)))
        }
        ClustererKindSetting::Vbx => Err(format!(
            "The 'vbx' clusterer is unavailable for the enhanced diarization model set: \
             VBx requires 256-dimensional embeddings (the vendored PLDA parameters are \
              dimension-locked), while the bundled TitaNet-Large family embeds 192-d. \
              Select 'ahc' (default) or 'nmesc' instead."
        )),
    }
}

/// Always-on ceiling rule shared by every path (05 D3): the user maximum when
/// it is smaller, otherwise the configured default ceiling; never below 1, so
/// the clusterer can never run unbounded, and never above the clustering
/// backend's 255 maximum.
pub(crate) fn effective_cluster_ceiling(
    config: &DiarizationConfig,
    max_speakers: Option<i32>,
) -> usize {
    let user_max = max_speakers.filter(|m| *m > 0).unwrap_or(i32::MAX) as usize;
    let requested = user_max.min(config.cluster_ceiling).max(1);
    let ceiling = requested.min(MAX_CLUSTERERS);
    if ceiling < requested {
        warn!(
            "Speaker-count ceiling {} exceeds the clustering backend maximum; clamped to {}",
            requested, ceiling
        );
    }
    ceiling
}

/// The clustering seam (05 D3). One `cluster` entry point for every pass, so a
/// deferred/incremental implementation can be added behind it without
/// touching callers.
pub trait Clustering {
    fn cluster(&self, embeddings: &[Vec<f32>]) -> Result<Vec<usize>, String>;
}

/// Global agglomerative clustering exactly as the batch pass runs it.
pub struct GlobalAhc {
    pub(crate) inner: Box<dyn polyvoice::clusterer::Clusterer>,
}

impl Clustering for GlobalAhc {
    fn cluster(&self, embeddings: &[Vec<f32>]) -> Result<Vec<usize>, String> {
        self.inner.cluster(embeddings).map_err(|e| e.to_string())
    }
}

/// Buffered clustering for the live Efficient path: the same kind, threshold
/// and ceiling the batch pass resolves, wrapped in the singleton dissolution
/// the `online-speaker-diarization` capability requires.
pub struct BufferedAhc {
    inner: polyvoice::clusterer::MinClusterSizeClusterer,
}

impl Clustering for BufferedAhc {
    fn cluster(&self, embeddings: &[Vec<f32>]) -> Result<Vec<usize>, String> {
        use polyvoice::clusterer::Clusterer as _;
        self.inner.cluster(embeddings).map_err(|e| e.to_string())
    }
}

/// Batch/global clusterer for a resolved config and effective ceiling.
pub(crate) fn clusterer_for_batch(
    config: &DiarizationConfig,
    ceiling: usize,
) -> Result<GlobalAhc, String> {
    Ok(GlobalAhc {
        inner: build_clusterer(config, ceiling)?,
    })
}

/// Buffered clusterer for the live Efficient path, from the same factory.
pub(crate) fn clusterer_for_buffer(
    config: &DiarizationConfig,
    ceiling: usize,
) -> Result<BufferedAhc, String> {
    Ok(BufferedAhc {
        inner: polyvoice::clusterer::MinClusterSizeClusterer::new(
            build_clusterer(config, ceiling)?,
            2,
        ),
    })
}

#[derive(Debug, Clone)]
pub(crate) struct SpeakerSegment {
    pub(crate) start: f32,
    pub(crate) end: f32,
    pub(crate) speaker: usize,
}

/// (start_time, end_time, embedding_vector) buffer per channel (Efficient mode).
#[derive(Default)]
pub(crate) struct EmbeddingBuffer {
    pub(crate) entries: Vec<(f32, f32, Vec<f32>)>,
}

impl EmbeddingBuffer {
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn push(&mut self, start: f32, end: f32, embedding: Vec<f32>) {
        self.entries.push((start, end, embedding));
    }

    /// Clusters the buffered embeddings through the shared `Clustering` seam
    /// (05 D3/4.2): same kind, threshold and ceiling the batch pass resolves,
    /// still wrapped in the singleton dissolution this capability requires.
    pub(crate) fn cluster(
        &self,
        config: &crate::audio::diarization::DiarizationConfig,
        ceiling: usize,
    ) -> Vec<SpeakerSegment> {
        use crate::audio::diarization::Clustering as _;

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
        let clusterer = match crate::audio::diarization::clusterer_for_buffer(config, ceiling) {
            Ok(c) => c,
            Err(e) => {
                warn!(
                    "Clusterer construction failed ({}), treating all segments as one speaker",
                    e
                );
                return self
                    .entries
                    .iter()
                    .map(|e| SpeakerSegment {
                        start: e.0,
                        end: e.1,
                        speaker: 0,
                    })
                    .collect();
            }
        };

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
                warn!(
                    "AhcClusterer failed ({}), treating all segments as one speaker",
                    e
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_ceiling_is_user_max_when_smaller_and_always_positive() {
        // The one shared rule, used by the batch path and (since 05 section 3)
        // the live path too — no second copy of the computation here.
        let ceiling = |max_speakers: Option<i32>, config: &DiarizationConfig| -> usize {
            effective_cluster_ceiling(config, max_speakers)
        };
        let cfg = DiarizationConfig::default();
        // No user max -> default ceiling.
        assert_eq!(ceiling(None, &cfg), 128);
        assert_eq!(ceiling(Some(0), &cfg), 128);
        assert_eq!(ceiling(Some(-1), &cfg), 128);
        // User max below the ceiling wins.
        assert_eq!(ceiling(Some(5), &cfg), 5);
        // User max above the ceiling is capped by the configured ceiling.
        assert_eq!(ceiling(Some(200), &cfg), 128);
        // Oversized stored ceiling is clamped to the backend maximum.
        let stored = DiarizationConfig {
            cluster_ceiling: 300,
            ..DiarizationConfig::default()
        };
        assert_eq!(ceiling(None, &stored), 255);
        assert_eq!(ceiling(Some(999), &stored), 255);
        // Never zero (unbounded stop unreachable).
        assert_eq!(ceiling(Some(1), &cfg), 1);
    }

    #[test]
    fn vbx_kind_fails_actionably_on_the_192d_enhanced_family() {
        // Gate (revised D2): the vendored PLDA params require 256-d
        // embeddings; selecting vbx must error with a clear message and never
        // silently switch kinds.
        let config = DiarizationConfig {
            clusterer: ClustererKindSetting::Vbx,
            ..DiarizationConfig::default()
        };
        let err = match build_clusterer(&config, 128) {
            Ok(_) => panic!("vbx must not build for the 192-d enhanced family"),
            Err(e) => e,
        };
        assert!(err.contains("256-dimensional"), "error names the dim requirement: {err}");
        assert!(err.contains("192-d"), "error names the active family: {err}");
        assert!(err.contains("ahc"), "error suggests the default kind: {err}");
    }

    #[test]
    fn clusterer_factory_enforces_clamped_ceiling() {
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Ahc,
            ..DiarizationConfig::default()
        };
        let c = build_clusterer(&cfg, 300).expect("ahc builds");
        assert_eq!(c.max_clusters(), 255, "clamped to the backend maximum");
        let c = build_clusterer(&cfg, 12).expect("ahc builds");
        assert_eq!(c.max_clusters(), 12);
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Nmesc,
            ..DiarizationConfig::default()
        };
        let c = build_clusterer(&cfg, 300).expect("nmesc builds");
        assert_eq!(c.max_clusters(), 255);
    }

    #[test]
    fn merge_threshold_is_inert_under_automatic_count() {
        // Same embeddings, two stored thresholds, kind nmesc: identical labels
        // (the threshold only decorates the ahc kind).
        let embeddings: Vec<Vec<f32>> = (0..8)
            .map(|i| {
                let mut v = vec![0.0f32; 6];
                v[i % 2] = 1.0;
                v[(i % 2) + 2] = 0.3;
                v
            })
            .collect();
        let mut labels_by_threshold = Vec::new();
        for threshold in [0.1f32, 0.9] {
            let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Ahc,
                cluster_threshold: threshold,
                ..DiarizationConfig::default()
            };
            let clusterer = build_clusterer(&cfg, 128).expect("nmesc builds");
            labels_by_threshold.push(clusterer.cluster(&embeddings).expect("cluster"));
        }
        assert_eq!(
            labels_by_threshold[0], labels_by_threshold[1],
            "stored merge threshold must not change nmesc results"
        );
    }

    #[test]
    fn fragmented_long_recording_is_capped_by_the_ceiling() {
        // Singleton pruning is gone (3.6); the always-enforced ceiling still
        // bounds the label count on a fragmented embedding set.
        let embeddings: Vec<Vec<f32>> = (0..40)
            .map(|i| {
                let mut v = vec![0.0f32; 16];
                v[i % 16] = 1.0;
                v[(i / 16) + 8] = 0.2;
                polyvoice::utils::l2_normalize(&mut v);
                v
            })
            .collect();
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Nmesc,
            ..DiarizationConfig::default()
        };
        let clusterer = build_clusterer(&cfg, 4).expect("nmesc builds");
        let labels = clusterer.cluster(&embeddings).expect("cluster");
        let distinct: std::collections::HashSet<_> = labels.iter().collect();
        assert!(
            distinct.len() <= 4,
            "ceiling must bound distinct labels, got {}",
            distinct.len()
        );
    }

    #[test]
    fn clusterer_kind_parses_and_resolves_without_rebuild() {
        assert_eq!(ClustererKindSetting::parse("vbx"), Some(ClustererKindSetting::Vbx));
        assert_eq!(ClustererKindSetting::parse("NMEsc"), Some(ClustererKindSetting::Nmesc));
        assert_eq!(ClustererKindSetting::parse(" ahc "), Some(ClustererKindSetting::Ahc));
        assert_eq!(ClustererKindSetting::parse("kmeans"), None);
        assert_eq!(ClustererKindSetting::Nmesc.as_str(), "nmesc");
        assert!(ClustererKindSetting::Nmesc.is_automatic_count());
        assert!(ClustererKindSetting::Vbx.is_automatic_count());
        assert!(!ClustererKindSetting::Ahc.is_automatic_count());
    }

    fn emb(v: &[f32]) -> Vec<f32> {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / norm).collect()
    }

    fn buffer_of(vectors: &[Vec<f32>]) -> EmbeddingBuffer {
        let mut b = EmbeddingBuffer::default();
        for (i, v) in vectors.iter().enumerate() {
            b.push(i as f32, i as f32 + 1.0, v.clone());
        }
        b
    }

    /// 05 task 4.2: `EmbeddingBuffer::cluster` now runs through the shared
    /// `Clustering` seam. The singleton dissolution (`MinClusterSizeClusterer`
    /// with min size 2) that `online-speaker-diarization` requires must be
    /// preserved, and a two-embedding fixture must still yield one cluster.
    #[test]
    fn buffered_clustering_keeps_singleton_dissolution() {
        let cfg = crate::audio::diarization::DiarizationConfig::default();
        let ceiling = crate::audio::diarization::effective_cluster_ceiling(&cfg, None);

        // Two embeddings: min-cluster-size 2 cannot leave two singletons, so
        // they collapse into a single cluster.
        let two = buffer_of(&[emb(&[1.0, 0.0, 0.0]), emb(&[0.0, 1.0, 0.0])]);
        let segs = two.cluster(&cfg, ceiling);
        assert_eq!(segs.len(), 2, "one segment per buffered entry");
        let labels: std::collections::HashSet<usize> = segs.iter().map(|s| s.speaker).collect();
        assert_eq!(labels.len(), 1, "two entries cannot remain two singletons");

        // Two tight pairs plus one outlier: the outlier is a singleton and must
        // be reassigned into a larger cluster rather than surviving alone.
        let five = buffer_of(&[
            emb(&[1.0, 0.0, 0.0]),
            emb(&[0.99, 0.01, 0.0]),
            emb(&[0.0, 1.0, 0.0]),
            emb(&[0.01, 0.99, 0.0]),
            emb(&[0.0, 0.0, 1.0]),
        ]);
        let segs = five.cluster(&cfg, ceiling);
        assert_eq!(segs.len(), 5);
        let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for s in &segs {
            *counts.entry(s.speaker).or_default() += 1;
        }
        assert!(
            counts.values().all(|c| *c >= 2),
            "no singleton cluster survives dissolution: {counts:?}"
        );
    }

    /// 05 task 3.3/3.4 default equivalence: enforcing the ceiling changes the
    /// value the clusterer receives for an unset user maximum (0 -> the
    /// configured default), but with no stored overrides the clustering result
    /// must be identical to the previous unbounded call.
    #[test]
    fn enforced_ceiling_does_not_change_default_clustering_result() {
        let cfg = crate::audio::diarization::DiarizationConfig::default();
        let vectors = vec![
            emb(&[1.0, 0.0, 0.0]),
            emb(&[0.98, 0.02, 0.0]),
            emb(&[0.0, 1.0, 0.0]),
            emb(&[0.02, 0.98, 0.0]),
            emb(&[0.0, 0.1, 1.0]),
            emb(&[0.0, 0.0, 0.99]),
        ];
        let buffer = buffer_of(&vectors);

        // Previous live behaviour: an unset user maximum reached the clusterer
        // as 0 (unbounded).
        let before = buffer.cluster(&cfg, 0);
        // New behaviour: the shared rule resolves it to the configured ceiling.
        let ceiling = crate::audio::diarization::effective_cluster_ceiling(&cfg, None);
        assert_eq!(ceiling, cfg.cluster_ceiling);
        let after = buffer.cluster(&cfg, ceiling);

        let labels = |segs: &[SpeakerSegment]| -> Vec<usize> {
            segs.iter().map(|s| s.speaker).collect()
        };
        assert_eq!(
            labels(&before),
            labels(&after),
            "with no stored overrides the enforced ceiling is inert"
        );
    }
}
