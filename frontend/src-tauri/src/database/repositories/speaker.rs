use crate::database::models::{
    bytes_to_embedding, embedding_to_bytes, MeetingExpectedSpeaker, MeetingSpeaker, Speaker,
    SpeakerEmbedding,
};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};
use uuid::Uuid;

/// Model tag identifying the extractor that produced stored embeddings.
/// Both the offline and online diarization paths use the polyvoice WeSpeaker
/// ResNet34 INT8 model, so all embeddings share this tag. Recognition filters
/// to the current model so prints are never compared across extractors.
pub const SPEAKER_EMBEDDING_MODEL: &str = "resnet34-int8";

/// Best-K exemplars reparented into a speaker per enrollment (design open
/// question: start at 8). Tunable as field data accumulates.
pub const ENROLLMENT_BEST_K: usize = 8;

/// Maximum prototypes kept per speaker. Above this, lowest-duration rows are
/// pruned (design open question: start at 64).
pub const PER_PERSON_PROTOTYPE_CAP: usize = 64;

/// Voiceprint storage statistics (change requirement: storage visibility).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpeakerStorageStats {
    pub registry_count: i64,
    pub prototype_count: i64,
    pub cache_count: i64,
    pub total_bytes: i64,
}

/// A single candidate prototype loaded for recognition: which speaker it
/// belongs to, which channel it was captured on, and the decoded embedding.
#[derive(Debug, Clone)]
pub struct PrototypeRow {
    pub speaker_id: String,
    pub channel: String,
    pub embedding: Vec<f32>,
}

/// A cluster centroid read back for re-matching (no audio re-processing).
#[derive(Debug, Clone)]
pub struct ClusterCentroid {
    pub cluster_label: String,
    pub channel: Option<String>,
    pub centroid: Vec<f32>,
}

/// An exemplar embedding + the duration of the source segment (quality signal
/// used to pick the best-K rows during enrollment).
#[derive(Debug, Clone)]
pub struct Exemplar {
    pub embedding: Vec<f32>,
    pub duration_secs: f64,
}

pub struct SpeakerRepository;

impl SpeakerRepository {
    // ===== Speakers CRUD =====

