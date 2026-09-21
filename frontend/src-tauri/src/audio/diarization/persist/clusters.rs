//! Persisting a session's clusters: per-cluster centroids and bounded exemplar
//! caches, plus auto-recognition against enrolled prototypes.

use super::super::identity::matching::{l2_normalize_in_place, Prototype};
use super::super::core::cluster::SpeakerSegment;
use super::super::core::timeline::find_best_speaker;
use super::super::ClusteredEmbedding;
use crate::database::repositories::speaker::{Exemplar, SpeakerRepository};
use sqlx::SqlitePool;
use std::collections::HashMap;
/// Where a channel's buffered chunk embeddings get their cluster label from.
/// The two live modes differ only here, so they share one builder (05 task
/// 2.2, which replaced `cluster_embeddings_by_labels`/`_by_overlap`).
pub(crate) enum EmbeddingLabeling<'a> {
    /// Efficient mode: clustering returned one label per buffered entry,
    /// aligned with `entries` by position. Entries past the end of the label
    /// list are dropped (the old `zip` truncation).
    ByPosition(&'a [SpeakerSegment]),
    /// Fast mode: the entries carry no label of their own, so each is
    /// attributed to the published turn it overlaps, falling back to the
    /// nearest turn within the gap bound and dropped when none is near enough.
    ByOverlap(&'a [SpeakerSegment]),
}

/// Build the persistable cluster embeddings for one channel from its buffered
/// `(start, end, embedding)` chunks.
pub(crate) fn clustered_embeddings(
    entries: &[(f32, f32, Vec<f32>)],
    labeling: EmbeddingLabeling<'_>,
) -> Vec<ClusteredEmbedding> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(idx, (start, end, embedding))| {
            let speaker = match labeling {
                EmbeddingLabeling::ByPosition(segments) => {
                    segments.get(idx).map(|seg| seg.speaker as i32)
                }
                EmbeddingLabeling::ByOverlap(segments) => {
                    find_best_speaker(segments, *start, *end).map(|spk| spk as i32)
                }
            }?;
            Some(ClusteredEmbedding {
                speaker,
                embedding: embedding.clone(),
                duration_secs: (end - start).max(0.0),
                start_secs: Some(*start),
                end_secs: Some(*end),
            })
        })
        .collect()
}


