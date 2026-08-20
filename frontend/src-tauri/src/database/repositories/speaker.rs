use crate::database::models::{
    bytes_to_embedding, embedding_to_bytes, MeetingExpectedSpeaker, MeetingSpeaker, Speaker,
    SpeakerEmbedding,
};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqliteConnection, SqlitePool};
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
    pub start_secs: Option<f32>,
    pub end_secs: Option<f32>,
}

/// Single voiceprint row with resolved meeting title (LEFT JOIN fallback).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct VoiceprintRow {
    pub id: String,
    pub model: String,
    pub channel: String,
    pub duration_secs: f64,
    pub speaker_id: Option<String>,
    pub meeting_id: Option<String>,
    pub cluster_label: Option<String>,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub meeting_title: Option<String>,
    pub created_at: crate::database::models::DateTimeUtc,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeakerVoiceprints {
    pub speaker_id: String,
    pub speaker_name: String,
    pub is_me: bool,
    pub prototype_count: usize,
    pub prototypes: Vec<VoiceprintRow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MeetingVoiceprints {
    pub meeting_id: String,
    pub meeting_title: String,
    pub caches: Vec<VoiceprintRow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoiceprintBrowser {
    pub speakers: Vec<SpeakerVoiceprints>,
    pub unconfirmed: Vec<MeetingVoiceprints>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RejectResult {
    pub speaker_id: Option<String>,
    pub remaining_prototypes: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplaceResult {
    pub affected_meetings: i64,
    pub affected_clusters: i64,
    pub affected_transcripts: i64,
}

pub struct SpeakerRepository;

impl SpeakerRepository {
    /// Enforce the per-person prototype cap by pruning the lowest-duration
    /// rows above the cap. Returns the speaker's prototype count after
    /// pruning (capped). Shared by cluster-cache enrollment and
    /// ground-truth buffer enrollment.
    async fn enforce_prototype_cap(
        conn: &mut SqliteConnection,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
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
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        // Provenance (meeting_id, cluster_label, timecodes) is retained.
        // SQLite supports UPDATE ... ORDER BY ... LIMIT.
        let k = ENROLLMENT_BEST_K as i64;
        sqlx::query(
            "UPDATE speaker_embeddings SET speaker_id = ?
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

        let enrolled = Self::enforce_prototype_cap(&mut tx, speaker_id).await?;

        tx.commit().await?;
        Ok(enrolled)
    }

    /// Enroll chunk embeddings as ground truth for a speaker: picks the
    /// best-N (longest duration first, capped at K=8) chunk embeddings whose
    /// time window overlaps `[window.0, window.1]` and inserts them as direct
    /// prototypes of the speaker (no cluster ownership), enforcing the
    /// per-person cap. Used to make user-chosen block assignments improve
    /// the speaker's global voiceprint set.
    pub async fn enroll_embeddings_from_buffer(
        pool: &SqlitePool,
        speaker_id: &str,
        channel: &str,
        embeddings: &[(f32, f32, Vec<f32>)],
        window: (f32, f32),
    ) -> Result<usize, SqlxError> {
        let (win_start, win_end) = window;
        if win_end <= win_start {
            return Ok(0);
        }
        let mut candidates: Vec<(f64, Vec<f32>, f32, f32)> = embeddings
            .iter()
            .filter(|(e_start, e_end, _)| e_start < &win_end && e_end > &win_start)
            .map(|(s, e, emb)| ((e - s) as f64, emb.clone(), *s, *e))
            .collect();
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
        candidates.truncate(ENROLLMENT_BEST_K as usize);
        if candidates.is_empty() {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;
        let now = Utc::now();
        let mut inserted = 0usize;
        for (dur, emb, start, end) in candidates {
            let id = format!("emb-{}", Uuid::new_v4());
            let emb_bytes = embedding_to_bytes(&emb);
            sqlx::query(
                "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, audio_start_time, audio_end_time, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&emb_bytes)
            .bind(SPEAKER_EMBEDDING_MODEL)
            .bind(channel)
            .bind(dur)
            .bind(speaker_id)
            .bind(start as f64)
            .bind(end as f64)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            inserted += 1;
        }
        Self::enforce_prototype_cap(&mut tx, speaker_id).await?;
        tx.commit().await?;
        Ok(inserted)
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
                    "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
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
                    "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at
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
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL AND meeting_id IS NOT NULL")
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

    // ===== Voiceprint browser (voiceprint-provenance) =====

    /// List voiceprints grouped by speaker (prototypes) and by meeting (unconfirmed caches).
    /// - When `speaker_id_filter` is Some, only that speaker's prototypes are returned.
    /// - When `unconfirmed_only` is true, the `speakers` branch is omitted.
    /// Meeting titles are resolved via LEFT JOIN with "deleted meeting" fallback when
    /// a prototype retains a meeting_id whose meeting was deleted.
    /// Supports lazy/paginated loading by speaker via `limit`/`offset` (applied to speakers).
    pub async fn list_voiceprints(
        pool: &SqlitePool,
        speaker_id_filter: Option<&str>,
        unconfirmed_only: bool,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<VoiceprintBrowser, SqlxError> {
        let mut speakers_out = Vec::new();
        let mut unconfirmed_out = Vec::new();

        if !unconfirmed_only {
            // Fetch speakers (filtered or paginated)
            let speakers: Vec<crate::database::models::Speaker> = if let Some(sid) = speaker_id_filter {
                sqlx::query_as::<_, crate::database::models::Speaker>(
                    "SELECT id, name, is_me, created_at, updated_at FROM speakers WHERE id = ?",
                )
                .bind(sid)
                .fetch_all(pool)
                .await?
            } else if let (Some(lim), Some(off)) = (limit, offset) {
                sqlx::query_as::<_, crate::database::models::Speaker>(
                    "SELECT id, name, is_me, created_at, updated_at FROM speakers ORDER BY name COLLATE NOCASE ASC LIMIT ? OFFSET ?",
                )
                .bind(lim)
                .bind(off)
                .fetch_all(pool)
                .await?
            } else if let Some(lim) = limit {
                sqlx::query_as::<_, crate::database::models::Speaker>(
                    "SELECT id, name, is_me, created_at, updated_at FROM speakers ORDER BY name COLLATE NOCASE ASC LIMIT ?",
                )
                .bind(lim)
                .fetch_all(pool)
                .await?
            } else {
                sqlx::query_as::<_, crate::database::models::Speaker>(
                    "SELECT id, name, is_me, created_at, updated_at FROM speakers ORDER BY name COLLATE NOCASE ASC",
                )
                .fetch_all(pool)
                .await?
            };

            for sp in speakers {
                let rows: Vec<VoiceprintRow> = sqlx::query_as::<_, VoiceprintRow>(
                    "SELECT se.id, se.model, se.channel, se.duration_secs, se.speaker_id, se.meeting_id, se.cluster_label, se.audio_start_time, se.audio_end_time, COALESCE(m.title, CASE WHEN se.meeting_id IS NOT NULL THEN 'deleted meeting' ELSE NULL END) as meeting_title, se.created_at
                     FROM speaker_embeddings se LEFT JOIN meetings m ON m.id = se.meeting_id
                     WHERE se.speaker_id = ?
                     ORDER BY se.created_at DESC",
                )
                .bind(&sp.id)
                .fetch_all(pool)
                .await?;
                let count = rows.len();
                speakers_out.push(SpeakerVoiceprints {
                    speaker_id: sp.id,
                    speaker_name: sp.name,
                    is_me: sp.is_me,
                    prototype_count: count,
                    prototypes: rows,
                });
            }
        }

        // Unconfirmed caches grouped by meeting, unless filtered to a single speaker
        if speaker_id_filter.is_none() {
            let cache_rows: Vec<VoiceprintRow> = sqlx::query_as::<_, VoiceprintRow>(
                "SELECT se.id, se.model, se.channel, se.duration_secs, se.speaker_id, se.meeting_id, se.cluster_label, se.audio_start_time, se.audio_end_time, COALESCE(m.title, 'deleted meeting') as meeting_title, se.created_at
                 FROM speaker_embeddings se LEFT JOIN meetings m ON m.id = se.meeting_id
                 WHERE se.speaker_id IS NULL AND se.meeting_id IS NOT NULL
                 ORDER BY se.meeting_id ASC, se.created_at DESC",
            )
            .fetch_all(pool)
            .await?;

            // Group by meeting_id
            use std::collections::BTreeMap;
            let mut grouped: BTreeMap<String, (String, Vec<VoiceprintRow>)> = BTreeMap::new();
            for row in cache_rows {
                let mid = row.meeting_id.clone().unwrap_or_default();
                let title = row.meeting_title.clone().unwrap_or_else(|| "deleted meeting".to_string());
                grouped.entry(mid.clone()).or_insert_with(|| (title.clone(), Vec::new())).1.push(row);
                // ensure title updated if first row had fallback
                if let Some(entry) = grouped.get_mut(&mid) {
                    if entry.0 == "deleted meeting" && title != "deleted meeting" {
                        entry.0 = title;
                    }
                }
            }
            for (mid, (title, caches)) in grouped {
                unconfirmed_out.push(MeetingVoiceprints {
                    meeting_id: mid,
                    meeting_title: title,
                    caches,
                });
            }
        }

        Ok(VoiceprintBrowser {
            speakers: speakers_out,
            unconfirmed: unconfirmed_out,
        })
    }

    // ===== Rejection / reconfirmation / replacement (voiceprint-rejection) =====

    /// Reject a voiceprint. When `permanent` is false, demote the prototype to
    /// an unassigned cache (retain provenance); when true, delete it outright.
    /// Returns the owning speaker (if any) and remaining prototype count for UI warning.
    pub async fn reject_voiceprint(
        pool: &SqlitePool,
        id: &str,
        permanent: bool,
    ) -> Result<RejectResult, SqlxError> {
        // Load the row to know its owner and provenance
        let row: Option<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        let Some(row) = row else {
            return Err(SqlxError::RowNotFound);
        };
        let speaker_id = row.speaker_id.clone();

        if permanent {
            sqlx::query("DELETE FROM speaker_embeddings WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        } else {
            // Cache rows are rejected by deletion; prototypes are demoted.
            if row.speaker_id.is_none() {
                sqlx::query("DELETE FROM speaker_embeddings WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await?;
            } else {
                // Demote: clear owner but keep provenance. Legacy rows without
                // meeting_id/cluster_label would violate the CHECK — delete them.
                if row.meeting_id.is_none() || row.cluster_label.is_none() {
                    sqlx::query("DELETE FROM speaker_embeddings WHERE id = ?")
                        .bind(id)
                        .execute(pool)
                        .await?;
                } else {
                    let res = sqlx::query("UPDATE speaker_embeddings SET speaker_id = NULL WHERE id = ?")
                        .bind(id)
                        .execute(pool)
                        .await;
                    if let Err(SqlxError::Database(ref db)) = res {
                        // CHECK violation fallback to delete
                        if db.message().contains("CHECK") {
                            sqlx::query("DELETE FROM speaker_embeddings WHERE id = ?")
                                .bind(id)
                                .execute(pool)
                                .await?;
                        } else {
                            res?;
                        }
                    } else {
                        res?;
                    }
                }
            }
        }

        let remaining = if let Some(ref sid) = speaker_id {
            let (cnt,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(sid)
                .fetch_one(pool)
                .await?;
            cnt
        } else {
            0
        };

        Ok(RejectResult { speaker_id, remaining_prototypes: remaining })
    }

    /// Reconfirm a cache (or demoted) voiceprint as a speaker's prototype.
    /// Enforces per-person cap (reuse enforce_prototype_cap), no provenance change.
    pub async fn reconfirm_voiceprint(
        pool: &SqlitePool,
        id: &str,
        speaker_id: &str,
    ) -> Result<(), SqlxError> {
        let mut tx = pool.begin().await?;
        let rows = sqlx::query("UPDATE speaker_embeddings SET speaker_id = ? WHERE id = ?")
            .bind(speaker_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if rows.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }
        Self::enforce_prototype_cap(&mut tx, speaker_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replace a speaker across the whole corpus: re-bind auto-matched clusters
    /// to `target` (or anonymous when None), delete source prototypes, re-match
    /// affected meetings from centroids. Runs atomically.
    pub async fn replace_speaker(
        pool: &SqlitePool,
        source: &str,
        target: Option<&str>,
    ) -> Result<ReplaceResult, SqlxError> {
        let mut tx = pool.begin().await?;

        // Collect affected auto-bound clusters
        let affected_clusters: Vec<(String, String)> = sqlx::query_as(
            "SELECT meeting_id, cluster_label FROM meeting_speakers WHERE speaker_id = ? AND matched_by = 'auto'",
        )
        .bind(source)
        .fetch_all(&mut *tx)
        .await?;

        let affected_clusters_count = affected_clusters.len() as i64;
        let mut affected_meetings_set = std::collections::HashSet::new();
        for (mid, _) in &affected_clusters {
            affected_meetings_set.insert(mid.clone());
        }
        let affected_meetings_count = affected_meetings_set.len() as i64;

        // Count transcripts that would be affected (for reporting)
        let mut affected_transcripts_count: i64 = 0;
        for (mid, cluster) in &affected_clusters {
            let (cnt,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?")
                .bind(mid)
                .bind(cluster)
                .fetch_one(&mut *tx)
                .await?;
            affected_transcripts_count += cnt;
        }

        if affected_clusters.is_empty() {
            // No bindings to move, still delete prototypes of source
            sqlx::query("DELETE FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(source)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(ReplaceResult {
                affected_meetings: 0,
                affected_clusters: 0,
                affected_transcripts: 0,
            });
        }

        // Re-bind each auto-bound row
        for (mid, cluster) in &affected_clusters {
            if let Some(tgt) = target {
                sqlx::query(
                    "UPDATE meeting_speakers SET speaker_id = ?, matched_by = 'auto', match_score = NULL WHERE meeting_id = ? AND cluster_label = ? AND matched_by = 'auto' AND speaker_id = ?",
                )
                .bind(tgt)
                .bind(mid)
                .bind(cluster)
                .bind(source)
                .execute(&mut *tx)
                .await?;
            } else {
                // Unbind to anonymous
                sqlx::query(
                    "UPDATE meeting_speakers SET speaker_id = NULL, match_score = NULL WHERE meeting_id = ? AND cluster_label = ? AND matched_by = 'auto' AND speaker_id = ?",
                )
                .bind(mid)
                .bind(cluster)
                .bind(source)
                .execute(&mut *tx)
                .await?;
            }
        }

        // Delete source prototypes
        sqlx::query("DELETE FROM speaker_embeddings WHERE speaker_id = ?")
            .bind(source)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        // Re-match affected meetings from centroids (outside transaction, best-effort)
        for meeting_id in affected_meetings_set {
            let centroids = SpeakerRepository::get_cluster_centroids(pool, &meeting_id).await.unwrap_or_default();
            if centroids.is_empty() {
                continue;
            }
            // Reload prototypes without source (target already reflects new state)
            let prototypes: Vec<crate::audio::speaker_recognition::Prototype> =
                SpeakerRepository::load_prototypes(pool, None, SPEAKER_EMBEDDING_MODEL)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(crate::audio::speaker_recognition::Prototype::from)
                    .collect();
            // Load current bindings to preserve user ones
            let existing_rows = SpeakerRepository::get_meeting_speakers(pool, &meeting_id).await.unwrap_or_default();
            let existing: std::collections::HashMap<String, Option<String>> = existing_rows
                .into_iter()
                .map(|r| (r.cluster_label.clone(), r.matched_by.clone()))
                .collect();
            for c in &centroids {
                if let Some(by) = existing.get(&c.cluster_label) {
                    if by.as_deref() == Some("user") {
                        continue;
                    }
                }
                if let Some(m) = crate::audio::speaker_recognition::best_match(&c.centroid, c.channel.as_deref(), &prototypes) {
                    let _ = SpeakerRepository::set_auto_binding_if_unbound(pool, &meeting_id, &c.cluster_label, &m.speaker_id, m.score as f64).await;
                }
            }
        }

        Ok(ReplaceResult {
            affected_meetings: affected_meetings_count,
            affected_clusters: affected_clusters_count,
            affected_transcripts: affected_transcripts_count,
        })
    }

    /// Preview the impact of `replace_speaker` without modifying anything.
    pub async fn preview_replace_speaker(
        pool: &SqlitePool,
        source: &str,
    ) -> Result<ReplaceResult, SqlxError> {
        let affected_clusters: Vec<(String, String)> = sqlx::query_as(
            "SELECT meeting_id, cluster_label FROM meeting_speakers WHERE speaker_id = ? AND matched_by = 'auto'",
        )
        .bind(source)
        .fetch_all(pool)
        .await?;
        let mut affected_meetings_set = std::collections::HashSet::new();
        for (mid, _) in &affected_clusters {
            affected_meetings_set.insert(mid.clone());
        }
        let mut affected_transcripts_count: i64 = 0;
        for (mid, cluster) in &affected_clusters {
            let (cnt,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?")
                    .bind(mid)
                    .bind(cluster)
                    .fetch_one(pool)
                    .await?;
            affected_transcripts_count += cnt;
        }
        Ok(ReplaceResult {
            affected_meetings: affected_meetings_set.len() as i64,
            affected_clusters: affected_clusters.len() as i64,
            affected_transcripts: affected_transcripts_count,
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
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + i as f32),
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
            assert!(r.audio_start_time.is_some(), "provenance start time must be retained");
            assert!(r.audio_end_time.is_some(), "provenance end time must be retained");
        }
        // load_prototypes still returns them (filter behavior unchanged)
        assert_eq!(
            SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL)
                .await
                .unwrap()
                .len(),
            ENROLLMENT_BEST_K
        );
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
                    start_secs: Some((c * ENROLLMENT_BEST_K + i) as f32 * 10.0),
                    end_secs: Some((c * ENROLLMENT_BEST_K + i) as f32 * 10.0 + (c * ENROLLMENT_BEST_K + i) as f32),
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
    async fn enroll_embeddings_from_buffer_takes_overlapping_best_n() {
        let pool = setup_pool().await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();

        // Buffer: two chunks overlapping [1.0, 5.0], one far outside.
        let buffer = vec![
            (0.5f32, 4.0f32, emb(&[1.0, 0.0, 0.0, 0.0])), // overlaps
            (2.0f32, 3.0f32, emb(&[2.0, 0.0, 0.0, 0.0])), // overlaps (shortest dur)
            (9.0f32, 12.0f32, emb(&[3.0, 0.0, 0.0, 0.0])), // outside
            (0.0f32, 6.0f32, emb(&[4.0, 0.0, 0.0, 0.0])), // overlaps (longest dur)
        ];

        let n = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool, &alice.id, "mic", &buffer, (1.0, 5.0),
        )
        .await
        .unwrap();
        assert_eq!(n, 3, "only window-overlapping chunks enroll");
        assert_eq!(buffer.len() as usize - 1, n);

        let protos =
            SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL)
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
    }

    #[tokio::test]
    async fn enroll_embeddings_from_buffer_keeps_channels_clean() {
        let pool = setup_pool().await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();

        let _ = SpeakerRepository::enroll_embeddings_from_buffer(
            &pool, &alice.id, "mic", &[(0.0, 4.0, emb(&[1.0, 0.0, 0.0, 0.0]))], (0.0, 4.0),
        )
        .await
        .unwrap();

        let protos =
            SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL)
                .await
                .unwrap();
        assert_eq!(protos.len(), 1);
        assert_eq!(protos[0].channel, "mic");
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
            Exemplar { embedding: emb(&[1.0, 2.0, 3.0, 4.0]), duration_secs: 1.0, start_secs: Some(10.0), end_secs: Some(11.0) },
            Exemplar { embedding: emb(&[5.0, 6.0, 7.0, 8.0]), duration_secs: 2.0, start_secs: Some(20.0), end_secs: Some(22.0) },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool, "m1", "SPEAKER_00", "mic", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        // Each 4-d f32 embedding is 16 bytes; 2 rows total (both reparented, provenance retained but not counted as cache).
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
        let exemplars = vec![Exemplar { embedding: emb(&[1.0, 2.0, 3.0, 4.0]), duration_secs: 1.0, start_secs: Some(5.0), end_secs: Some(6.0) }];
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

    #[tokio::test]
    async fn storage_stats_disambiguates_provenanced_prototype_and_cache() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        // Two clusters, 2 exemplars each
        let exemplars0 = vec![
            Exemplar { embedding: emb(&[1.0, 0.0, 0.0, 0.0]), duration_secs: 1.0, start_secs: Some(10.0), end_secs: Some(11.0) },
            Exemplar { embedding: emb(&[2.0, 0.0, 0.0, 0.0]), duration_secs: 2.0, start_secs: Some(12.0), end_secs: Some(14.0) },
        ];
        let exemplars1 = vec![
            Exemplar { embedding: emb(&[3.0, 0.0, 0.0, 0.0]), duration_secs: 1.5, start_secs: Some(20.0), end_secs: Some(21.5) },
            Exemplar { embedding: emb(&[4.0, 0.0, 0.0, 0.0]), duration_secs: 2.5, start_secs: Some(22.0), end_secs: Some(24.5) },
        ];
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_00", "mic", &emb(&[0.0; 4]), &exemplars0, SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_01", "mic", &emb(&[0.0; 4]), &exemplars1, SPEAKER_EMBEDDING_MODEL).await.unwrap();
        // Enroll only SPEAKER_00 -> 2 prototypes retaining meeting_id, but cache count must exclude them
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();
        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 2);
        assert_eq!(stats.cache_count, 2, "only unassigned SPEAKER_01 caches count");
    }

    #[tokio::test]
    async fn list_voiceprints_grouped_shapes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_meeting(&pool, "m2").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let exemplars = vec![Exemplar { embedding: emb(&[1.0, 0.0, 0.0, 0.0]), duration_secs: 1.0, start_secs: Some(5.0), end_secs: Some(6.0) }];
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_00", "mic", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::write_cluster_cache(&pool, "m2", "SPEAKER_01", "system", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();

        let browser = SpeakerRepository::list_voiceprints(&pool, None, false, None, None).await.unwrap();
        // One speaker with 1 prototype
        assert_eq!(browser.speakers.len(), 1);
        assert_eq!(browser.speakers[0].prototype_count, 1);
        assert_eq!(browser.speakers[0].prototypes[0].meeting_id.as_deref(), Some("m1"));
        assert_eq!(browser.speakers[0].prototypes[0].audio_start_time, Some(5.0));
        // Unconfirmed branch grouped by meeting (m2 only, m1's prototypes not counted)
        assert_eq!(browser.unconfirmed.len(), 1);
        assert_eq!(browser.unconfirmed[0].meeting_id, "m2");
        assert_eq!(browser.unconfirmed[0].caches.len(), 1);

        // Filtered by speaker
        let filtered = SpeakerRepository::list_voiceprints(&pool, Some(&alice.id), false, None, None).await.unwrap();
        assert_eq!(filtered.speakers.len(), 1);
        assert!(filtered.unconfirmed.is_empty(), "speaker filter must not include unconfirmed");

        // Legacy row with NULL provenance should be represented with None
        let id = format!("emb-{}", uuid::Uuid::new_v4());
        sqlx::query("INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(embedding_to_bytes(&emb(&[9.0, 0.0, 0.0, 0.0])))
            .bind(SPEAKER_EMBEDDING_MODEL)
            .bind("mic")
            .bind(1.0)
            .bind(&alice.id)
            .bind(chrono::Utc::now())
            .execute(&pool)
            .await
            .unwrap();
        let browser2 = SpeakerRepository::list_voiceprints(&pool, None, false, None, None).await.unwrap();
        let alice_rows = browser2.speakers.iter().find(|s| s.speaker_id == alice.id).unwrap();
        assert!(alice_rows.prototypes.iter().any(|r| r.meeting_id.is_none() && r.audio_start_time.is_none()), "legacy row must have NULL provenance");
    }

    #[tokio::test]
    async fn reject_demote_and_reconfirm_enforces_cap() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let exemplars: Vec<Exemplar> = (0..3).map(|i| Exemplar { embedding: emb(&[i as f32, 0.0, 0.0, 0.0]), duration_secs: i as f64 + 1.0, start_secs: Some(i as f32 * 10.0), end_secs: Some(i as f32 * 10.0 + 1.0) }).collect();
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_00", "mic", &emb(&[0.0; 4]), &exemplars, SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();
        let rows: Vec<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>("SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE speaker_id = ? ORDER BY created_at DESC")
            .bind(&alice.id)
            .fetch_all(&pool)
            .await
            .unwrap();
        let first_id = rows[0].id.clone();
        // Demote (not permanent) -> should become unconfirmed cache
        let res = SpeakerRepository::reject_voiceprint(&pool, &first_id, false).await.unwrap();
        assert_eq!(res.speaker_id.as_deref(), Some(alice.id.as_str()));
        let remaining: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?").bind(&alice.id).fetch_one(&pool).await.unwrap();
        assert_eq!(remaining.0, 2);
        // Must be excluded from recognition
        let protos = SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL).await.unwrap();
        assert_eq!(protos.len(), 2);
        // Row should now be unassigned cache with same provenance
        let demoted: SpeakerEmbedding = sqlx::query_as::<_, SpeakerEmbedding>("SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE id = ?")
            .bind(&first_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(demoted.speaker_id.is_none());
        assert_eq!(demoted.meeting_id.as_deref(), Some("m1"));

        // Reconfirm it back to Alice (should enforce cap but we are far from 64)
        SpeakerRepository::reconfirm_voiceprint(&pool, &first_id, &alice.id).await.unwrap();
        let protos2 = SpeakerRepository::load_prototypes(&pool, Some(&[alice.id.clone()]), SPEAKER_EMBEDDING_MODEL).await.unwrap();
        assert_eq!(protos2.len(), 3);

        // Cap enforcement: fill to cap and try to exceed
        for c in 0..20 {
            let emb_data = emb(&[100.0 + c as f32, 0.0, 0.0, 0.0]);
            let id = format!("emb-cap-{}", c);
            sqlx::query("INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
                .bind(&id)
                .bind(embedding_to_bytes(&emb_data))
                .bind(SPEAKER_EMBEDDING_MODEL)
                .bind("mic")
                .bind(0.5)
                .bind(&alice.id)
                .bind(chrono::Utc::now())
                .execute(&pool)
                .await
                .unwrap();
        }
        // Insert one more cache and reconfirm should prune to cap
        let cache_id = format!("emb-{}", uuid::Uuid::new_v4());
        sqlx::query("INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&cache_id)
            .bind(embedding_to_bytes(&emb(&[999.0, 0.0, 0.0, 0.0])))
            .bind(SPEAKER_EMBEDDING_MODEL)
            .bind("mic")
            .bind(10.0)
            .bind("m1")
            .bind("SPEAKER_99")
            .bind(1.0)
            .bind(2.0)
            .bind(chrono::Utc::now())
            .execute(&pool)
            .await
            .unwrap();
        SpeakerRepository::reconfirm_voiceprint(&pool, &cache_id, &alice.id).await.unwrap();
        let cnt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?").bind(&alice.id).fetch_one(&pool).await.unwrap();
        assert!(cnt.0 <= PER_PERSON_PROTOTYPE_CAP as i64);
    }

    #[tokio::test]
    async fn replace_preserves_user_binding_and_override_and_is_atomic() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice").await.unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob").await.unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;
        insert_transcript(&pool, "t2", "m1", "SPEAKER_01").await;
        // Alice auto-bound to SPEAKER_00, Bob user-bound to SPEAKER_01
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_00", "mic", &emb(&[0.1; 4]), &vec![Exemplar { embedding: emb(&[0.1; 4]), duration_secs: 1.0, start_secs: Some(1.0), end_secs: Some(2.0) }], SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::write_cluster_cache(&pool, "m1", "SPEAKER_01", "mic", &emb(&[0.2; 4]), &vec![Exemplar { embedding: emb(&[0.2; 4]), duration_secs: 1.0, start_secs: Some(3.0), end_secs: Some(4.0) }], SPEAKER_EMBEDDING_MODEL).await.unwrap();
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.9).await.unwrap();
        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_01", &bob.id).await.unwrap();
        SpeakerRepository::set_transcript_override(&pool, "t2", &bob.id).await.unwrap();
        // Enroll Alice's prototype so she has voiceprint to delete
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id).await.unwrap();

        let res = SpeakerRepository::replace_speaker(&pool, &alice.id, Some(&bob.id)).await.unwrap();
        assert!(res.affected_meetings >= 1);
        assert!(res.affected_clusters >= 1);
        // User binding must survive
        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1").await.unwrap();
        let sp01 = rows.iter().find(|r| r.cluster_label == "SPEAKER_01").unwrap();
        assert_eq!(sp01.speaker_id.as_deref(), Some(bob.id.as_str()));
        assert_eq!(sp01.matched_by.as_deref(), Some("user"));
        // Auto row should now be Bob
        let sp00 = rows.iter().find(|r| r.cluster_label == "SPEAKER_00").unwrap();
        assert_eq!(sp00.speaker_id.as_deref(), Some(bob.id.as_str()));
        // Transcript override preserved
        assert_eq!(SpeakerRepository::get_transcript_display_name(&pool, "t2").await.unwrap().as_deref(), Some("Bob"));
        // Source prototypes deleted
        let cnt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?").bind(&alice.id).fetch_one(&pool).await.unwrap();
        assert_eq!(cnt.0, 0);
    }
}