    /// List all registry speakers ordered by name (for the editor dropdown).
    pub async fn list_speakers(pool: &SqlitePool) -> Result<Vec<Speaker>, SqlxError> {
        sqlx::query_as::<_, Speaker>(
            "SELECT id, name, is_me, created_at, updated_at FROM speakers ORDER BY name COLLATE NOCASE ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// Fetch a single speaker by id.
    pub async fn get_speaker(pool: &SqlitePool, id: &str) -> Result<Option<Speaker>, SqlxError> {
        sqlx::query_as::<_, Speaker>(
            "SELECT id, name, is_me, created_at, updated_at FROM speakers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Find an existing speaker by case-insensitive name, or create one.
    /// Idempotent: repeated calls with the same name (any casing) return the
    /// same row. Honors the case-insensitive uniqueness index.
    pub async fn find_or_create_by_name(
        pool: &SqlitePool,
        name: &str,
    ) -> Result<Speaker, SqlxError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(SqlxError::Protocol("speaker name cannot be empty".into()));
        }

        if let Some(existing) = Self::find_by_name(pool, trimmed).await? {
            return Ok(existing);
        }

        let id = format!("speaker-{}", Uuid::new_v4());
        let now = Utc::now();
        match sqlx::query(
            "INSERT INTO speakers (id, name, is_me, created_at, updated_at) VALUES (?, ?, 0, ?, ?)",
        )
        .bind(&id)
        .bind(trimmed)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        {
            Ok(_) => Self::get_speaker(pool, &id)
                .await?
                .ok_or_else(|| SqlxError::Protocol("inserted speaker not found".into())),
            Err(SqlxError::Database(e)) if e.is_unique_violation() => {
                // A concurrent insert won the race; return that row.
                Self::find_by_name(pool, trimmed)
                    .await?
                    .ok_or_else(|| SqlxError::Protocol("unique speaker vanished after race".into()))
            }
            Err(e) => Err(e),
        }
    }

    /// Case-insensitive name lookup.
    pub async fn find_by_name(
        pool: &SqlitePool,
        name: &str,
    ) -> Result<Option<Speaker>, SqlxError> {
        sqlx::query_as::<_, Speaker>(
            "SELECT id, name, is_me, created_at, updated_at FROM speakers WHERE name = ? COLLATE NOCASE LIMIT 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await
    }

    /// Globally rename a speaker. A single-row update; all meetings display
    /// the new name via the read-time join (no per-meeting propagation needed).
    pub async fn rename_speaker(
        pool: &SqlitePool,
        speaker_id: &str,
        new_name: &str,
    ) -> Result<bool, SqlxError> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(SqlxError::Protocol("speaker name cannot be empty".into()));
        }
        let now = Utc::now();
        let rows = sqlx::query(
            "UPDATE speakers SET name = ?, updated_at = ? WHERE id = ?",
        )
        .bind(trimmed)
        .bind(now)
        .bind(speaker_id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected() > 0)
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
        sqlx::query("DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ?")
            .bind(meeting_id)
            .bind(cluster_label)
            .execute(&mut *tx)
            .await?;

        let now = Utc::now();
        for exemplar in exemplars {
            let id = format!("emb-{}", Uuid::new_v4());
            let emb_bytes = embedding_to_bytes(&exemplar.embedding);
            sqlx::query(
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, meeting_id, cluster_label, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&emb_bytes)
            .bind(model)
            .bind(channel)
            .bind(exemplar.duration_secs)
            .bind(meeting_id)
            .bind(cluster_label)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    // ===== meeting_speakers mapping =====

    /// Read all cluster mappings for a meeting.
    pub async fn get_meeting_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<MeetingSpeaker>, SqlxError> {
        sqlx::query_as::<_, MeetingSpeaker>(
            "SELECT meeting_id, cluster_label, speaker_id, centroid, channel, matched_by, match_score
             FROM meeting_speakers WHERE meeting_id = ? ORDER BY cluster_label",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    /// Link a cluster to a speaker with `matched_by='user'` (manual edit).
    /// Persists the binding and (when a centroid already exists) leaves it in
    /// place. Returns the speaker id bound.
    pub async fn set_user_binding(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: &str,
    ) -> Result<(), SqlxError> {
        sqlx::query(
            "INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, matched_by, match_score)
             VALUES (?, ?, ?, 'user', NULL)
             ON CONFLICT(meeting_id, cluster_label) DO UPDATE SET
                 speaker_id = excluded.speaker_id,
                 matched_by = 'user',
                 match_score = NULL",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(speaker_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Auto-assign a cluster to a speaker only when it is not already
    /// user-bound. Re-matching uses this to preserve manual bindings.
    /// Returns true when a binding was set.
    pub async fn set_auto_binding_if_unbound(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: &str,
        score: f64,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query(
            "INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, matched_by, match_score)
             VALUES (?, ?, ?, 'auto', ?)
             ON CONFLICT(meeting_id, cluster_label) DO UPDATE SET
                 speaker_id = CASE WHEN meeting_speakers.matched_by = 'user'
                                   THEN meeting_speakers.speaker_id ELSE excluded.speaker_id END,
                 matched_by = CASE WHEN meeting_speakers.matched_by = 'user'
                                   THEN meeting_speakers.matched_by ELSE 'auto' END,
                 match_score = CASE WHEN meeting_speakers.matched_by = 'user'
                                    THEN meeting_speakers.match_score ELSE excluded.match_score END",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(speaker_id)
        .bind(score)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected() > 0)
    }

    /// Read cached cluster centroids for a meeting (used by re-match, which
    /// runs recognition from centroids only — no audio re-processing).
    pub async fn get_cluster_centroids(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<ClusterCentroid>, SqlxError> {
        let rows = sqlx::query_as::<_, MeetingSpeaker>(
            "SELECT meeting_id, cluster_label, speaker_id, centroid, channel, matched_by, match_score
             FROM meeting_speakers WHERE meeting_id = ? AND centroid IS NOT NULL",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| match r.centroid {
                Some(bytes) => Some(ClusterCentroid {
                    cluster_label: r.cluster_label,
                    channel: r.channel,
                    centroid: bytes_to_embedding(&bytes),
                }),
                None => None,
            })
            .collect())
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
        let mut tx = pool.begin().await?;

        // Reparent the best-K exemplars (longest duration first) to the speaker.
        // SQLite supports UPDATE ... ORDER BY ... LIMIT.
        let k = ENROLLMENT_BEST_K as i64;
        sqlx::query(
            "UPDATE speaker_embeddings SET speaker_id = ?, meeting_id = NULL, cluster_label = NULL
             WHERE id IN (
                 SELECT id FROM speaker_embeddings
                 WHERE meeting_id = ? AND cluster_label = ?
                 ORDER BY duration_secs DESC LIMIT ?
             )",
        )
        .bind(speaker_id)
        .bind(meeting_id)
        .bind(cluster_label)
        .bind(k)
        .execute(&mut *tx)
        .await?;

        // Enforce the per-person prototype cap: if the speaker now has more
        // than the cap, delete the lowest-duration rows above the cap.
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(speaker_id)
                .fetch_one(&mut *tx)
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
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(count.0.min(PER_PERSON_PROTOTYPE_CAP as i64) as usize)
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
        let rows: Vec<SpeakerEmbedding> = match candidate_speaker_ids {
            Some(ids) if !ids.is_empty() => {
                // Build an IN (?, ?, ...) clause for the candidate set.
                let placeholders = std::iter::repeat("?")
                    .take(ids.len())
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, created_at
                     FROM speaker_embeddings
                     WHERE speaker_id IS NOT NULL AND model = ? AND speaker_id IN ({})",
                    placeholders
                );
                let mut q = sqlx::query_as::<_, SpeakerEmbedding>(&sql).bind(model);
                for id in ids {
                    q = q.bind(id);
                }
                q.fetch_all(pool).await?
            }
            _ => {
                sqlx::query_as::<_, SpeakerEmbedding>(
                    "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, created_at
                     FROM speaker_embeddings
                     WHERE speaker_id IS NOT NULL AND model = ?",
                )
                .bind(model)
                .fetch_all(pool)
                .await?
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

    // ===== Expected speakers =====

    /// Replace the expected-speaker allowlist for a meeting with the given set.
    /// An empty set means recognition matches against ALL registry speakers.
    pub async fn set_expected_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
        speaker_ids: &[String],
    ) -> Result<(), SqlxError> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM meeting_expected_speakers WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(&mut *tx)
            .await?;
        for id in speaker_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO meeting_expected_speakers (meeting_id, speaker_id) VALUES (?, ?)",
            )
            .bind(meeting_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Read the expected-speaker ids for a meeting.
    pub async fn get_expected_speakers(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<String>, SqlxError> {
        let rows = sqlx::query_as::<_, MeetingExpectedSpeaker>(
            "SELECT meeting_id, speaker_id FROM meeting_expected_speakers WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.speaker_id).collect())
    }

    // ===== Storage stats =====

    /// Voiceprint storage statistics: registry speaker count, enrolled
    /// prototype count, unassigned cache count, and total embedding bytes.
    pub async fn storage_stats(pool: &SqlitePool) -> Result<SpeakerStorageStats, SqlxError> {
        let registry_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speakers")
            .fetch_one(pool)
            .await?;
        let prototype_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NOT NULL")
                .fetch_one(pool)
                .await?;
        let cache_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE meeting_id IS NOT NULL")
                .fetch_one(pool)
                .await?;
        let total_bytes: (i64,) =
            sqlx::query_as("SELECT COALESCE(SUM(LENGTH(embedding)), 0) FROM speaker_embeddings")
                .fetch_one(pool)
                .await?;
        Ok(SpeakerStorageStats {
            registry_count: registry_count.0,
            prototype_count: prototype_count.0,
            cache_count: cache_count.0,
            total_bytes: total_bytes.0,
        })
    }

    // ===== Per-transcript speaker override (design D10) =====

    /// Set the per-transcript speaker override. Writes ONLY the transcripts row;
    /// does not touch `meeting_speakers` and does not enroll embeddings.
    /// Returns false when the transcript does not exist.
    pub async fn set_transcript_override(
        pool: &SqlitePool,
        transcript_id: &str,
        speaker_id: &str,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query("UPDATE transcripts SET speaker_override_id = ? WHERE id = ?")
            .bind(speaker_id)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
    }

    /// Clear the per-transcript speaker override (falls back to the cluster
    /// mapping at display time). Returns false when the transcript does not
    /// exist or has no override.
    pub async fn clear_transcript_override(
        pool: &SqlitePool,
        transcript_id: &str,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query("UPDATE transcripts SET speaker_override_id = NULL WHERE id = ? AND speaker_override_id IS NOT NULL")
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
    }

    /// Read the transcript's meeting id + cluster label, needed by the
    /// "apply to all blocks of this speaker" route to reuse the cluster-wide
    /// assignment path. Returns None when the transcript does not exist.
    pub async fn get_transcript_cluster(
        pool: &SqlitePool,
        transcript_id: &str,
    ) -> Result<Option<(String, Option<String>)>, SqlxError> {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT meeting_id, speaker FROM transcripts WHERE id = ?",
        )
        .bind(transcript_id)
        .fetch_optional(pool)
        .await
    }

    /// Resolve a transcript's display name with override precedence
    /// (override speaker, then cluster binding, then legacy label) — the same
    /// expression the transcript queries use. Used for single-block lookups.
    pub async fn get_transcript_display_name(
        pool: &SqlitePool,
        transcript_id: &str,
    ) -> Result<Option<String>, SqlxError> {
        sqlx::query_scalar(
            "SELECT COALESCE(so.name, s.name, t.speaker_label)
             FROM transcripts t
             LEFT JOIN speakers so ON so.id = t.speaker_override_id
             LEFT JOIN meeting_speakers ms ON ms.meeting_id = t.meeting_id AND ms.cluster_label = t.speaker
             LEFT JOIN speakers s ON s.id = ms.speaker_id
             WHERE t.id = ?",
        )
        .bind(transcript_id)
        .fetch_optional(pool)
        .await
    }

    /// Apply live per-turn overrides to a meeting's transcripts: for each
    /// (cluster_label, start, end, speaker_id), set `speaker_override_id` on
    /// transcripts of that cluster overlapping the time range. Overrides are
    /// applied after auto-recognition so the user's explicit choice wins;
    /// they never touch `meeting_speakers` or enroll embeddings.
    pub async fn apply_turn_overrides(
        pool: &SqlitePool,
        meeting_id: &str,
        overrides: &[(String, f64, f64, String)],
    ) -> Result<usize, SqlxError> {
        let mut updated = 0usize;
        for (cluster_label, start, end, speaker_id) in overrides {
            let rows = sqlx::query(
                "UPDATE transcripts SET speaker_override_id = ?
                 WHERE meeting_id = ?
                   AND speaker = ?
                   AND audio_start_time < ?
                   AND audio_end_time > ?",
            )
            .bind(speaker_id)
            .bind(meeting_id)
            .bind(cluster_label)
            .bind(end)
            .bind(start)
            .execute(pool)
            .await?;
            updated += rows.rows_affected() as usize;
        }
        Ok(updated)
    }

    // ===== Display-name resolution (used by transcript/meeting queries) =====

    /// Resolve a meeting's cluster_label -> display name map by joining
    /// `meeting_speakers` to `speakers`. Clusters without a registry binding
    /// are omitted; callers fall back to legacy `speaker_label` / the cluster
    /// label itself.
    pub async fn get_display_names(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<std::collections::HashMap<String, String>, SqlxError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT ms.cluster_label, s.name
             FROM meeting_speakers ms
             JOIN speakers s ON s.id = ms.speaker_id
             WHERE ms.meeting_id = ? AND ms.speaker_id IS NOT NULL",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    async fn insert_meeting(pool: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    fn emb(vals: &[f32]) -> Vec<f32> {
        vals.to_vec()
    }

    #[tokio::test]
    async fn find_or_create_is_idempotent_and_case_insensitive() {
        let pool = setup_pool().await;

        let a = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let a2 = SpeakerRepository::find_or_create_by_name(&pool, "alice").await.unwrap();
        assert_eq!(a.id, a2.id, "case-insensitive lookup returns same row");

        let b = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();
        assert_ne!(a.id, b.id, "distinct names create distinct speakers");

        let list = SpeakerRepository::list_speakers(&pool).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn rename_speaker_updates_name() {
        let pool = setup_pool().await;
        let a = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        assert!(SpeakerRepository::rename_speaker(&pool, &a.id, "Alice Smith").await.unwrap());
        let found = SpeakerRepository::find_by_name(&pool, "alice smith").await.unwrap().unwrap();
        assert_eq!(found.id, a.id);
        assert_eq!(found.name, "Alice Smith");
    }

    #[tokio::test]
    async fn write_cache_and_enroll_reparents_best_k() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();

        // 12 exemplars with ascending durations; only best K=8 should enroll.
        let exemplars: Vec<Exemplar> = (0..12)
            .map(|i| Exemplar {
                embedding: emb(&[i as f32, 0.0, 0.0, 0.0]),
                duration_secs: i as f64,
            })
            .collect();
        let centroid = emb(&[99.0, 0.0, 0.0, 0.0]);
        SpeakerRepository::write_cluster_cache(
            &pool, "m1", "SPEAKER_00", "mic", &centroid, &exemplars, SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Cache rows exist before enrollment.
        let stats_before = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats_before.cache_count, 12);
        assert_eq!(stats_before.prototype_count, 0);

        let n = SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();
        assert_eq!(n, ENROLLMENT_BEST_K, "exactly best-K prototypes enrolled");

        let stats_after = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats_after.prototype_count, ENROLLMENT_BEST_K as i64);
        // The 4 lowest-duration cache rows were NOT reparented; they stay as cache.
        assert_eq!(stats_after.cache_count, 4);

        // Enrolled prototypes should be the 8 longest-duration (indices 4..12).
        let protos =
            SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL)
                .await
                .unwrap();
        assert_eq!(protos.len(), ENROLLMENT_BEST_K);
        for p in &protos {
            assert!(p.embedding[0] >= 4.0, "lowest-duration rows must not enroll");
        }
    }

    #[tokio::test]
    async fn enrollment_enforces_per_person_cap() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();

        // Enroll many clusters so total prototypes exceed the per-person cap.
        for c in 0..(PER_PERSON_PROTOTYPE_CAP / ENROLLMENT_BEST_K + 2) {
            let label = format!("SPEAKER_{:02}", c);
            let exemplars: Vec<Exemplar> = (0..ENROLLMENT_BEST_K)
                .map(|i| Exemplar {
                    embedding: emb(&[c as f32, i as f32, 0.0, 0.0]),
                    duration_secs: (c * ENROLLMENT_BEST_K + i) as f64,
                })
                .collect();
            SpeakerRepository::write_cluster_cache(
                &pool, "m1", &label, "mic", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL,
            )
            .await
            .unwrap();
            SpeakerRepository::enroll_cluster(&pool, "m1", &label, &alice.id).await.unwrap();
        }

        let protos =
            SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL)
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
    async fn expected_speakers_round_trip() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();

        SpeakerRepository::set_expected_speakers(&pool, "m1", &[alice.id.clone(), bob.id.clone()])
            .await
            .unwrap();
        let mut ids = SpeakerRepository::get_expected_speakers(&pool, "m1").await.unwrap();
        ids.sort();
        let mut expected = vec![alice.id, bob.id];
        expected.sort();
        assert_eq!(ids, expected);

        // Empty set = match-all.
        SpeakerRepository::set_expected_speakers(&pool, "m1", &[]).await.unwrap();
        assert!(SpeakerRepository::get_expected_speakers(&pool, "m1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn auto_binding_does_not_overwrite_user_binding() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();

        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();
        // Re-match tries to auto-assign Bob; user binding to Alice must win.
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &bob.id, 0.9)
            .await
            .unwrap();

        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1").await.unwrap();
        let row = rows.iter().find(|r| r.cluster_label == "SPEAKER_00").unwrap();
        assert_eq!(row.speaker_id.as_deref(), Some(alice.id.as_str()));
        assert_eq!(row.matched_by.as_deref(), Some("user"));
    }

    #[tokio::test]
    async fn storage_stats_match_raw_sum() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();

        let exemplars = vec![
            Exemplar { embedding: emb(&[1.0, 2.0, 3.0, 4.0]), duration_secs: 1.0 },
            Exemplar { embedding: emb(&[5.0, 6.0, 7.0, 8.0]), duration_secs: 2.0 },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool, "m1", "SPEAKER_00", "mic", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        // Each 4-d f32 embedding is 16 bytes; 2 rows total (both reparented).
        assert_eq!(stats.total_bytes, (4 * 4 * 2) as i64);
        assert_eq!(stats.registry_count, 1);
        assert_eq!(stats.prototype_count, 2);
        assert_eq!(stats.cache_count, 0);
    }

    #[tokio::test]
    async fn centroid_round_trips_through_bytes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let centroid = emb(&[0.1, 0.2, 0.3, 0.4]);
        SpeakerRepository::write_cluster_cache(
            &pool, "m1", "SPEAKER_00", "system", &centroid, &[], SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        let centroids = SpeakerRepository::get_cluster_centroids(&pool, "m1").await.unwrap();
        assert_eq!(centroids.len(), 1);
        assert_eq!(centroids[0].cluster_label, "SPEAKER_00");
        assert_eq!(centroids[0].channel.as_deref(), Some("system"));
        assert_eq!(centroids[0].centroid, centroid);
    }

    async fn insert_transcript(pool: &SqlitePool, id: &str, meeting_id: &str, speaker: &str) {
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp) VALUES (?, ?, 'text', '2026-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(meeting_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
            .bind(speaker)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn block_override_takes_precedence_over_cluster_mapping() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        // Cluster maps to Alice; no override yet -> Alice is displayed.
        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1").await.unwrap().as_deref(),
            Some("Alice")
        );

        // Single-block override to Bob -> Bob wins over the cluster mapping.
        assert!(SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id).await.unwrap());
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1").await.unwrap().as_deref(),
            Some("Bob")
        );

        // meeting_speakers must be untouched by the override.
        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1").await.unwrap();
        assert_eq!(rows[0].speaker_id.as_deref(), Some(alice.id.as_str()));

        // Clearing the override falls back to the cluster mapping.
        assert!(SpeakerRepository::clear_transcript_override(&pool, "t1").await.unwrap());
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1").await.unwrap().as_deref(),
            Some("Alice")
        );
    }

    #[tokio::test]
    async fn block_override_survives_rematch() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id).await.unwrap();

        // Re-match writes a fresh cluster cache + auto-binding; it must not
        // touch the transcript-level override.
        let exemplars = vec![Exemplar { embedding: emb(&[1.0, 2.0, 3.0, 4.0]), duration_secs: 1.0 }];
        SpeakerRepository::write_cluster_cache(
            &pool, "m1", "SPEAKER_00", "mic", &emb(&[0.5; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.8)
            .await
            .unwrap();

        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1").await.unwrap().as_deref(),
            Some("Bob"),
            "override must survive re-match / cache rewrite"
        );
    }
}
