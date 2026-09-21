//! Persisting a session's clusters: per-cluster centroids and bounded exemplar
//! caches, plus auto-recognition against enrolled prototypes.

use super::super::identity::matching::{l2_normalize_in_place, Prototype};
use super::super::ClusteredEmbedding;
use crate::database::repositories::speaker::{Exemplar, SpeakerRepository};
use sqlx::SqlitePool;
use std::collections::HashMap;

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