/// Group a channel's clustered embeddings by cluster id, computing the
/// L2-normalized centroid (mean of member embeddings) and a bounded set of
/// exemplar embeddings (top by duration) for each cluster.
fn group_cluster_embeddings(
    embeddings: &[ClusteredEmbedding],
) -> Vec<(i32, Vec<f32>, Vec<Exemplar>)> {
    let mut by_cluster: HashMap<i32, Vec<&ClusteredEmbedding>> = HashMap::new();
    for e in embeddings {
        by_cluster.entry(e.speaker).or_default().push(e);
    }

    let mut out = Vec::new();
    for (spk, items) in by_cluster {
        let dim = items.first().map(|e| e.embedding.len()).unwrap_or(0);
        let mut centroid = vec![0.0f32; dim];
        for it in &items {
            for (i, x) in it.embedding.iter().enumerate() {
                centroid[i] += x;
            }
        }
        let n = items.len().max(1) as f32;
        for c in centroid.iter_mut() {
            *c /= n;
        }
        l2_normalize_in_place(&mut centroid);

        let mut sorted: Vec<&ClusteredEmbedding> = items.clone();
        sorted.sort_by(|a, b| {
            b.duration_secs
                .partial_cmp(&a.duration_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let exemplars: Vec<Exemplar> = sorted
            .iter()
            .take(MAX_CLUSTER_CACHE_EXEMPLARS)
            .map(|e| Exemplar {
                embedding: e.embedding.clone(),
                duration_secs: e.duration_secs as f64,
                start_secs: e.start_secs,
                end_secs: e.end_secs,
            })
            .collect();

        out.push((spk, centroid, exemplars));
    }
    out
}

/// Persist each cluster's centroid + exemplar cache for one channel, then
/// auto-assign recognized speakers from the provided prototypes. User
/// bindings are preserved. `prototypes` is pre-loaded by the caller. All
/// rows and recognition use the enhanced `titanet_large` family.
async fn persist_channel_clusters(
    pool: &SqlitePool,
    meeting_id: &str,
    embeddings: &[ClusteredEmbedding],
    prefix: &str,
    channel: &str,
    prototypes: &[Prototype],
) -> Result<(), String> {
    let clusters = group_cluster_embeddings(embeddings);
    for (spk, centroid, exemplars) in clusters {
        let label = format!("{}_{:02}", prefix, spk);
        SpeakerRepository::write_cluster_cache(
            pool,
            meeting_id,
            &label,
            channel,
            &centroid,
            &exemplars,
            crate::audio::embedder::ENHANCED_MODEL_TAG,
        )
        .await
        .map_err(|e| format!("Failed to persist cluster cache: {}", e))?;

        let threshold = crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD;
        if let Some(m) = crate::audio::speaker_recognition::best_match_with_threshold(
            &centroid,
            Some(channel),
            prototypes,
            threshold,
        ) {
            SpeakerRepository::set_auto_binding_if_unbound(
                pool,
                meeting_id,
                &label,
                &m.speaker_id,
                m.score as f64,
            )
            .await
            .map_err(|e| format!("Failed to auto-assign speaker: {}", e))?;
        }
    }
    Ok(())
}

/// Persist per-cluster centroids + exemplar caches for both channels and
/// auto-assign recognized speakers. The expected-speaker allowlist (or all
/// speakers when empty) constrains candidates; prototypes are loaded once
/// for the enhanced `titanet_large` family. Used by both the offline
/// diarization path and the online recording stop-time finalize.
pub async fn persist_and_recognize_session(
    pool: &SqlitePool,
    meeting_id: &str,
    mic: &[ClusteredEmbedding],
    sys: &[ClusteredEmbedding],
    is_stereo: bool,
) -> Result<(), String> {
    if mic.is_empty() && sys.is_empty() {
        return Ok(());
    }

    let expected = SpeakerRepository::get_expected_speakers(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load expected speakers: {}", e))?;
    let candidates: Option<&[String]> = if expected.is_empty() {
        None
    } else {
        Some(&expected)
    };
    let prototypes: Vec<Prototype> = SpeakerRepository::load_prototypes(
        pool,
        candidates,
        crate::audio::embedder::ENHANCED_MODEL_TAG,
    )
    .await
    .map_err(|e| format!("Failed to load prototypes: {}", e))?
    .into_iter()
    .map(Prototype::from)
    .collect();

    let mic_prefix = if is_stereo { "MIC_SPEAKER" } else { "SPEAKER" };
    persist_channel_clusters(pool, meeting_id, mic, mic_prefix, "mic", &prototypes).await?;
    if is_stereo {
        persist_channel_clusters(pool, meeting_id, sys, "SPEAKER", "system", &prototypes).await?;
    }
    Ok(())
}

/// Maximum exemplar cache rows persisted per cluster (top by duration).
/// Enrollment later reparents the best-K=8 of these; the cache is bounded so
/// storage grows with the number of clusters, not segments.
pub(crate) const MAX_CLUSTER_CACHE_EXEMPLARS: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;

    /// Four buffered chunks against three turns. The last two chunks are what
    /// separates the two labelings: position 2 maps the 40 s chunk onto turn 2,
    /// while overlap drops it (34 s past the last turn, outside the gap bound)
    /// and attributes the 5-6 s chunk instead. The fourth chunk has no label
    /// under either rule.
    fn fixture() -> (Vec<(f32, f32, Vec<f32>)>, Vec<SpeakerSegment>) {
        (
            vec![
                (0.0, 2.0, vec![1.0, 0.0]),
                (2.0, 5.0, vec![0.9, 0.1]),
                (40.0, 41.0, vec![0.5, 0.5]),
                (5.0, 6.0, vec![0.0, 1.0]),
            ],
            vec![
                SpeakerSegment { start: 0.0, end: 2.0, speaker: 0 },
                SpeakerSegment { start: 2.0, end: 5.0, speaker: 1 },
                SpeakerSegment { start: 5.0, end: 6.0, speaker: 0 },
            ],
        )
    }

    fn shape(v: &[ClusteredEmbedding]) -> Vec<(i32, f32, Option<f32>, Option<f32>)> {
        v.iter()
            .map(|c| (c.speaker, c.duration_secs, c.start_secs, c.end_secs))
            .collect()
    }

    /// Both live modes had their own labeler before task 2.2 replaced them with
    /// `clustered_embeddings`. These are the exact outputs the deleted
    /// `cluster_embeddings_by_labels`/`_by_overlap` produced for this fixture,
    /// captured from them before the deletion.
    #[test]
    fn one_builder_reproduces_both_pre_dedup_labelings() {
        let (entries, segments) = fixture();

        let by_position = clustered_embeddings(&entries, EmbeddingLabeling::ByPosition(&segments));
        assert_eq!(
            shape(&by_position),
            vec![
                (0, 2.0, Some(0.0), Some(2.0)),
                (1, 3.0, Some(2.0), Some(5.0)),
                (0, 1.0, Some(40.0), Some(41.0)),
            ]
        );

        let by_overlap = clustered_embeddings(&entries, EmbeddingLabeling::ByOverlap(&segments));
        assert_eq!(
            shape(&by_overlap),
            vec![
                (0, 2.0, Some(0.0), Some(2.0)),
                (1, 3.0, Some(2.0), Some(5.0)),
                (0, 1.0, Some(5.0), Some(6.0)),
            ]
        );
    }

    /// The grouping the persistence step does over that output: same cluster
    /// count and same per-cluster durations for either labeling.
    #[test]
    fn grouping_is_unchanged_for_both_labelings() {
        let (entries, segments) = fixture();
        for labeling in [
            EmbeddingLabeling::ByPosition(&segments),
            EmbeddingLabeling::ByOverlap(&segments),
        ] {
            let embeddings = clustered_embeddings(&entries, labeling);
            let mut grouped = group_cluster_embeddings(&embeddings);
            grouped.sort_by_key(|(spk, _, _)| *spk);

            assert_eq!(grouped.len(), 2, "two clusters");
            let durations: Vec<(i32, f64, usize)> = grouped
                .iter()
                .map(|(spk, _, exemplars)| {
                    (
                        *spk,
                        exemplars.iter().map(|e| e.duration_secs).sum::<f64>(),
                        exemplars.len(),
                    )
                })
                .collect();
            assert_eq!(durations, vec![(0, 3.0, 2), (1, 3.0, 1)]);
        }
    }
}
