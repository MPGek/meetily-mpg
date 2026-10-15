//! Cluster cache writes, prototype enrollment (cluster, block window and
//! ground-truth buffer), foreign-prototype demotion, the per-person cap, and
//! the prototype load used by recognition.

use super::{
    SpeakerRepository, ENROLLMENT_BEST_K, PER_PERSON_PROTOTYPE_CAP, SPEAKER_EMBEDDING_MODEL,
    SPEAKER_EMBEDDING_MODEL_LEGACY_DASH,
};
use crate::database::models::{bytes_to_embedding, embedding_to_bytes, SpeakerEmbedding};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqliteConnection, SqlitePool};
use uuid::Uuid;

/// Rows examined per enrollment: the best-K by duration plus spares that fill
/// a slot when the coherence guard drops a row ahead of the cut.
const ENROLLMENT_POOL: usize = ENROLLMENT_BEST_K * 2;

/// What an enrollment did: prototypes now held, and candidates the coherence
/// guard left in the cache because they did not fit the rest
/// (voiceprint-enrollment-quality).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrollOutcome {
    pub enrolled: usize,
    pub dropped: usize,
}

/// A single candidate prototype loaded for recognition: which speaker it
/// belongs to, which channel it was captured on, and the decoded embedding.
#[derive(Debug, Clone)]
pub struct PrototypeRow {
    pub speaker_id: String,
    pub channel: String,
    pub embedding: Vec<f32>,
}

/// An exemplar embedding + the duration of the source segment (quality signal
/// used to pick the best-K rows during enrollment).
#[derive(Debug, Clone)]
pub struct Exemplar {
    pub embedding: Vec<f32>,
    pub duration_secs: f64,
    pub start_secs: Option<f32>,
    pub end_secs: Option<f32>,
}

