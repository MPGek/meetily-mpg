//! Persisting a session's clusters: per-cluster centroids and bounded exemplar
//! caches, plus auto-recognition against enrolled prototypes.

use super::super::identity::matching::{
    best_match_with_threshold, l2_normalize_in_place, MatchResult, Prototype,
};
use super::super::core::cluster::SpeakerSegment;
use super::super::core::timeline::find_best_speaker;
use super::super::ClusteredEmbedding;
use log::{info, warn};

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

    let prototypes = load_candidate_prototypes(pool, meeting_id).await?;

    let mic_prefix = if is_stereo { "MIC_SPEAKER" } else { "SPEAKER" };
    persist_channel_clusters(pool, meeting_id, mic, mic_prefix, "mic", &prototypes).await?;
    if is_stereo {
        persist_channel_clusters(pool, meeting_id, sys, "SPEAKER", "system", &prototypes).await?;
    }
    Ok(())
}

/// The prototypes a meeting's recognition may match against: the meeting's
/// expected speakers, or every speaker when no allowlist was set. Shared by
/// the per-cluster and the per-row pass so a row and its cluster are always
/// judged against the same candidates.
async fn load_candidate_prototypes(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<Vec<Prototype>, String> {
    let expected = SpeakerRepository::get_expected_speakers(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load expected speakers: {}", e))?;
    let candidates: Option<&[String]> = if expected.is_empty() {
        None
    } else {
        Some(&expected)
    };
    Ok(SpeakerRepository::load_prototypes(
        pool,
        candidates,
        crate::audio::embedder::ENHANCED_MODEL_TAG,
    )
    .await
    .map_err(|e| format!("Failed to load prototypes: {}", e))?
    .into_iter()
    .map(Prototype::from)
    .collect())
}

/// Name each transcript row of a live session from the embeddings that belong
/// to that row, not from its cluster's centroid
/// (per-row-speaker-recognition).
///
/// A live session's clustering can merge several people into one cluster, and
/// one match per cluster then renames every row it covers. This pass matches
/// each row against the same candidate prototypes, using only the session
/// embeddings whose window overlaps the row and whose channel is the row's
/// own, at the same threshold. Display resolution prefers the result over the
/// cluster binding, and never over a user decision.
///
/// Rows with no overlapping embedding, and rows whose best candidate stays
/// below the threshold, are left without a row-level match so they keep
/// resolving through their cluster. Nothing else about the row, the cluster or
/// its caches is touched. Returns how many rows were given a match.
pub(crate) async fn recognize_transcript_rows(
    pool: &SqlitePool,
    meeting_id: &str,
    mic_embeddings: &[(f32, f32, Vec<f32>)],
    sys_embeddings: &[(f32, f32, Vec<f32>)],
    saw_system_audio: bool,
) -> Result<usize, String> {
    if mic_embeddings.is_empty() && sys_embeddings.is_empty() {
        return Ok(0);
    }
    let prototypes = load_candidate_prototypes(pool, meeting_id).await?;
    if prototypes.is_empty() {
        return Ok(0);
    }

    let rows = SpeakerRepository::list_transcript_windows(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to list transcript windows: {}", e))?;
    let threshold = crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD;
    let mut matched = 0usize;

    for (id, start, end, source_device) in rows {
        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        // Same channel rule the stop-time assignment uses: a system row only
        // exists when the session captured system audio.
        let is_system = saw_system_audio && source_device.as_deref() == Some("System");
        let (embeddings, channel) = if is_system {
            (sys_embeddings, "system")
        } else {
            (mic_embeddings, "mic")
        };

        let mut best: Option<MatchResult> = None;
        for (emb_start, emb_end, embedding) in embeddings {
            let overlap = end.min(*emb_end as f64) - start.max(*emb_start as f64);
            if overlap <= 0.0 {
                continue;
            }
            if let Some(candidate) =
                best_match_with_threshold(embedding, Some(channel), &prototypes, threshold)
            {
                let better = match &best {
                    Some(previous) => candidate.score > previous.score,
                    None => true,
                };
                if better {
                    best = Some(candidate);
                }
            }
        }

        if let Some(m) = best {
            if SpeakerRepository::set_transcript_auto_match(
                pool,
                &id,
                &m.speaker_id,
                m.score as f64,
            )
            .await
            .map_err(|e| format!("Failed to record the row speaker match: {}", e))?
            {
                matched += 1;
            }
        }
    }

    if matched > 0 {
        info!(
            "Row-level recognition named {} of the meeting's transcript rows from their own audio",
            matched
        );
    }
    Ok(matched)
}

/// Refresh a meeting's row-level automatic matches from its persisted
/// exemplar cache, under whatever candidate set applies now
/// (per-row-speaker-recognition, design D1).
///
/// Every existing row-level match is cleared first, so this is a refresh and
/// not an accumulation: a row the current candidates no longer support goes
/// back to resolving through its cluster, and a cluster binding that just
/// changed can never be outranked by a stale row name. Reads only cached
/// embeddings, never audio. Returns (cleared, recomputed).
pub(crate) async fn refresh_transcript_row_matches(
    pool: &SqlitePool,
    meeting_id: &str,
) -> Result<(u64, usize), String> {
    let cleared = SpeakerRepository::clear_meeting_auto_matches(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to clear row speaker matches: {}", e))?;
    let cached = SpeakerRepository::list_meeting_cached_embeddings(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load cached embeddings: {}", e))?;
    let (sys_cached, mic_cached): (Vec<_>, Vec<_>) = cached
        .into_iter()
        .partition(|(_, _, _, channel)| channel == "system");
    let strip = |v: Vec<(f32, f32, Vec<f32>, String)>| -> Vec<(f32, f32, Vec<f32>)> {
        v.into_iter()
            .map(|(start, end, embedding, _)| (start, end, embedding))
            .collect()
    };
    let saw_system_audio = !sys_cached.is_empty();
    let recomputed = recognize_transcript_rows(
        pool,
        meeting_id,
        &strip(mic_cached),
        &strip(sys_cached),
        saw_system_audio,
    )
    .await?;
    Ok((cleared, recomputed))
}

/// Name a freshly diarized meeting's rows from its persisted embeddings
/// (offline-per-row-recognition).
///
/// The offline pass names a cluster from one match of its centroid, which fails
/// twice over: a cluster that holds two people is named after whichever of them
/// the mean is nearer, and a mean vector scores lower than the vectors it
/// averages, so a cluster of a recognizable person can stay under the threshold
/// and anonymous. This runs the row-level refresh the re-match operation runs,
/// so a row gets the candidate its own audio matches, and the offline pass and
/// a later re-match agree.
///
/// The embeddings it reads are the meeting's persisted ones: the offline run's
/// exemplars and, for a meeting recorded with live diarization, the live
/// session's chunk embeddings that are still stored. The latter are what the
/// registry's voiceprints are made of, and on the user's own assignments they
/// name rows far better than the cluster centroid does (see the change's
/// design). The refresh clears the meeting's earlier row-level matches first, so
/// a name a live session or an earlier run left cannot outrank this run's result.
///
/// Best-effort: an error is logged and swallowed. Row names improve on the
/// cluster names, they are not a precondition of the diarization, and by this
/// point its rows have already been rewritten, so failing the run here would
/// leave the user worse off than the cluster bindings do.
pub(crate) async fn name_rows_after_offline_pass(pool: &SqlitePool, meeting_id: &str) {
    match refresh_transcript_row_matches(pool, meeting_id).await {
        Ok((_cleared, named)) => info!(
            "Row-level recognition after the offline pass named {} row(s) of {} from their own embeddings",
            named, meeting_id
        ),
        Err(e) => warn!(
            "Row-level recognition after the offline pass failed for {}; its rows keep resolving through their clusters: {}",
            meeting_id, e
        ),
    }
}

/// Maximum exemplar cache rows persisted per cluster (top by duration).
/// Enrollment later reparents the best-K=8 of these; the cache is bounded so
/// storage grows with the number of clusters, not segments.
pub(crate) const MAX_CLUSTER_CACHE_EXEMPLARS: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::models::embedding_to_bytes;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("run migrations");
        pool
    }

    /// A 192-d embedding pointing at one axis, so two speakers' prototypes are
    /// orthogonal and a query matches exactly one of them.
    fn axis(index: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 192];
        v[index] = 1.0;
        v
    }

    async fn enroll(pool: &SqlitePool, speaker_id: &str, name: &str, channel: &str, emb: &[f32]) {
        sqlx::query("INSERT INTO speakers (id, name, created_at, updated_at) VALUES (?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .bind(speaker_id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, created_at)
             VALUES (?, ?, 'titanet_large', ?, 2.0, ?, '2026-01-01T00:00:00Z')",
        )
        .bind(format!("proto-{speaker_id}"))
        .bind(embedding_to_bytes(emb))
        .bind(channel)
        .bind(speaker_id)
        .execute(pool)
        .await
        .unwrap();
    }

    /// per-row-speaker-recognition 2.1: a row whose own audio identifies
    /// somebody else keeps that identity, and the cluster it belongs to is
    /// left exactly as recognition left it.
    #[tokio::test]
    async fn row_is_named_from_its_own_audio_without_touching_the_cluster() {
        let pool = setup_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        enroll(&pool, "spk-greg", "Greg", "system", &axis(0)).await;
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;

        // Two rows of the same merged cluster: one covered by Alex's voice,
        // one with no embedding of its own.
        for (id, start, end) in [("t-alex", 10.0, 12.0), ("t-uncovered", 40.0, 41.0)] {
            sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, source_device)
                 VALUES (?, 'm1', 'text', '2026-01-01T00:00:00Z', 'SPEAKER_00', ?, ?, 'System')",
            )
            .bind(id)
            .bind(start)
            .bind(end)
            .execute(&pool)
            .await
            .unwrap();
        }
        // The cluster was auto-bound to Greg, as a merged cluster's centroid would be.
        sqlx::query("INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, channel, matched_by, match_score) VALUES ('m1', 'SPEAKER_00', 'spk-greg', 'system', 'auto', 0.86)")
            .execute(&pool)
            .await
            .unwrap();

        let named = recognize_transcript_rows(&pool, "m1", &[], &[(10.5, 11.5, axis(1))], true)
            .await
            .expect("row recognition runs");
        assert_eq!(named, 1, "only the covered row can be named");

        let (auto_id, score): (Option<String>, Option<f64>) = sqlx::query_as(
            "SELECT speaker_auto_id, speaker_auto_score FROM transcripts WHERE id = 't-alex'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(auto_id.as_deref(), Some("spk-alex"));
        assert!(score.unwrap() > 0.9, "an exact prototype match scores high");

        let uncovered: Option<String> =
            sqlx::query_scalar("SELECT speaker_auto_id FROM transcripts WHERE id = 't-uncovered'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(uncovered, None, "a row with no overlapping embedding keeps resolving via its cluster");

        let cluster: (String, String, f64) = sqlx::query_as(
            "SELECT speaker_id, matched_by, match_score FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = 'SPEAKER_00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            cluster,
            ("spk-greg".to_string(), "auto".to_string(), 0.86),
            "the cluster binding must be untouched by the row-level pass"
        );
    }

    /// A row is named only from its own channel's evidence: a system row must
    /// not be named by a microphone-channel embedding, which is how the
    /// stop-time assignment keeps the two voices apart. (Prototype *channel*
    /// is a preference in the shared matcher, not a filter; what this pass
    /// controls is which side's embeddings a row may be matched against.)
    #[tokio::test]
    async fn row_is_named_only_from_its_own_channel_evidence() {
        let pool = setup_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, source_device)
             VALUES ('t1', 'm1', 'text', '2026-01-01T00:00:00Z', 'SPEAKER_00', 10.0, 12.0, 'System')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // The overlapping embedding sits on the microphone side, so this
        // system row has no evidence of its own.
        let named = recognize_transcript_rows(&pool, "m1", &[(10.5, 11.5, axis(1))], &[], true)
            .await
            .unwrap();
        assert_eq!(named, 0, "a system row must not be named from mic audio");
        let auto_id: Option<String> =
            sqlx::query_scalar("SELECT speaker_auto_id FROM transcripts WHERE id = 't1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(auto_id, None);

        // The same embedding on the system side does name it.
        let named = recognize_transcript_rows(&pool, "m1", &[], &[(10.5, 11.5, axis(1))], true)
            .await
            .unwrap();
        assert_eq!(named, 1);
    }

    /// per-row-speaker-recognition 4.1: a re-match refreshes the row-level
    /// names from the cached exemplars under the candidates that apply now,
    /// drops the ones those candidates no longer support, and leaves every
    /// user decision alone.
    #[tokio::test]
    async fn rematch_refresh_clears_unsupported_rows_and_spares_user_decisions() {
        let pool = setup_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        enroll(&pool, "spk-bob", "Bob", "system", &axis(2)).await;

        // Three rows of one cluster: one covered by Alex's voice, one the user
        // overrode, and one belonging to a cluster the user bound.
        for (id, start, end, cluster, override_id) in [
            ("t-alex", 10.0, 12.0, "SPEAKER_00", None),
            ("t-user", 20.0, 22.0, "SPEAKER_00", Some("spk-bob")),
            ("t-bound", 30.0, 32.0, "SPEAKER_01", None),
        ] {
            sqlx::query(
                "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, source_device, audio_start_time, audio_end_time, speaker_override_id)
                 VALUES (?, 'm1', 'text', '2026-01-01T00:00:00Z', ?, 'System', ?, ?, ?)",
            )
            .bind(id)
            .bind(cluster)
            .bind(start)
            .bind(end)
            .bind(override_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, channel, matched_by, match_score) VALUES ('m1', 'SPEAKER_01', 'spk-bob', 'system', 'user', NULL)")
            .execute(&pool)
            .await
            .unwrap();

        // The meeting's cached exemplars: Alex's voice over the first row, and
        // an unknown voice over the third.
        for (id, start, end, emb) in [
            ("cache-1", 10.5, 11.5, axis(1)),
            ("cache-2", 30.5, 31.5, axis(7)),
        ] {
            sqlx::query(
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at)
                 VALUES (?, ?, 'titanet_large', 'system', 1.0, 'm1', 'SPEAKER_00', ?, ?, '2026-01-01T00:00:00Z')",
            )
            .bind(id)
            .bind(embedding_to_bytes(&emb))
            .bind(start)
            .bind(end)
            .execute(&pool)
            .await
            .unwrap();
        }

        let (cleared, recomputed) = refresh_transcript_row_matches(&pool, "m1")
            .await
            .expect("refresh runs");
        assert_eq!(cleared, 0, "nothing to clear on the first refresh");
        assert_eq!(recomputed, 1, "only the row covered by a known voice is named");
        let named: Option<String> =
            sqlx::query_scalar("SELECT speaker_auto_id FROM transcripts WHERE id = 't-alex'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(named.as_deref(), Some("spk-alex"));

        // Alex leaves the expected-speaker allowlist, so his prototypes are no
        // longer candidates: the refresh must drop the name it gave that row.
        sqlx::query("INSERT INTO meeting_expected_speakers (meeting_id, speaker_id) VALUES ('m1', 'spk-bob')")
            .execute(&pool)
            .await
            .unwrap();
        let (cleared, recomputed) = refresh_transcript_row_matches(&pool, "m1")
            .await
            .expect("refresh runs again");
        assert_eq!(cleared, 1, "the stale row-level name is cleared");
        assert_eq!(recomputed, 0, "and nothing supports a new one");
        let named: Option<String> =
            sqlx::query_scalar("SELECT speaker_auto_id FROM transcripts WHERE id = 't-alex'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(named, None, "the row resolves through its cluster again");

        // The user's decisions are untouched throughout.
        let override_id: Option<String> =
            sqlx::query_scalar("SELECT speaker_override_id FROM transcripts WHERE id = 't-user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(override_id.as_deref(), Some("spk-bob"));
        let bound: (String, String) = sqlx::query_as(
            "SELECT speaker_id, matched_by FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = 'SPEAKER_01'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(bound, ("spk-bob".to_string(), "user".to_string()));
    }

    /// Manual verification harness for per-row-speaker-recognition 5.1: run
    /// the shipped refresh against a real meeting and print what a user would
    /// see before and after. Gated on `MEETILY_VERIFY_DB` (a *copy* of a
    /// database — this writes to it) plus `MEETILY_VERIFY_MEETING`, so it
    /// skips everywhere else. Asserts only what must hold on any data; the
    /// meeting-specific numbers are recorded in the change's tasks file.
    #[tokio::test]
    async fn verify_row_matches_on_a_real_meeting() {
        let (Ok(db), Ok(meeting_id)) = (
            std::env::var("MEETILY_VERIFY_DB"),
            std::env::var("MEETILY_VERIFY_MEETING"),
        ) else {
            eprintln!("skipping: set MEETILY_VERIFY_DB (a copy!) and MEETILY_VERIFY_MEETING");
            return;
        };
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{db}"))
            .await
            .expect("open the database copy");
        // The copy comes from a database the app has not migrated yet, so
        // bring it to the current schema exactly as the app would on start.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate the database copy");

        // The real projection, not a copy of it, so this measures what a
        // surface would actually render.
        let display = |pool: SqlitePool, meeting_id: String| async move {
            sqlx::query_as::<_, (String, Option<String>, Option<String>)>(&format!(
                "SELECT id, speaker_label, speaker_matched_by FROM ({})                  WHERE meeting_id = ? ORDER BY audio_start_time",
                crate::database::repositories::meeting::TRANSCRIPT_DISPLAY_SELECT
            ))
            .bind(meeting_id)
            .fetch_all(&pool)
            .await
            .unwrap()
        };

        let before = display(pool.clone(), meeting_id.clone()).await;
        let overrides_before: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, speaker_override_id FROM transcripts WHERE meeting_id = ? ORDER BY id",
        )
        .bind(&meeting_id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let (cleared, named) = refresh_transcript_row_matches(&pool, &meeting_id)
            .await
            .expect("refresh runs on the real meeting");
        let after = display(pool.clone(), meeting_id.clone()).await;

        let rows = before.len();
        let changed: Vec<(&str, &str, &str)> = before
            .iter()
            .zip(after.iter())
            .filter(|(b, a)| b.1 != a.1)
            .map(|(b, a)| {
                (
                    b.0.as_str(),
                    b.1.as_deref().unwrap_or("<none>"),
                    a.1.as_deref().unwrap_or("<none>"),
                )
            })
            .collect();
        let mut moves: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (_, from, to) in &changed {
            *moves.entry(format!("{from} -> {to}")).or_default() += 1;
        }
        let mut moves: Vec<_> = moves.into_iter().collect();
        moves.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        eprintln!("rows: {rows}, cleared: {cleared}, named from their own audio: {named}");
        eprintln!("rows whose displayed name changed: {}", changed.len());
        for (mv, n) in &moves {
            eprintln!("  {mv}: {n} row(s)");
        }

        // Invariants that must hold on any data.
        let overrides_after: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, speaker_override_id FROM transcripts WHERE meeting_id = ? ORDER BY id",
        )
        .bind(&meeting_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            overrides_before, overrides_after,
            "a refresh must never touch a user override"
        );
        assert!(named <= rows, "cannot name more rows than the meeting has");
        for (b, a) in before.iter().zip(after.iter()) {
            if b.2.as_deref() == Some("user") {
                assert_eq!(b.1, a.1, "a user-provenance row changed name");
            }
        }

        // 5.2: a user's decisions outrank the row-level names on this very
        // data. Override the first row whose name the refresh changed, and
        // confirm its cluster as correct, then re-run the refresh.
        if let Some((row_id, _, _)) = changed.first().map(|(id, f, t)| (id.to_string(), f, t)) {
            let cluster: String =
                sqlx::query_scalar("SELECT speaker FROM transcripts WHERE id = ?")
                    .bind(&row_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let user_speaker: String =
                sqlx::query_scalar("SELECT id FROM speakers ORDER BY name LIMIT 1")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            sqlx::query("UPDATE transcripts SET speaker_override_id = ? WHERE id = ?")
                .bind(&user_speaker)
                .bind(&row_id)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("UPDATE meeting_speakers SET matched_by = 'user' WHERE meeting_id = ? AND cluster_label = ?")
                .bind(&meeting_id)
                .bind(&cluster)
                .execute(&pool)
                .await
                .unwrap();

            let (_, _) = refresh_transcript_row_matches(&pool, &meeting_id)
                .await
                .expect("refresh runs after the user decided");
            let with_user = display(pool.clone(), meeting_id.clone()).await;

            let user_name: String = sqlx::query_scalar("SELECT name FROM speakers WHERE id = ?")
                .bind(&user_speaker)
                .fetch_one(&pool)
                .await
                .unwrap();
            let overridden = with_user.iter().find(|(id, _, _)| id == &row_id).unwrap();
            assert_eq!(
                (overridden.1.as_deref(), overridden.2.as_deref()),
                (Some(user_name.as_str()), Some("user")),
                "the overridden row must show the user's name"
            );

            let cluster_rows: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM transcripts WHERE meeting_id = ? AND speaker = ?",
            )
            .bind(&meeting_id)
            .bind(&cluster)
            .fetch_all(&pool)
            .await
            .unwrap();
            // The changed row's cluster may be one recognition never bound (the
            // row was anonymous before the refresh named it); then there is no
            // confirmed cluster name to check, only the per-row override above.
            let confirmed_name: Option<String> = sqlx::query_scalar(
                "SELECT s.name FROM meeting_speakers ms JOIN speakers s ON s.id = ms.speaker_id
                 WHERE ms.meeting_id = ? AND ms.cluster_label = ?",
            )
            .bind(&meeting_id)
            .bind(&cluster)
            .fetch_optional(&pool)
            .await
            .unwrap()
            .flatten();
            if confirmed_name.is_none() {
                eprintln!(
                    "user decisions: the changed row's cluster {cluster} is unbound, so only the overridden row was checked"
                );
                return;
            }
            let mut checked = 0usize;
            for (id, name, provenance) in &with_user {
                if !cluster_rows.contains(id) || id == &row_id {
                    continue;
                }
                assert_eq!(
                    (name.as_deref(), provenance.as_deref()),
                    (confirmed_name.as_deref(), Some("user")),
                    "row {id} of the confirmed cluster must show the confirmed name"
                );
                checked += 1;
            }
            eprintln!(
                "user decisions: 1 overridden row + {checked} row(s) of the confirmed cluster {cluster} all show the user's names"
            );
        }

    }

    // ===== offline-per-row-recognition =====

    async fn seed_meeting(pool: &SqlitePool) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn seed_row(pool: &SqlitePool, id: &str, start: f64, end: f64, cluster: &str) {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, source_device)
             VALUES (?, 'm1', 'text', '2026-01-01T00:00:00Z', ?, ?, ?, 'System')",
        )
        .bind(id)
        .bind(cluster)
        .bind(start)
        .bind(end)
        .execute(pool)
        .await
        .unwrap();
    }

    /// One segment embedding as an offline run hands it to the persistence step.
    fn seg(speaker: i32, embedding: Vec<f32>, secs: f32, window: (f32, f32)) -> ClusteredEmbedding {
        ClusteredEmbedding {
            speaker,
            embedding,
            duration_secs: secs,
            start_secs: Some(window.0),
            end_secs: Some(window.1),
        }
    }

    /// What a surface would render for a row: the real display projection.
    async fn shown(pool: &SqlitePool, id: &str) -> Option<String> {
        sqlx::query_scalar(&format!(
            "SELECT speaker_label FROM ({}) WHERE id = ?",
            crate::database::repositories::meeting::TRANSCRIPT_DISPLAY_SELECT
        ))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn cluster_speaker(pool: &SqlitePool, label: &str) -> Option<String> {
        sqlx::query_scalar(
            "SELECT speaker_id FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = ?",
        )
        .bind(label)
        .fetch_optional(pool)
        .await
        .unwrap()
        .flatten()
    }

    /// The reported failure: one cluster holds two people and its centroid is
    /// nearer one of them, so the other person's rows were named after him.
    #[tokio::test]
    async fn offline_pass_names_a_row_its_merged_cluster_would_have_misnamed() {
        let pool = setup_pool().await;
        seed_meeting(&pool).await;
        enroll(&pool, "spk-vasiliy", "Vasiliy", "system", &axis(0)).await;
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        seed_row(&pool, "t-alex", 10.0, 12.0, "SPEAKER_00").await;
        seed_row(&pool, "t-vasiliy", 40.0, 42.0, "SPEAKER_00").await;

        // Vasiliy's speech outweighs Alex's inside the one cluster.
        let sys = vec![
            seg(0, axis(1), 2.0, (10.5, 11.5)),
            seg(0, axis(0), 3.0, (40.2, 40.9)),
            seg(0, axis(0), 3.0, (41.0, 41.8)),
        ];
        persist_and_recognize_session(&pool, "m1", &[], &sys, true).await.unwrap();
        assert_eq!(
            cluster_speaker(&pool, "SPEAKER_00").await.as_deref(),
            Some("spk-vasiliy"),
            "the merged cluster is named after the nearer person, as reported"
        );
        assert_eq!(shown(&pool, "t-alex").await.as_deref(), Some("Vasiliy"), "before the step: the wrong name");

        name_rows_after_offline_pass(&pool, "m1").await;

        assert_eq!(shown(&pool, "t-alex").await.as_deref(), Some("Alex"));
        assert_eq!(shown(&pool, "t-vasiliy").await.as_deref(), Some("Vasiliy"));
        assert_eq!(
            cluster_speaker(&pool, "SPEAKER_00").await.as_deref(),
            Some("spk-vasiliy"),
            "the cluster binding itself is left alone"
        );
    }

    /// The other half of the report: a cluster's mean vector scores under the
    /// threshold, so the cluster stays anonymous although a row of it is clearly
    /// somebody's voice.
    #[tokio::test]
    async fn offline_pass_names_a_row_whose_cluster_centroid_is_under_the_threshold() {
        let pool = setup_pool().await;
        seed_meeting(&pool).await;
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        for (id, start) in [("t1", 10.0), ("t2", 20.0), ("t3", 30.0)] {
            seed_row(&pool, id, start, start + 2.0, "SPEAKER_00").await;
        }
        // Three orthogonal voices in one cluster: the centroid is 0.58 from each.
        let sys = vec![
            seg(0, axis(1), 2.0, (10.5, 11.5)),
            seg(0, axis(2), 2.0, (20.5, 21.5)),
            seg(0, axis(3), 2.0, (30.5, 31.5)),
        ];
        persist_and_recognize_session(&pool, "m1", &[], &sys, true).await.unwrap();
        assert_eq!(cluster_speaker(&pool, "SPEAKER_00").await, None, "the centroid alone names nobody");

        name_rows_after_offline_pass(&pool, "m1").await;

        assert_eq!(shown(&pool, "t1").await.as_deref(), Some("Alex"));
        assert_ne!(shown(&pool, "t2").await.as_deref(), Some("Alex"), "a row with no match stays anonymous");
        assert_eq!(cluster_speaker(&pool, "SPEAKER_00").await, None, "and the cluster stays unbound");
    }

    /// A name a previous run recorded must not survive a run that no longer
    /// supports it: it would sit above the new result and could never change.
    #[tokio::test]
    async fn an_offline_pass_replaces_the_row_names_an_earlier_run_left() {
        let pool = setup_pool().await;
        seed_meeting(&pool).await;
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        seed_row(&pool, "t1", 10.0, 12.0, "SPEAKER_00").await;
        seed_row(&pool, "t2", 20.0, 22.0, "SPEAKER_00").await;
        // Left by a live session: t2 was named Alex, but nothing now supports it.
        SpeakerRepository::set_transcript_auto_match(&pool, "t2", "spk-alex", 0.9).await.unwrap();

        let sys = vec![seg(0, axis(1), 2.0, (10.5, 11.5))];
        persist_and_recognize_session(&pool, "m1", &[], &sys, true).await.unwrap();
        name_rows_after_offline_pass(&pool, "m1").await;

        let matches: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, speaker_auto_id FROM transcripts WHERE meeting_id = 'm1' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            matches,
            vec![
                ("t1".to_string(), Some("spk-alex".to_string())),
                ("t2".to_string(), None),
            ],
            "t1 is named by the new run, and the stale name on t2 is gone"
        );
    }

    /// The user decisions are protected by the display order, not by this step
    /// touching them: a per-block override and a user-bound cluster both still
    /// win over what the row audio says.
    #[tokio::test]
    async fn the_offline_row_step_leaves_user_decisions_alone() {
        let pool = setup_pool().await;
        seed_meeting(&pool).await;
        enroll(&pool, "spk-alex", "Alex", "system", &axis(1)).await;
        enroll(&pool, "spk-bob", "Bob", "system", &axis(2)).await;
        enroll(&pool, "spk-carol", "Carol", "system", &axis(3)).await;
        seed_row(&pool, "t-override", 10.0, 12.0, "SPEAKER_00").await;
        seed_row(&pool, "t-bound", 20.0, 22.0, "SPEAKER_01").await;
        sqlx::query("UPDATE transcripts SET speaker_override_id = 'spk-bob' WHERE id = 't-override'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, channel, matched_by, match_score) VALUES ('m1', 'SPEAKER_01', 'spk-carol', 'system', 'user', NULL)")
            .execute(&pool)
            .await
            .unwrap();

        // Both rows own audio says Alex.
        let sys = vec![
            seg(0, axis(1), 2.0, (10.5, 11.5)),
            seg(1, axis(1), 2.0, (20.5, 21.5)),
        ];
        persist_and_recognize_session(&pool, "m1", &[], &sys, true).await.unwrap();
        name_rows_after_offline_pass(&pool, "m1").await;

        assert_eq!(shown(&pool, "t-override").await.as_deref(), Some("Bob"), "the override wins");
        assert_eq!(shown(&pool, "t-bound").await.as_deref(), Some("Carol"), "the user-bound cluster wins");
        let override_id: Option<String> =
            sqlx::query_scalar("SELECT speaker_override_id FROM transcripts WHERE id = 't-override'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(override_id.as_deref(), Some("spk-bob"));
        let bound: (String, String) = sqlx::query_as(
            "SELECT speaker_id, matched_by FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = 'SPEAKER_01'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(bound, ("spk-carol".to_string(), "user".to_string()));
    }

    /// The step cannot fail a diarization. With no tables at all the refresh
    /// itself errors, and the function the orchestrator calls still returns.
    #[tokio::test]
    async fn a_failing_row_step_never_fails_the_diarization() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        assert!(
            refresh_transcript_row_matches(&pool, "m1").await.is_err(),
            "the refresh does report the failure"
        );
        // Returns nothing: there is no error left to propagate.
        name_rows_after_offline_pass(&pool, "m1").await;
    }

    /// The cluster-persistence step persists clusters and cluster bindings and
    /// nothing else: on its own it records no row-level match. (Since
    /// offline-per-row-recognition the offline orchestrator follows it with
    /// `name_rows_after_offline_pass`, which is what names rows; the online stop
    /// path follows it with `recognize_transcript_rows`.)
    #[tokio::test]
    async fn cluster_persistence_records_no_row_level_match_on_its_own() {
        let pool = setup_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        enroll(&pool, "spk-alex", "Alex", "mic", &axis(1)).await;
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, source_device)
             VALUES ('t1', 'm1', 'text', '2026-01-01T00:00:00Z', 'SPEAKER_00', 10.0, 12.0, 'Microphone')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // What the offline orchestrator hands over: clustered embeddings only.
        let mic = vec![ClusteredEmbedding {
            speaker: 0,
            embedding: axis(1),
            duration_secs: 2.0,
            start_secs: Some(10.5),
            end_secs: Some(11.5),
        }];
        persist_and_recognize_session(&pool, "m1", &mic, &[], false)
            .await
            .expect("offline persistence runs");

        // The cluster is recognized, as before this change ...
        let bound: Option<String> = sqlx::query_scalar(
            "SELECT speaker_id FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = 'SPEAKER_00'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap()
        .flatten();
        assert_eq!(bound.as_deref(), Some("spk-alex"));
        // ... and no row carries a row-level match.
        let rows_with_match: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = 'm1' AND speaker_auto_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows_with_match, 0);
    }

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