impl SpeakerRepository {
    /// Enforce the per-person prototype cap by pruning the lowest-duration
    /// rows above the cap. Returns the speaker's prototype count after
    /// pruning (capped). Shared by cluster-cache enrollment and
    /// ground-truth buffer enrollment.
    pub(super) async fn enforce_prototype_cap(
        conn: &mut SqliteConnection,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        // Duplicates go first, so they never count toward the cap or evict a
        // distinct prototype.
        Self::collapse_duplicate_prototypes(conn, speaker_id).await?;
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(speaker_id)
                .fetch_one(&mut *conn)
                .await?;
        let excess = count.0 - PER_PERSON_PROTOTYPE_CAP as i64;
        if excess > 0 {
            sqlx::query(
                "DELETE FROM speaker_embeddings WHERE id IN (
                     SELECT id FROM speaker_embeddings
                     WHERE speaker_id = ?
                     ORDER BY duration_secs ASC LIMIT ?
                 )",
            )
            .bind(speaker_id)
            .bind(excess)
            .execute(&mut *conn)
            .await?;
        }
        Ok(count.0.min(PER_PERSON_PROTOTYPE_CAP as i64) as usize)
    }

    /// Keep one row per identical voiceprint of a speaker: same meeting,
    /// channel, audio window and embedding. The row kept is the verified one
    /// if any, then one with a clip, then the oldest; it inherits a clip from
    /// a removed copy when it has none. The migration
    /// `20260930000000_dedupe_speaker_voiceprints.sql` runs the same rule over
    /// every speaker once.
    async fn collapse_duplicate_prototypes(
        conn: &mut SqliteConnection,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        const RANKED: &str = "ranked AS (
                 SELECT id, audio_blob, audio_codec, audio_sample_rate,
                        ROW_NUMBER() OVER w AS rn,
                        FIRST_VALUE(id) OVER w AS keep_id
                 FROM speaker_embeddings
                 WHERE speaker_id = ?
                 WINDOW w AS (
                     PARTITION BY meeting_id, channel, audio_start_time, audio_end_time, embedding
                     ORDER BY is_verified DESC, (audio_blob IS NOT NULL) DESC, created_at ASC, id ASC
                 )
             )";
        sqlx::query(&format!(
            "WITH {RANKED},
             donor AS (
                 SELECT keep_id, audio_blob, audio_codec, audio_sample_rate,
                        ROW_NUMBER() OVER (PARTITION BY keep_id ORDER BY rn) AS dn
                 FROM ranked WHERE rn > 1 AND audio_blob IS NOT NULL
             )
             UPDATE speaker_embeddings
             SET audio_blob = donor.audio_blob,
                 audio_codec = donor.audio_codec,
                 audio_sample_rate = donor.audio_sample_rate
             FROM donor
             WHERE speaker_embeddings.id = donor.keep_id
               AND donor.dn = 1
               AND speaker_embeddings.audio_blob IS NULL"
        ))
        .bind(speaker_id)
        .execute(&mut *conn)
        .await?;
        let removed = sqlx::query(&format!(
            "WITH {RANKED}
             DELETE FROM speaker_embeddings WHERE id IN (SELECT id FROM ranked WHERE rn > 1)"
        ))
        .bind(speaker_id)
        .execute(&mut *conn)
        .await?;
        Ok(removed.rows_affected() as usize)
    }

    // ===== Cluster cache writes (diarization integration) =====

    /// Persist a cluster's centroid (into `meeting_speakers`) and its exemplar
    /// embeddings (into `speaker_embeddings` as unassigned cache rows owned by
    /// `meeting_id` + `cluster_label`). Called by offline/online diarization
    /// after clustering. Replaces any prior cache for this cluster atomically.
    /// Preserves an existing user binding on the `meeting_speakers` row.
    pub async fn write_cluster_cache(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        channel: &str,
        centroid: &[f32],
        exemplars: &[Exemplar],
        model: &str,
    ) -> Result<(), SqlxError> {
        // Best-effort voice clips, cut BEFORE opening the transaction so slow
        // ffmpeg encodes never hold the write lock. One Opus mono clip per
        // exemplar window from the meeting's saved audio (same channel as the
        // embedding). Any failure yields legacy clip-less rows; persistence
        // never fails because of clips.
        let windows: Vec<(f64, f64)> = exemplars
            .iter()
            .map(|e| match (e.start_secs, e.end_secs) {
                (Some(s), Some(en)) => (s as f64, en as f64),
                _ => (f64::NAN, f64::NAN),
            })
            .collect();
        let clips = crate::audio::voiceprint_clips::cut_clips_for_meeting(
            pool,
            meeting_id,
            channel,
            &windows,
        )
        .await;

        let mut tx = pool.begin().await?;

        // Upsert the meeting_speakers row with centroid + channel. An existing
        // user binding (speaker_id + matched_by='user') is preserved: we only
        // touch centroid/channel.
        let centroid_bytes = embedding_to_bytes(centroid);
        sqlx::query(
            "INSERT INTO meeting_speakers (meeting_id, cluster_label, centroid, channel)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(meeting_id, cluster_label) DO UPDATE SET
                 centroid = excluded.centroid,
                 channel = excluded.channel",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(&centroid_bytes)
        .bind(channel)
        .execute(&mut *tx)
        .await?;

        // Drop prior cache rows for this cluster before writing fresh ones.
        // Only delete unassigned cache rows (speaker_id IS NULL); enrolled prototypes
        // (speaker_id IS NOT NULL) are owned by speakers and must survive cache refreshes.
        // The DELETE is channel-scoped to preserve cross-channel cache independence:
        // re-persisting one channel's cache does not affect the other channel's rows.
        sqlx::query(
            "DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ? AND channel = ? AND speaker_id IS NULL",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(channel)
        .execute(&mut *tx)
        .await?;

        let now = Utc::now();
        for (exemplar, clip) in exemplars.iter().zip(clips.iter()) {
            let id = format!("emb-{}", Uuid::new_v4());
            let emb_bytes = embedding_to_bytes(&exemplar.embedding);
            // Verify start_secs present implies end_secs
            let (start, end) = match (exemplar.start_secs, exemplar.end_secs) {
                (Some(s), Some(e)) => (Some(s as f64), Some(e as f64)),
                (None, None) => (None, None),
                (Some(_), None) | (None, Some(_)) => {
                    return Err(SqlxError::Protocol(
                        "Exemplar start_secs present implies end_secs must be present".into(),
                    ))
                }
            };
            sqlx::query(
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL, ?)",
            )
            .bind(&id)
            .bind(&emb_bytes)
            .bind(model)
            .bind(channel)
            .bind(exemplar.duration_secs)
            .bind(meeting_id)
            .bind(cluster_label)
            .bind(start)
            .bind(end)
            .bind(clip.as_deref())
            .bind(clip.as_ref().map(|_| crate::audio::voiceprint_clips::VOICEPRINT_CLIP_CODEC))
            .bind(clip.as_ref().map(|_| crate::audio::voiceprint_clips::VOICEPRINT_CLIP_SAMPLE_RATE as i64))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    // ===== Enrollment =====

    /// Enroll a cluster's cached exemplar embeddings as prototypes of a
    /// speaker by reparenting the best-K cache rows (longest duration first)
    /// to `speaker_id`, then enforcing the per-person prototype cap by pruning
    /// the lowest-duration rows above the cap. Enrollment is reparenting, not
    /// copying (design D2).
    pub async fn enroll_cluster(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        Ok(Self::enroll_cluster_outcome(pool, meeting_id, cluster_label, speaker_id)
            .await?
            .enrolled)
    }

    /// [`Self::enroll_cluster`] that also reports how many candidates the
    /// coherence guard left in the cache.
    pub async fn enroll_cluster_outcome(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: &str,
    ) -> Result<EnrollOutcome, SqlxError> {
        let mut tx = pool.begin().await?;

        // Examine the longest rows (best-K plus spares), keep the first K that
        // cohere with the others. Provenance (meeting_id, cluster_label,
        // timecodes) is retained by reparenting.
        let pool_rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM speaker_embeddings
             WHERE meeting_id = ? AND cluster_label = ?
             ORDER BY duration_secs DESC LIMIT ?",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(ENROLLMENT_POOL as i64)
        .fetch_all(&mut *tx)
        .await?;
        let (picked, dropped) = Self::pick_coherent_rows(&pool_rows);
        for i in picked {
            sqlx::query("UPDATE speaker_embeddings SET speaker_id = ? WHERE id = ?")
                .bind(speaker_id)
                .bind(&pool_rows[i].0)
                .execute(&mut *tx)
                .await?;
        }

        let enrolled = Self::enforce_prototype_cap(&mut tx, speaker_id).await?;

        tx.commit().await?;
        Self::log_dropped(dropped, meeting_id, cluster_label, speaker_id);
        Ok(EnrollOutcome { enrolled, dropped })
    }

    /// Pick up to K coherent rows from a duration-ordered `(id, embedding)`
    /// pool. Returns the picked pool indexes and the dropped count.
    fn pick_coherent_rows(pool_rows: &[(String, Vec<u8>)]) -> (Vec<usize>, usize) {
        let vectors: Vec<Vec<f32>> = pool_rows.iter().map(|(_, b)| bytes_to_embedding(b)).collect();
        let refs: Vec<&[f32]> = vectors.iter().map(|v| v.as_slice()).collect();
        crate::database::repositories::enrollment_guard::pick_coherent(&refs, ENROLLMENT_BEST_K)
    }

    fn log_dropped(dropped: usize, meeting_id: &str, cluster_label: &str, speaker_id: &str) {
        if dropped > 0 {
            tracing::info!(
                meeting_id = %meeting_id,
                cluster_label = %cluster_label,
                speaker_id = %speaker_id,
                dropped,
                "enrollment left incoherent candidates in the cache"
            );
        }
    }

    /// Enroll only the cluster exemplars overlapping a corrected block's time
    /// window and capture channel as prototypes of `speaker_id`. Foreign
    /// prototypes already overlapping that window are demoted to cache first,
    /// so a repeated correction of the same block converges on the newest
    /// speaker instead of leaving the audio with the previous one. Best-K by
    /// duration, no-op on an empty/inverted window. No audio re-processing.
    pub async fn enroll_block_window(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        channel: &str,
        window: (f64, f64),
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        Ok(Self::enroll_block_window_outcome(
            pool,
            meeting_id,
            cluster_label,
            channel,
            window,
            speaker_id,
        )
        .await?
        .enrolled)
    }

    /// [`Self::enroll_block_window`] that also reports how many candidates
    /// the coherence guard left in the cache.
    pub async fn enroll_block_window_outcome(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        channel: &str,
        window: (f64, f64),
        speaker_id: &str,
    ) -> Result<EnrollOutcome, SqlxError> {
        let (start, end) = window;
        if end <= start {
            return Ok(EnrollOutcome { enrolled: 0, dropped: 0 });
        }

        let mut tx = pool.begin().await?;

        // Demote other speakers' overlapping prototypes so this correction can
        // reclaim them. Runs before enrollment, inside the same transaction.
        Self::demote_foreign_prototypes_conn(
            &mut tx,
            meeting_id,
            cluster_label,
            Some(channel),
            speaker_id,
            Some(window),
        )
        .await?;

        // Examine the longest unassigned cache rows overlapping the block
        // (best-K plus spares) and reparent the first K that cohere.
        let pool_rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
            "SELECT id, embedding FROM speaker_embeddings
             WHERE meeting_id = ? AND cluster_label = ? AND channel = ?
               AND speaker_id IS NULL
               AND audio_start_time IS NOT NULL AND audio_end_time IS NOT NULL
               AND audio_start_time < ? AND audio_end_time > ?
             ORDER BY duration_secs DESC LIMIT ?",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(channel)
        .bind(end)
        .bind(start)
        .bind(ENROLLMENT_POOL as i64)
        .fetch_all(&mut *tx)
        .await?;
        let (picked, dropped) = Self::pick_coherent_rows(&pool_rows);
        for i in picked {
            sqlx::query("UPDATE speaker_embeddings SET speaker_id = ? WHERE id = ?")
                .bind(speaker_id)
                .bind(&pool_rows[i].0)
                .execute(&mut *tx)
                .await?;
        }

        let enrolled = Self::enforce_prototype_cap(&mut tx, speaker_id).await?;

        tx.commit().await?;
        Self::log_dropped(dropped, meeting_id, cluster_label, speaker_id);
        Ok(EnrollOutcome { enrolled, dropped })
    }

    /// Demote prototype rows a previous binding left on a cluster back to
    /// unassigned cache, so re-binding (block-scoped or cluster-wide) never
    /// leaves one speaker holding another's audio. Rows owned by
    /// `keep_speaker_id` are retained; `channel`/`window` narrow the scope when
    /// provided. A row covered by a transcript block overridden to its current
    /// speaker is left in place: a legitimate per-block correction wins over
    /// the cluster default. Returns the number of demoted rows.
    pub async fn demote_foreign_prototypes(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        channel: Option<&str>,
        keep_speaker_id: &str,
        window: Option<(f64, f64)>,
    ) -> Result<usize, SqlxError> {
        let mut tx = pool.begin().await?;
        let demoted = Self::demote_foreign_prototypes_conn(
            &mut tx,
            meeting_id,
            cluster_label,
            channel,
            keep_speaker_id,
            window,
        )
        .await?;
        tx.commit().await?;
        Ok(demoted)
    }

    /// Connection-scoped body of [`Self::demote_foreign_prototypes`], usable
    /// inside a caller's transaction (block enrollment).
    async fn demote_foreign_prototypes_conn(
        conn: &mut SqliteConnection,
        meeting_id: &str,
        cluster_label: &str,
        channel: Option<&str>,
        keep_speaker_id: &str,
        window: Option<(f64, f64)>,
    ) -> Result<usize, SqlxError> {
        let mut sql = String::from(
            "UPDATE speaker_embeddings SET speaker_id = NULL
             WHERE meeting_id = ? AND cluster_label = ?
               AND speaker_id IS NOT NULL
               AND speaker_id <> ?",
        );
        if channel.is_some() {
            sql.push_str(" AND channel = ?");
        }
        if window.is_some() {
            sql.push_str(
                " AND audio_start_time IS NOT NULL AND audio_end_time IS NOT NULL
                  AND audio_start_time < ? AND audio_end_time > ?",
            );
        }
        sql.push_str(
            " AND NOT EXISTS (
                 SELECT 1 FROM transcripts t
                 WHERE t.meeting_id = speaker_embeddings.meeting_id
                   AND t.speaker = speaker_embeddings.cluster_label
                   AND t.speaker_override_id = speaker_embeddings.speaker_id
                   AND t.audio_start_time IS NOT NULL AND t.audio_end_time IS NOT NULL
                   AND speaker_embeddings.audio_start_time IS NOT NULL
                   AND speaker_embeddings.audio_end_time IS NOT NULL
                   AND t.audio_start_time < speaker_embeddings.audio_end_time
                   AND t.audio_end_time > speaker_embeddings.audio_start_time
             )",
        );

        let mut query = sqlx::query(&sql)
            .bind(meeting_id)
            .bind(cluster_label)
            .bind(keep_speaker_id);
        if let Some(ch) = channel {
            query = query.bind(ch);
        }
        if let Some((start, end)) = window {
            query = query.bind(end).bind(start);
        }
        let rows = query.execute(&mut *conn).await?;
        Ok(rows.rows_affected() as usize)
    }

    /// Enroll chunk embeddings as ground truth for a speaker: picks the
    /// best-N (longest duration first, capped at K=8) chunk embeddings whose
    /// time window overlaps `[window.0, window.1]` and inserts them as direct
    /// prototypes of the speaker (no cluster ownership), enforcing the
    /// per-person cap. Used to make user-chosen block assignments improve
    /// the speaker's global voiceprint set. Each enrolled prototype row
    /// carries full provenance (`meeting_id`, `cluster_label`, audio timecodes)
    /// so the Voiceprint Browser can display the source meeting and play the
    /// audio clip.
    pub async fn enroll_embeddings_from_buffer(
        pool: &SqlitePool,
        speaker_id: &str,
        channel: &str,
        embeddings: &[(f32, f32, Vec<f32>)],
        window: (f32, f32),
        meeting_id: &str,
        cluster_label: &str,
    ) -> Result<usize, SqlxError> {
        Ok(Self::enroll_embeddings_from_buffer_outcome(
            pool,
            speaker_id,
            channel,
            embeddings,
            window,
            meeting_id,
            cluster_label,
        )
        .await?
        .enrolled)
    }

    /// [`Self::enroll_embeddings_from_buffer`] that also reports how many
    /// candidates the coherence guard dropped.
    pub async fn enroll_embeddings_from_buffer_outcome(
        pool: &SqlitePool,
        speaker_id: &str,
        channel: &str,
        embeddings: &[(f32, f32, Vec<f32>)],
        window: (f32, f32),
        meeting_id: &str,
        cluster_label: &str,
    ) -> Result<EnrollOutcome, SqlxError> {
        let (win_start, win_end) = window;
        if win_end <= win_start {
            return Ok(EnrollOutcome { enrolled: 0, dropped: 0 });
        }
        let mut candidates: Vec<(f64, Vec<f32>, f32, f32)> = embeddings
            .iter()
            .filter(|(e_start, e_end, _)| e_start < &win_end && e_end > &win_start)
            .map(|(s, e, emb)| ((e - s) as f64, emb.clone(), *s, *e))
            .collect();
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
        // Best-K plus spares, then the first K that cohere with the others.
        candidates.truncate(ENROLLMENT_POOL);
        let (picked, dropped) = {
            let refs: Vec<&[f32]> = candidates.iter().map(|c| c.1.as_slice()).collect();
            crate::database::repositories::enrollment_guard::pick_coherent(&refs, ENROLLMENT_BEST_K)
        };
        let candidates: Vec<(f64, Vec<f32>, f32, f32)> = picked
            .into_iter()
            .map(|i| candidates[i].clone())
            .collect();
        // Skip chunks this speaker already holds (another override over the
        // same chunk, or a repeated confirm), before any clip is cut.
        let mut fresh = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let held: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM speaker_embeddings
                 WHERE speaker_id = ? AND meeting_id = ? AND channel = ?
                   AND audio_start_time = ? AND audio_end_time = ? AND embedding = ?
                 LIMIT 1",
            )
            .bind(speaker_id)
            .bind(meeting_id)
            .bind(channel)
            .bind(candidate.2 as f64)
            .bind(candidate.3 as f64)
            .bind(embedding_to_bytes(&candidate.1))
            .fetch_optional(pool)
            .await?;
            if held.is_none() {
                fresh.push(candidate);
            }
        }
        let candidates = fresh;
        if candidates.is_empty() {
            return Ok(EnrollOutcome { enrolled: 0, dropped });
        }

        // Best-effort voice clips for the enrolled candidates, cut before the
        // transaction so ffmpeg never holds the write lock.
        let windows: Vec<(f64, f64)> = candidates
            .iter()
            .map(|(_, _, s, e)| (*s as f64, *e as f64))
            .collect();
        let clips = crate::audio::voiceprint_clips::cut_clips_for_meeting(
            pool,
            meeting_id,
            channel,
            &windows,
        )
        .await;

        let mut tx = pool.begin().await?;
        let now = Utc::now();
        let mut inserted = 0usize;
        for ((dur, emb, start, end), clip) in candidates.into_iter().zip(clips.iter()) {
            let id = format!("emb-{}", Uuid::new_v4());
            let emb_bytes = embedding_to_bytes(&emb);
            // Model-aware: infer family from embedding dimension (192 = TitaNet, 256 = legacy).
            let model_tag = if emb.len() == 192 {
                "titanet_large"
            } else {
                SPEAKER_EMBEDDING_MODEL
            };
            sqlx::query(
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL, ?)",
            )
            .bind(&id)
            .bind(&emb_bytes)
            .bind(model_tag)
            .bind(channel)
            .bind(dur)
            .bind(speaker_id)
            .bind(meeting_id)
            .bind(cluster_label)
            .bind(start as f64)
            .bind(end as f64)
            .bind(clip.as_deref())
            .bind(clip.as_ref().map(|_| crate::audio::voiceprint_clips::VOICEPRINT_CLIP_CODEC))
            .bind(clip.as_ref().map(|_| crate::audio::voiceprint_clips::VOICEPRINT_CLIP_SAMPLE_RATE as i64))
            .bind(now)
            .execute(&mut *tx)
            .await?;
            inserted += 1;
        }
        Self::enforce_prototype_cap(&mut tx, speaker_id).await?;
        tx.commit().await?;
        Self::log_dropped(dropped, meeting_id, cluster_label, speaker_id);
        Ok(EnrollOutcome { enrolled: inserted, dropped })
    }

    // ===== Prototype queries (recognition) =====

    /// Load candidate prototypes for recognition. When `candidate_speaker_ids`
    /// is `None`, loads prototypes for ALL speakers (empty-allowlist semantics).
    /// Filters by the current model tag so prints are never compared across
    /// extractor models.
    pub async fn load_prototypes(
        pool: &SqlitePool,
        candidate_speaker_ids: Option<&[String]>,
        model: &str,
    ) -> Result<Vec<PrototypeRow>, SqlxError> {
        // Backward compat: legacy stored `resnet34-int8` (dash) but canonical is `resnet34_int8` (underscore).
        let is_legacy = model == SPEAKER_EMBEDDING_MODEL;
        let rows: Vec<SpeakerEmbedding> = match candidate_speaker_ids {
            Some(ids) if !ids.is_empty() => {
                // Build an IN (?, ?, ...) clause for the candidate set.
                let placeholders = std::iter::repeat("?")
                    .take(ids.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = if is_legacy {
                    format!(
                        "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
                         FROM speaker_embeddings
                         WHERE speaker_id IS NOT NULL AND model IN (?, ?) AND speaker_id IN ({})",
                        placeholders
                    )
                } else {
                    format!(
                        "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
                         FROM speaker_embeddings
                         WHERE speaker_id IS NOT NULL AND model = ? AND speaker_id IN ({})",
                        placeholders
                    )
                };
                let mut q = sqlx::query_as::<_, SpeakerEmbedding>(&sql);
                if is_legacy {
                    q = q
                        .bind(SPEAKER_EMBEDDING_MODEL)
                        .bind(SPEAKER_EMBEDDING_MODEL_LEGACY_DASH);
                } else {
                    q = q.bind(model);
                }
                for id in ids {
                    q = q.bind(id);
                }
                q.fetch_all(pool).await?
            }
            _ => {
                let (sql, is_legacy_all) = if is_legacy {
                    (
                        "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
                         FROM speaker_embeddings
                         WHERE speaker_id IS NOT NULL AND model IN (?, ?)",
                        true,
                    )
                } else {
                    (
                        "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
                         FROM speaker_embeddings
                         WHERE speaker_id IS NOT NULL AND model = ?",
                        false,
                    )
                };
                let mut q = sqlx::query_as::<_, SpeakerEmbedding>(sql);
                if is_legacy_all {
                    q = q
                        .bind(SPEAKER_EMBEDDING_MODEL)
                        .bind(SPEAKER_EMBEDDING_MODEL_LEGACY_DASH);
                } else {
                    q = q.bind(model);
                }
                q.fetch_all(pool).await?
            }
        };

        Ok(rows
            .into_iter()
            .filter_map(|r| match r.speaker_id {
                Some(sid) => Some(PrototypeRow {
                    speaker_id: sid,
                    channel: r.channel,
                    embedding: bytes_to_embedding(&r.embedding),
                }),
                None => None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;
    use super::*;

    #[tokio::test]
    async fn write_cache_and_enroll_reparents_best_k() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // 12 exemplars with ascending durations; only best K=8 should enroll.
        let exemplars: Vec<Exemplar> = (0..12)
            .map(|i| Exemplar {
                embedding: emb(&[i as f32, 0.0, 0.0, 0.0]),
                duration_secs: i as f64,
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + i as f32),
            })
            .collect();
        let centroid = emb(&[99.0, 0.0, 0.0, 0.0]);
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &centroid,
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Cache rows exist before enrollment.
        let stats_before = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats_before.cache_count, 12);
        assert_eq!(stats_before.prototype_count, 0);

        let n = SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        assert_eq!(n, ENROLLMENT_BEST_K, "exactly best-K prototypes enrolled");

        let stats_after = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats_after.prototype_count, ENROLLMENT_BEST_K as i64);
        // The 4 lowest-duration cache rows were NOT reparented; they stay as cache.
        assert_eq!(stats_after.cache_count, 4);

        // Enrolled prototypes should be the 8 longest-duration (indices 4..12).
        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), ENROLLMENT_BEST_K);
        for p in &protos {
            assert!(
                p.embedding[0] >= 4.0,
                "lowest-duration rows must not enroll"
            );
        }

        // Enrolled prototypes must retain provenance (meeting_id, cluster_label, times)
        let rows: Vec<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE speaker_id = ?",
        )
        .bind(&alice.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), ENROLLMENT_BEST_K);
        for r in &rows {
            assert_eq!(r.meeting_id.as_deref(), Some("m1"));
            assert_eq!(r.cluster_label.as_deref(), Some("SPEAKER_00"));
            assert!(
                r.audio_start_time.is_some(),
                "provenance start time must be retained"
            );
            assert!(
                r.audio_end_time.is_some(),
                "provenance end time must be retained"
            );
        }
        // load_prototypes still returns them (filter behavior unchanged)
        assert_eq!(
            SpeakerRepository::load_prototypes(
                &pool,
                Some(std::slice::from_ref(&alice.id)),
                SPEAKER_EMBEDDING_MODEL
            )
            .await
            .unwrap()
            .len(),
            ENROLLMENT_BEST_K
        );
    }

    #[tokio::test]
    async fn block_correction_enrolls_cluster_cache() {
        // Cluster-wide enrollment path: enroll_cluster reparents the cluster's
        // best-K cache rows. Used by apply-to-all, not by single-block
        // corrections (which use enroll_block_window).
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        let exemplars: Vec<Exemplar> = (0..5)
            .map(|i| Exemplar {
                embedding: emb(&[1.0 + i as f32, 0.0, 0.0, 0.0]),
                duration_secs: (i + 1) as f64,
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + (i + 1) as f32),
            })
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[99.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // The single-block correction path: set override + enroll cluster.
        assert!(
            SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
                .await
                .unwrap()
        );
        let (meeting_id, cluster_label) = SpeakerRepository::get_transcript_cluster(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        let cluster_label = cluster_label.unwrap();
        let n = SpeakerRepository::enroll_cluster(&pool, &meeting_id, &cluster_label, &bob.id)
            .await
            .unwrap();
        assert_eq!(
            n, 5,
            "all cached exemplars of the block's cluster are enrolled"
        );

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&bob.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), 5);

        // Enrolled prototypes retain provenance (meeting_id, cluster_label).
        let rows: Vec<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE speaker_id = ?",
        )
        .bind(&bob.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 5);
        for r in &rows {
            assert_eq!(r.meeting_id.as_deref(), Some("m1"));
            assert_eq!(r.cluster_label.as_deref(), Some("SPEAKER_00"));
        }

        // The block override must still resolve to Bob.
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Bob")
        );
    }

    #[tokio::test]
    async fn block_correction_without_cache_is_noop() {
        // Legacy/no-cache meeting: enroll_cluster for a cluster with no cache
        // rows returns 0 without error, so the label still applies.
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        let (meeting_id, cluster_label) = SpeakerRepository::get_transcript_cluster(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        let cluster_label = cluster_label.unwrap();

        let n = SpeakerRepository::enroll_cluster(&pool, &meeting_id, &cluster_label, &bob.id)
            .await
            .unwrap();
        assert_eq!(n, 0, "no cache rows -> zero prototypes enrolled, no error");

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 0);
    }

    #[tokio::test]
    async fn enroll_block_window_only_enrolls_overlapping_channel_rows_capped_at_k() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // 10 mic exemplars inside the window, 2 far outside, 1 system inside.
        let mut mic: Vec<Exemplar> = (0..10)
            .map(|i| Exemplar {
                embedding: emb(&[100.0 + i as f32, 0.0, 0.0, 0.0]),
                duration_secs: (i + 1) as f64,
                start_secs: Some(10.0 + i as f32),
                end_secs: Some(11.0 + i as f32),
            })
            .collect();
        mic.push(Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 50.0,
            start_secs: Some(0.0),
            end_secs: Some(5.0),
        });
        mic.push(Exemplar {
            embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
            duration_secs: 50.0,
            start_secs: Some(200.0),
            end_secs: Some(205.0),
        });
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[9.0; 4]),
            &mic,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        let sys = vec![Exemplar {
            embedding: emb(&[7.0, 0.0, 0.0, 0.0]),
            duration_secs: 99.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "system",
            &emb(&[8.0; 4]),
            &sys,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Window 9..22 overlaps all 10 mic exemplars -> exactly K reparented.
        let n = SpeakerRepository::enroll_block_window(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            (9.0, 22.0),
            &alice.id,
        )
        .await
        .unwrap();
        assert_eq!(n, ENROLLMENT_BEST_K, "capped at K within the window");

        let alice_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_rows.0, ENROLLMENT_BEST_K as i64);

        // The two far mic rows and the system row never enrolled.
        let mic_cache: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL AND channel = 'mic'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mic_cache.0, 4);
        let sys_cache: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL AND channel = 'system'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(sys_cache.0, 1, "other channel excluded by channel filter");
    }

    #[tokio::test]
    async fn demote_foreign_prototypes_keeps_override_pinned_rows() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        let carol = SpeakerRepository::find_or_create_by_name(&pool, "Carol")
            .await
            .unwrap();

        // Two system exemplars, both enrolled to Bob.
        let exemplars = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 4.0,
                start_secs: Some(10.0),
                end_secs: Some(14.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 4.0,
                start_secs: Some(20.0),
                end_secs: Some(24.0),
            },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "system",
            &emb(&[9.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &bob.id)
            .await
            .unwrap();

        // A sibling block overridden to Bob pins the first exemplar.
        insert_transcript_window(&pool, "t1", "m1", Some("SPEAKER_00"), 9.0, 15.0, "System").await;
        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();

        // Demoting the pinned window changes nothing.
        let d1 = SpeakerRepository::demote_foreign_prototypes(
            &pool,
            "m1",
            "SPEAKER_00",
            Some("system"),
            &carol.id,
            Some((9.0, 15.0)),
        )
        .await
        .unwrap();
        assert_eq!(d1, 0, "pinned row is protected from demotion");

        // Demoting the unpinned window returns the second exemplar to cache.
        let d2 = SpeakerRepository::demote_foreign_prototypes(
            &pool,
            "m1",
            "SPEAKER_00",
            Some("system"),
            &carol.id,
            Some((19.0, 25.0)),
        )
        .await
        .unwrap();
        assert_eq!(d2, 1);

        let bob_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&bob.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bob_rows.0, 1, "Bob keeps only the pinned exemplar");
    }

    #[tokio::test]
    async fn repeated_block_correction_converges_on_latest_speaker() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 4.0,
            start_secs: Some(10.0),
            end_secs: Some(14.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[9.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        insert_transcript_window(&pool, "t1", "m1", Some("SPEAKER_00"), 9.0, 15.0, "Microphone").await;

        // First correction: Alice.
        SpeakerRepository::set_transcript_override(&pool, "t1", &alice.id)
            .await
            .unwrap();
        let n1 = SpeakerRepository::enroll_block_window(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            (9.0, 15.0),
            &alice.id,
        )
        .await
        .unwrap();
        assert_eq!(n1, 1);

        // Re-correction of the same block: Bob.
        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();
        let n2 = SpeakerRepository::enroll_block_window(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            (9.0, 15.0),
            &bob.id,
        )
        .await
        .unwrap();
        assert_eq!(n2, 1);

        let alice_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let bob_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&bob.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_rows.0, 0, "previous owner loses the block's audio");
        assert_eq!(bob_rows.0, 1, "latest correction owns the block's audio");
    }

    #[tokio::test]
    async fn mixed_cluster_block_correction_does_not_enroll_siblings() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alex = SpeakerRepository::find_or_create_by_name(&pool, "Alex")
            .await
            .unwrap();

        // One cluster covering three people; the longest exemplars are not
        // Alex's (this is what the whole-cluster best-K used to enroll).
        let exemplars = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 17.4,
                start_secs: Some(46.8),
                end_secs: Some(105.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 22.0,
                start_secs: Some(235.0),
                end_secs: Some(257.0),
            },
            Exemplar {
                embedding: emb(&[3.0, 0.0, 0.0, 0.0]),
                duration_secs: 9.1,
                start_secs: Some(287.6),
                end_secs: Some(296.7),
            },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_02",
            "system",
            &emb(&[9.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        let before = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(before.len(), 1);
        assert!(before[0].speaker_id.is_none());

        // Correct only Alex's block.
        insert_transcript_window(&pool, "t_alex", "m1", Some("SPEAKER_02"), 287.6, 296.7, "System")
            .await;
        SpeakerRepository::set_transcript_override(&pool, "t_alex", &alex.id)
            .await
            .unwrap();
        let n = SpeakerRepository::enroll_block_window(
            &pool,
            "m1",
            "SPEAKER_02",
            "system",
            (287.6, 296.7),
            &alex.id,
        )
        .await
        .unwrap();
        assert_eq!(n, 1, "only Alex's own exemplar is enrolled");

        let alex_rows: Vec<(f64, f64)> = sqlx::query_as(
            "SELECT audio_start_time, audio_end_time FROM speaker_embeddings WHERE speaker_id = ?",
        )
        .bind(&alex.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(alex_rows.len(), 1);
        assert!((alex_rows[0].0 - 287.6).abs() < 0.01);
        assert!((alex_rows[0].1 - 296.7).abs() < 0.01);

        // Sibling exemplars remain unassigned cache.
        let cache: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cache.0, 2, "sibling speakers' exemplars are untouched");

        // A single-block correction never binds the cluster.
        let after = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(after.len(), 1);
        assert!(after[0].speaker_id.is_none());
        assert!(after[0].matched_by.is_none());
    }

    #[tokio::test]
    async fn null_cluster_block_correction_enrolls_only_overlapping_rows() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        let exemplars = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 5.0,
                start_secs: Some(100.0),
                end_secs: Some(105.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 9.0,
                start_secs: Some(200.0),
                end_secs: Some(205.0),
            },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_05",
            "system",
            &emb(&[9.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        insert_transcript_window(&pool, "t1", "m1", None, 99.0, 106.0, "System").await;
        let resolved = SpeakerRepository::resolve_cluster_by_time_overlap(
            &pool, "m1", "system", 99.0, 106.0,
        )
        .await
        .unwrap();
        assert_eq!(resolved.as_deref(), Some("SPEAKER_05"));

        let n = SpeakerRepository::enroll_block_window(
            &pool,
            "m1",
            resolved.as_deref().unwrap(),
            "system",
            (99.0, 106.0),
            &bob.id,
        )
        .await
        .unwrap();
        assert_eq!(n, 1, "only the overlapping exemplar is enrolled");

        let bob_rows: Vec<(f64, f64)> = sqlx::query_as(
            "SELECT audio_start_time, audio_end_time FROM speaker_embeddings WHERE speaker_id = ?",
        )
        .bind(&bob.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(bob_rows.len(), 1);
        assert!((bob_rows[0].0 - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn enrollment_enforces_per_person_cap() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // Enroll many clusters so total prototypes exceed the per-person cap.
        for c in 0..(PER_PERSON_PROTOTYPE_CAP / ENROLLMENT_BEST_K + 2) {
            let label = format!("SPEAKER_{:02}", c);
            let exemplars: Vec<Exemplar> = (0..ENROLLMENT_BEST_K)
                .map(|i| Exemplar {
                    embedding: emb(&[c as f32, i as f32, 0.0, 0.0]),
                    duration_secs: (c * ENROLLMENT_BEST_K + i) as f64,
                    start_secs: Some((c * ENROLLMENT_BEST_K + i) as f32 * 10.0),
                    end_secs: Some(
                        (c * ENROLLMENT_BEST_K + i) as f32 * 10.0
                            + (c * ENROLLMENT_BEST_K + i) as f32,
                    ),
                })
                .collect();
            SpeakerRepository::write_cluster_cache(
                &pool,
                "m1",
                &label,
                "mic",
                &emb(&[0.0; 4]),
                &exemplars,
                SPEAKER_EMBEDDING_MODEL,
            )
            .await
            .unwrap();
            SpeakerRepository::enroll_cluster(&pool, "m1", &label, &alice.id)
                .await
                .unwrap();
        }

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert!(
            protos.len() <= PER_PERSON_PROTOTYPE_CAP,
            "prototype count {} exceeds cap {}",
            protos.len(),
            PER_PERSON_PROTOTYPE_CAP
        );
    }

    #[tokio::test]
    async fn enroll_embeddings_from_buffer_takes_overlapping_best_n() {
        let pool = setup_pool().await;
        let meeting_id = "meeting-1";
        insert_meeting(&pool, meeting_id).await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // Buffer: two chunks overlapping [1.0, 5.0], one far outside.
        let buffer = vec![
            (0.5f32, 4.0f32, emb(&[1.0, 0.0, 0.0, 0.0])), // overlaps
            (2.0f32, 3.0f32, emb(&[2.0, 0.0, 0.0, 0.0])), // overlaps (shortest dur)
            (9.0f32, 12.0f32, emb(&[3.0, 0.0, 0.0, 0.0])), // outside
            (0.0f32, 6.0f32, emb(&[4.0, 0.0, 0.0, 0.0])), // overlaps (longest dur)
        ];

        let cluster_label = "MIC_SPEAKER_00";
        let n = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool,
            &alice.id,
            "mic",
            &buffer,
            (1.0, 5.0),
            meeting_id,
            cluster_label,
        )
        .await
        .unwrap();
        assert_eq!(n, 3, "only window-overlapping chunks enroll");
        assert_eq!(buffer.len() as usize - 1, n);

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), 3);
        assert!(protos.iter().all(|p| p.channel == "mic"));
        // The longest-overlapping chunk (duration 6, x=4.0) must be included.
        assert!(protos.iter().any(|p| p.embedding[0] == 4.0));
        assert!(
            protos.iter().all(|p| p.embedding[0] != 3.0),
            "non-overlapping chunk must not enroll"
        );
        // Verify provenance is preserved
        let rows: Vec<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE speaker_id = ?"
        )
        .bind(&alice.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.meeting_id.as_deref() == Some(meeting_id)));
        assert!(rows.iter().all(|r| r.cluster_label.as_deref() == Some(cluster_label)));
    }

    #[tokio::test]
    async fn enroll_embeddings_from_buffer_keeps_channels_clean() {
        let pool = setup_pool().await;
        let meeting_id = "meeting-2";
        insert_meeting(&pool, meeting_id).await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        let _ = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool,
            &alice.id,
            "mic",
            &[(0.0, 4.0, emb(&[1.0, 0.0, 0.0, 0.0]))],
            (0.0, 4.0),
            meeting_id,
            "MIC_SPEAKER_00",
        )
        .await
        .unwrap();

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), 1);
        assert_eq!(protos[0].channel, "mic");
    }

    async fn prototype_count(pool: &SqlitePool, speaker_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
            .bind(speaker_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Several sub-row overrides overlap one ~20 s chunk: re-enrolling it for
    /// the same person must not add a second copy (fix-live-subrow-assignment-scope).
    #[tokio::test]
    async fn enroll_embeddings_from_buffer_does_not_duplicate_a_chunk() {
        let pool = setup_pool().await;
        let meeting_id = "meeting-dup";
        insert_meeting(&pool, meeting_id).await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let buffer = vec![(1473.4f32, 1493.2f32, emb(&[1.0, 0.0, 0.0, 0.0]))];

        let first = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool, &alice.id, "system", &buffer, (1480.0, 1481.0), meeting_id, "SPEAKER_03",
        )
        .await
        .unwrap();
        // A second override, another sub-row in the same chunk, another cluster label.
        let second = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool, &alice.id, "system", &buffer, (1485.0, 1486.0), meeting_id, "SPEAKER_26",
        )
        .await
        .unwrap();

        assert_eq!(first, 1);
        assert_eq!(second, 0, "an already-held chunk is not re-enrolled");
        assert_eq!(prototype_count(&pool, &alice.id).await, 1);
    }

    /// At the cap, a duplicate must be collapsed before pruning, so the
    /// shortest distinct prototype is not evicted to make room for a copy.
    #[tokio::test]
    async fn duplicates_never_evict_a_distinct_prototype_at_the_cap() {
        let pool = setup_pool().await;
        let meeting_id = "meeting-cap-dup";
        insert_meeting(&pool, meeting_id).await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let insert = |i: usize, dur: f64, x: f32| {
            let pool = pool.clone();
            let speaker = alice.id.clone();
            async move {
                sqlx::query(
                    "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at)
                     VALUES (?, ?, 'titanet_large', 'system', ?, ?, ?, 'SPEAKER_00', ?, ?, '2026-01-01T00:00:00Z')",
                )
                .bind(format!("row-{i}"))
                .bind(embedding_to_bytes(&emb(&[x, 0.0, 0.0, 0.0])))
                .bind(dur)
                .bind(&speaker)
                .bind(meeting_id)
                .bind(x as f64 * 100.0)
                .bind(x as f64 * 100.0 + dur)
                .execute(&pool)
                .await
                .unwrap();
            }
        };
        // 63 long distinct prototypes plus one short distinct one: at the cap.
        for i in 0..PER_PERSON_PROTOTYPE_CAP - 1 {
            insert(i, 20.0, i as f32 + 1.0).await;
        }
        insert(900, 1.0, 900.0).await;
        // A byte-identical copy of the first long prototype.
        insert(901, 20.0, 1.0).await;

        let mut conn = pool.acquire().await.unwrap();
        let kept = SpeakerRepository::enforce_prototype_cap(&mut conn, &alice.id).await.unwrap();
        drop(conn);

        assert_eq!(kept, PER_PERSON_PROTOTYPE_CAP);
        let short_survives: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM speaker_embeddings WHERE id = 'row-900'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(short_survives, 1, "the distinct short prototype stays");
        let copies: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE id IN ('row-0', 'row-901')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(copies, 1, "only one of the identical pair remains");
    }

    /// The dedupe migration collapses 13 identical copies to one, keeps the
    /// verified flag and a clip from different copies, and leaves distinct
    /// rows and unassigned cache rows alone.
    #[tokio::test]
    async fn dedupe_migration_collapses_existing_duplicates() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m-mig").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let row = |id: String, speaker: Option<String>, x: f32, verified: i64, clip: Option<Vec<u8>>, created: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, created_at)
                     VALUES (?, ?, 'titanet_large', 'system', 19.8, ?, 'm-mig', 'SPEAKER_03', 1473.43, 1493.21, ?, 'opus', 16000, ?, ?)",
                )
                .bind(id)
                .bind(embedding_to_bytes(&emb(&[x, 0.0, 0.0, 0.0])))
                .bind(speaker)
                .bind(clip)
                .bind(verified)
                .bind(created)
                .execute(&pool)
                .await
                .unwrap();
            }
        };
        // 13 copies: the oldest has neither flag nor clip, one is verified
        // without a clip, one has a clip but is unverified.
        row("dup-00".into(), Some(alice.id.clone()), 1.0, 0, None, "2026-01-01T00:00:00Z").await;
        row("dup-01".into(), Some(alice.id.clone()), 1.0, 1, None, "2026-01-01T00:00:01Z").await;
        row("dup-02".into(), Some(alice.id.clone()), 1.0, 0, Some(vec![7, 7]), "2026-01-01T00:00:02Z").await;
        for i in 3..13 {
            row(format!("dup-{i:02}"), Some(alice.id.clone()), 1.0, 0, None, "2026-01-01T00:00:03Z").await;
        }
        row("distinct".into(), Some(alice.id.clone()), 2.0, 0, None, "2026-01-01T00:00:00Z").await;
        row("cache-a".into(), None, 1.0, 0, None, "2026-01-01T00:00:00Z").await;
        row("cache-b".into(), None, 1.0, 0, None, "2026-01-01T00:00:00Z").await;

        sqlx::raw_sql(include_str!("../../../../migrations/20260930000000_dedupe_speaker_voiceprints.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let kept: Vec<(String, i64, Option<Vec<u8>>)> = sqlx::query_as(
            "SELECT id, is_verified, audio_blob FROM speaker_embeddings WHERE speaker_id = ? ORDER BY id",
        )
        .bind(&alice.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(kept.len(), 2, "one of the 13 copies plus the distinct row");
        let survivor = kept.iter().find(|r| r.0 != "distinct").unwrap();
        assert_eq!(survivor.0, "dup-01", "the verified copy is kept");
        assert_eq!(survivor.1, 1);
        assert_eq!(survivor.2.as_deref(), Some(&[7u8, 7][..]), "it inherits the clip");
        let caches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(caches, 2, "unassigned cache rows are untouched");
    }

    #[tokio::test]
    async fn the_same_chunk_enrolls_for_a_different_person() {
        let pool = setup_pool().await;
        let meeting_id = "meeting-dup-2";
        insert_meeting(&pool, meeting_id).await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();
        let buffer = vec![(0.0f32, 20.0f32, emb(&[1.0, 0.0, 0.0, 0.0]))];

        for id in [&alice.id, &bob.id] {
            SpeakerRepository::enroll_embeddings_from_buffer(
                &pool, id, "system", &buffer, (1.0, 2.0), meeting_id, "SPEAKER_00",
            )
            .await
            .unwrap();
        }

        assert_eq!(prototype_count(&pool, &alice.id).await, 1);
        assert_eq!(prototype_count(&pool, &bob.id).await, 1);
    }

    #[tokio::test]
    async fn centroid_round_trips_through_bytes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let centroid = emb(&[0.1, 0.2, 0.3, 0.4]);
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "system",
            &centroid,
            &[],
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        let centroids = SpeakerRepository::get_cluster_centroids(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(centroids.len(), 1);
        assert_eq!(centroids[0].cluster_label, "SPEAKER_00");
        assert_eq!(centroids[0].channel.as_deref(), Some("system"));
        assert_eq!(centroids[0].centroid, centroid);
    }

    /// Regression test: write_cluster_cache must not delete enrolled prototypes.
    /// This test verifies the fix for the bug where offline diarization re-runs
    /// would wipe out voiceprints enrolled during the recording session.
    #[tokio::test]
    async fn write_cluster_cache_preserves_enrolled_prototypes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // Step 1: Write initial cache with 5 exemplars
        let initial_exemplars: Vec<Exemplar> = (0..5)
            .map(|i| Exemplar {
                embedding: emb(&[0.5 + i as f32, 0.0, 0.0, 0.0]),
                duration_secs: (i + 1) as f64,
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + (i + 1) as f32),
            })
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[99.0; 4]),
            &initial_exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Step 2: Enroll the cluster's exemplars as Alice's prototypes
        let enrolled = SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        assert_eq!(enrolled, 5, "all 5 exemplars should be enrolled");

        // Verify Alice has prototypes
        let protos_before = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(protos_before.len(), 5, "Alice should have 5 prototypes before cache refresh");

        // Step 3: Simulate offline diarization re-run: write new cache for the same cluster
        // This is what happens when offline diarization runs after online enrollment
        let new_exemplars: Vec<Exemplar> = (10..15)
            .map(|i| Exemplar {
                embedding: emb(&[i as f32, 0.0, 0.0, 0.0]),
                duration_secs: (i - 9) as f64,
                start_secs: Some((i - 10) as f32 * 100.0),
                end_secs: Some((i - 10) as f32 * 100.0 + (i - 9) as f32),
            })
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[88.0; 4]), // new centroid
            &new_exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Step 4: Verify Alice's prototypes survived the cache refresh
        let protos_after = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(
            protos_after.len(),
            5,
            "Alice's prototypes must survive cache refresh (bug regression)"
        );

        // Verify the prototypes are the ORIGINAL ones (values 0..5), not the new cache (10..15)
        for p in &protos_after {
            assert!(
                p.embedding[0] < 5.0,
                "prototypes should be the original enrolled ones, not the new cache"
            );
        }

        // Step 5: Verify the new cache rows exist separately (unassigned)
        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 5, "Alice's 5 prototypes");
        assert_eq!(stats.cache_count, 5, "new cache has 5 unassigned exemplars");
    }

    // ===== Coherence guard on enrollment (guard-prototype-enrollment) =====

    #[tokio::test]
    async fn enroll_cluster_leaves_outliers_in_the_cache_and_fills_from_the_next_longest() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        // Two foreign-sounding rows that are the LONGEST of the cluster, so
        // without the guard both would be among the best-K.
        cache_with_outliers(
            &pool,
            10,
            &[([0.0, 0.0, 1.0, 0.0], 50.0), ([0.0, 0.0, 0.0, 1.0], 40.0)],
        )
        .await;

        let out = SpeakerRepository::enroll_cluster_outcome(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        assert_eq!(out, EnrollOutcome { enrolled: ENROLLMENT_BEST_K, dropped: 2 });

        // The outliers are still unassigned cache rows.
        let outliers_in_cache: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL AND duration_secs >= 40.0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(outliers_in_cache.0, 2);
        // The slots went to the next-longest coherent rows (durations 10..=3).
        let shortest: (f64,) = sqlx::query_as(
            "SELECT MIN(duration_secs) FROM speaker_embeddings WHERE speaker_id = ?",
        )
        .bind(&alice.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(shortest.0, 3.0);
    }

    #[tokio::test]
    async fn enroll_cluster_without_outliers_drops_nothing() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        cache_with_outliers(&pool, 10, &[]).await;
        let out = SpeakerRepository::enroll_cluster_outcome(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        assert_eq!(out, EnrollOutcome { enrolled: ENROLLMENT_BEST_K, dropped: 0 });
    }

    #[tokio::test]
    async fn enroll_block_window_drops_an_outlier_but_enrolls_a_two_row_block_whole() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        // Window 490..600 covers only the two outliers' windows (500+).
        cache_with_outliers(
            &pool,
            6,
            &[([10.0, 1.0, 0.0, 0.0], 20.0), ([0.0, 0.0, 1.0, 0.0], 30.0)],
        )
        .await;
        let two = SpeakerRepository::enroll_block_window_outcome(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            (490.0, 600.0),
            &alice.id,
        )
        .await
        .unwrap();
        assert_eq!(
            two,
            EnrollOutcome { enrolled: 2, dropped: 0 },
            "two rows are too few to judge against each other"
        );

        // A wider window with coherent rows plus one foreign row (the
        // longest): the foreign row is left in the cache.
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_meeting(&pool, "m2").await;
        let ex: Vec<Exemplar> = (0..6)
            .map(|i| Exemplar {
                embedding: emb(&[10.0 + i as f32, 1.0, 0.0, 0.0]),
                duration_secs: (i + 1) as f64,
                start_secs: Some(10.0 + i as f32),
                end_secs: Some(11.0 + i as f32),
            })
            .chain(std::iter::once(Exemplar {
                embedding: emb(&[0.0, 0.0, 0.0, 1.0]),
                duration_secs: 30.0,
                start_secs: Some(12.0),
                end_secs: Some(14.0),
            }))
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m2",
            "SPEAKER_00",
            "mic",
            &emb(&[1.0, 0.0, 0.0, 0.0]),
            &ex,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        let wide = SpeakerRepository::enroll_block_window_outcome(
            &pool,
            "m2",
            "SPEAKER_00",
            "mic",
            (9.0, 20.0),
            &bob.id,
        )
        .await
        .unwrap();
        assert_eq!(wide, EnrollOutcome { enrolled: 6, dropped: 1 });
        let left: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE meeting_id = 'm2' AND speaker_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(left.0, 1, "only the outlier stays unassigned");
    }

    #[tokio::test]
    async fn enroll_embeddings_from_buffer_skips_an_outlier_chunk_and_still_skips_held_ones() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "meeting-1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let mut buffer: Vec<(f32, f32, Vec<f32>)> = (0..5)
            .map(|i| {
                (
                    i as f32,
                    i as f32 + 1.0 + 0.1 * i as f32,
                    emb(&[10.0 + i as f32, 1.0, 0.0, 0.0]),
                )
            })
            .collect();
        buffer.push((0.0, 9.0, emb(&[0.0, 0.0, 1.0, 0.0]))); // longest, foreign

        let first = SpeakerRepository::enroll_embeddings_from_buffer_outcome(
            &pool, &alice.id, "mic", &buffer, (0.0, 10.0), "meeting-1", "MIC_SPEAKER_00",
        )
        .await
        .unwrap();
        assert_eq!(first, EnrollOutcome { enrolled: 5, dropped: 1 });
        let stored: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored.0, 5);

        let second = SpeakerRepository::enroll_embeddings_from_buffer_outcome(
            &pool, &alice.id, "mic", &buffer, (0.0, 10.0), "meeting-1", "MIC_SPEAKER_00",
        )
        .await
        .unwrap();
        assert_eq!(second.enrolled, 0, "chunks already held are not enrolled twice");
    }
}
