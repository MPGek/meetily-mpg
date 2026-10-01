//! Per-transcript speaker state: user overrides, row-level automatic
//! matches, transcript lookups, time-overlap cluster resolution and
//! display-name resolution.

use super::SpeakerRepository;
use crate::database::models::bytes_to_embedding;
use sqlx::{Error as SqlxError, SqlitePool};

impl SpeakerRepository {
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

    /// Record the row-level automatic speaker match on one transcript
    /// (per-row-speaker-recognition). Writes ONLY that row's two recognition
    /// columns: the cluster label, the cluster's own binding, any user
    /// override and the manual `speaker_label` are all left alone, because
    /// this level sits below a user decision and above the cluster binding at
    /// display time. Returns false when the transcript does not exist.
    pub async fn set_transcript_auto_match(
        pool: &SqlitePool,
        transcript_id: &str,
        speaker_id: &str,
        score: f64,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query(
            "UPDATE transcripts SET speaker_auto_id = ?, speaker_auto_score = ? WHERE id = ?",
        )
        .bind(speaker_id)
        .bind(score)
        .bind(transcript_id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected() > 0)
    }

    /// Drop every row-level automatic match of a meeting, so its rows resolve
    /// through their cluster again. Used before a re-match recomputes them:
    /// clearing first is what stops a stale row-level name from outranking a
    /// freshly refreshed cluster binding. User overrides are untouched.
    pub async fn clear_meeting_auto_matches(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<u64, SqlxError> {
        let rows = sqlx::query(
            "UPDATE transcripts SET speaker_auto_id = NULL, speaker_auto_score = NULL
             WHERE meeting_id = ? AND speaker_auto_id IS NOT NULL",
        )
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected())
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

    /// Read the transcript's meeting id, time range, and source device for
    /// cluster resolution when the transcript has no cluster label.
    /// Returns (meeting_id, audio_start_time, audio_end_time, source_device).
    /// Returns None when the transcript does not exist.
    pub async fn get_transcript_time_info(
        pool: &SqlitePool,
        transcript_id: &str,
    ) -> Result<Option<(String, Option<f64>, Option<f64>, Option<String>)>, SqlxError> {
        sqlx::query_as::<_, (String, Option<f64>, Option<f64>, Option<String>)>(
            "SELECT meeting_id, audio_start_time, audio_end_time, source_device FROM transcripts WHERE id = ?",
        )
        .bind(transcript_id)
        .fetch_optional(pool)
        .await
    }

    /// A meeting's own cached cluster embeddings that carry a time window, as
    /// `(start_secs, end_secs, embedding, channel)`. These are the unassigned
    /// cache rows (`speaker_id IS NULL`) written when the session was
    /// persisted, which is what lets a later re-match recompute the row-level
    /// matches without reading audio (per-row-speaker-recognition, design D1).
    /// Enhanced family only, in line with every other matching path.
    pub async fn list_meeting_cached_embeddings(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<(f32, f32, Vec<f32>, String)>, SqlxError> {
        let rows: Vec<(f64, f64, Vec<u8>, Option<String>)> = sqlx::query_as(
            "SELECT audio_start_time, audio_end_time, embedding, channel
             FROM speaker_embeddings
             WHERE meeting_id = ? AND speaker_id IS NULL AND model = ?
               AND audio_start_time IS NOT NULL AND audio_end_time IS NOT NULL
             ORDER BY audio_start_time",
        )
        .bind(meeting_id)
        .bind(crate::audio::embedder::ENHANCED_MODEL_TAG)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(start, end, blob, channel)| {
                (
                    start as f32,
                    end as f32,
                    bytes_to_embedding(&blob),
                    channel.unwrap_or_else(|| "mic".to_string()),
                )
            })
            .collect())
    }

    /// Every transcript row of a meeting with its time window and source
    /// device, ordered by start time, for the row-level recognition pass
    /// (per-row-speaker-recognition). Returns (id, start, end, source_device);
    /// a row with no time window is returned too, and the caller skips it
    /// because there is nothing to overlap against.
    pub async fn list_transcript_windows(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<(String, Option<f64>, Option<f64>, Option<String>)>, SqlxError> {
        sqlx::query_as::<_, (String, Option<f64>, Option<f64>, Option<String>)>(
            "SELECT id, audio_start_time, audio_end_time, source_device FROM transcripts
             WHERE meeting_id = ? ORDER BY audio_start_time, id",
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    /// Resolve the best-matching cluster label for a transcript whose speaker
    /// column is NULL by finding unassigned cache rows whose time windows
    /// overlap the given time range. Groups by cluster_label and returns the
    /// cluster with the longest total overlap duration. Filters by channel
    /// to maintain channel separation.
    pub async fn resolve_cluster_by_time_overlap(
        pool: &SqlitePool,
        meeting_id: &str,
        channel: &str,
        time_start: f64,
        time_end: f64,
    ) -> Result<Option<String>, SqlxError> {
        if time_end <= time_start {
            return Ok(None);
        }

        // Query unassigned cache rows overlapping the time range
        let rows: Vec<(String, f64, f64)> = sqlx::query_as(
            "SELECT cluster_label, audio_start_time, audio_end_time
             FROM speaker_embeddings
             WHERE meeting_id = ?
               AND channel = ?
               AND speaker_id IS NULL
               AND cluster_label IS NOT NULL
               AND audio_start_time < ?
               AND audio_end_time > ?
               AND audio_start_time IS NOT NULL
               AND audio_end_time IS NOT NULL",
        )
        .bind(meeting_id)
        .bind(channel)
        .bind(time_end)
        .bind(time_start)
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        // Group by cluster_label and calculate total overlap duration
        let mut cluster_overlaps: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for (cluster_label, emb_start, emb_end) in rows {
            let overlap_start = time_start.max(emb_start);
            let overlap_end = time_end.min(emb_end);
            if overlap_end > overlap_start {
                let overlap = overlap_end - overlap_start;
                *cluster_overlaps.entry(cluster_label).or_insert(0.0) += overlap;
            }
        }

        // Return the cluster with the longest total overlap
        Ok(cluster_overlaps
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(label, _)| label))
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

    /// Persist a user's identity onto the stored rows of a user-bound cluster.
    /// Sets `speaker_override_id` on every transcript of the cluster so each
    /// stored row itself resolves to the user's name and user provenance via
    /// the override join, independent of the `meeting_speakers` render-time
    /// join alone. `transcripts.speaker` (the cluster label) is left intact so
    /// it still joins to `meeting_speakers`. This does NOT touch the cluster
    /// binding and does NOT enroll embeddings.
    pub async fn apply_cluster_binding_overrides(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        let rows = sqlx::query(
            "UPDATE transcripts SET speaker_override_id = ?
             WHERE meeting_id = ? AND speaker = ?",
        )
        .bind(speaker_id)
        .bind(meeting_id)
        .bind(cluster_label)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected() as usize)
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
    use super::super::test_support::*;
    use super::super::*;

    /// per-row-speaker-recognition 1.2: the row-level match is its own
    /// channel. Writing and clearing it must leave every other speaker column
    /// of that row exactly as it was, because those columns carry the user's
    /// decisions and the cluster's identity.
    #[tokio::test]
    async fn row_level_match_write_and_clear_touch_nothing_else() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_transcript_window(&pool, "t1", "m1", Some("SPEAKER_00"), 0.0, 2.0, "System").await;
        sqlx::query("UPDATE transcripts SET speaker_label = 'manual', speaker_override_id = 'spk-user' WHERE id = 't1'")
            .execute(&pool)
            .await
            .unwrap();

        let before: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT speaker, speaker_label, speaker_override_id FROM transcripts WHERE id = 't1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(
            SpeakerRepository::set_transcript_auto_match(&pool, "t1", "spk-auto", 0.74)
                .await
                .unwrap()
        );
        let (auto_id, auto_score): (Option<String>, Option<f64>) = sqlx::query_as(
            "SELECT speaker_auto_id, speaker_auto_score FROM transcripts WHERE id = 't1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(auto_id.as_deref(), Some("spk-auto"));
        assert_eq!(auto_score, Some(0.74));

        let after_write: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT speaker, speaker_label, speaker_override_id FROM transcripts WHERE id = 't1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, after_write, "writing the row match changed another column");

        // A row with no match is not reported as cleared, and clearing a
        // meeting leaves the other columns alone too.
        assert_eq!(
            SpeakerRepository::clear_meeting_auto_matches(&pool, "m1")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            SpeakerRepository::clear_meeting_auto_matches(&pool, "m1")
                .await
                .unwrap(),
            0,
            "clearing twice must report nothing left to clear"
        );
        let (auto_id, auto_score): (Option<String>, Option<f64>) = sqlx::query_as(
            "SELECT speaker_auto_id, speaker_auto_score FROM transcripts WHERE id = 't1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(auto_id, None);
        assert_eq!(auto_score, None);

        let after_clear: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT speaker, speaker_label, speaker_override_id FROM transcripts WHERE id = 't1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, after_clear, "clearing the row match changed another column");

        // A transcript that does not exist is reported, not silently ignored.
        assert!(
            !SpeakerRepository::set_transcript_auto_match(&pool, "nope", "spk-auto", 0.9)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn cluster_binding_overrides_persist_user_identity_on_rows() {
        // Simulates finalize persisting a live cluster binding: set_user_binding
        // (meeting_speakers) + apply_cluster_binding_overrides (transcript rows).
        // Reopening the meeting must show the user's name with user provenance.
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;
        insert_transcript(&pool, "t2", "m1", "SPEAKER_00").await;

        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        let n = SpeakerRepository::apply_cluster_binding_overrides(
            &pool,
            "m1",
            "SPEAKER_00",
            &alice.id,
        )
        .await
        .unwrap();
        assert_eq!(
            n, 2,
            "both transcripts of the cluster carry the user's identity"
        );

        for tid in ["t1", "t2"] {
            assert_eq!(
                SpeakerRepository::get_transcript_display_name(&pool, tid)
                    .await
                    .unwrap()
                    .as_deref(),
                Some("Alice"),
                "stored row resolves to the user's name after reopen"
            );
            let override_id: Option<String> =
                sqlx::query_scalar("SELECT speaker_override_id FROM transcripts WHERE id = ?")
                    .bind(tid)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                override_id.as_deref(),
                Some(alice.id.as_str()),
                "row is user-confirmed"
            );
        }
    }

    #[tokio::test]
    async fn block_override_takes_precedence_over_cluster_mapping() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        // Cluster maps to Alice; no override yet -> Alice is displayed.
        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Alice")
        );

        // Single-block override to Bob -> Bob wins over the cluster mapping.
        assert!(
            SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
                .await
                .unwrap()
        );
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Bob")
        );

        // meeting_speakers must be untouched by the override.
        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(rows[0].speaker_id.as_deref(), Some(alice.id.as_str()));

        // Clearing the override falls back to the cluster mapping.
        assert!(SpeakerRepository::clear_transcript_override(&pool, "t1")
            .await
            .unwrap());
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Alice")
        );
    }

    #[tokio::test]
    async fn block_override_survives_rematch() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();

        // Re-match writes a fresh cluster cache + auto-binding; it must not
        // touch the transcript-level override.
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 2.0, 3.0, 4.0]),
            duration_secs: 1.0,
            start_secs: Some(5.0),
            end_secs: Some(6.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.5; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.8)
            .await
            .unwrap();

        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Bob"),
            "override must survive re-match / cache rewrite"
        );
    }

    #[tokio::test]
    async fn resolve_cluster_by_time_overlap_single_match() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        // Create cluster cache with exemplars at specific time ranges
        let exemplars = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 2.0,
                start_secs: Some(10.0),
                end_secs: Some(12.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 3.0,
                start_secs: Some(13.0),
                end_secs: Some(16.0),
            },
        ];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Query overlapping the first exemplar
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 10.5, 11.5)
                .await
                .unwrap();
        assert_eq!(result.as_deref(), Some("SPEAKER_00"));

        // Query overlapping both exemplars
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 11.0, 14.0)
                .await
                .unwrap();
        assert_eq!(result.as_deref(), Some("SPEAKER_00"));
    }

    #[tokio::test]
    async fn resolve_cluster_by_time_overlap_multiple_clusters() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        // SPEAKER_00: exemplars at 10-12 (2s) and 13-16 (3s) = 5s total
        let exemplars0 = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 2.0,
                start_secs: Some(10.0),
                end_secs: Some(12.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 3.0,
                start_secs: Some(13.0),
                end_secs: Some(16.0),
            },
        ];
        // SPEAKER_01: exemplars at 11-15 (4s) = 4s total
        let exemplars1 = vec![Exemplar {
            embedding: emb(&[3.0, 0.0, 0.0, 0.0]),
            duration_secs: 4.0,
            start_secs: Some(11.0),
            end_secs: Some(15.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars0,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_01",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars1,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Query 10-16: SPEAKER_00 has 5s overlap, SPEAKER_01 has 4s overlap -> SPEAKER_00 wins
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 10.0, 16.0)
                .await
                .unwrap();
        assert_eq!(result.as_deref(), Some("SPEAKER_00"));

        // Query 11-15: SPEAKER_00 has 4s overlap (11-12 + 13-15), SPEAKER_01 has 4s overlap -> tie, either is acceptable
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 11.0, 15.0)
                .await
                .unwrap();
        assert!(result.is_some(), "should resolve to a cluster on tie");
    }

    #[tokio::test]
    async fn resolve_cluster_by_time_overlap_no_match() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Query outside the exemplar time range
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 20.0, 25.0)
                .await
                .unwrap();
        assert!(result.is_none(), "no overlap should return None");

        // Query with invalid time range (end <= start)
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 15.0, 10.0)
                .await
                .unwrap();
        assert!(result.is_none(), "invalid time range should return None");

        // Query for non-existent meeting
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m999", "mic", 10.0, 12.0)
                .await
                .unwrap();
        assert!(result.is_none(), "non-existent meeting should return None");
    }

    #[tokio::test]
    async fn resolve_cluster_by_time_overlap_channel_filtering() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        // Mic channel: exemplar at 10-12
        let exemplars_mic = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        // System channel: exemplar at 10-12 (same time, different channel)
        let exemplars_sys = vec![Exemplar {
            embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "MIC_SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars_mic,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "system",
            &emb(&[0.0; 4]),
            &exemplars_sys,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Query mic channel -> should get MIC_SPEAKER_00
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 10.5, 11.5)
                .await
                .unwrap();
        assert_eq!(result.as_deref(), Some("MIC_SPEAKER_00"));

        // Query system channel -> should get SPEAKER_00
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "system", 10.5, 11.5)
                .await
                .unwrap();
        assert_eq!(result.as_deref(), Some("SPEAKER_00"));
    }

    #[tokio::test]
    async fn resolve_cluster_by_time_overlap_ignores_enrolled_prototypes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        // Create cache and enroll it (becomes prototype with speaker_id set)
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();

        // Query should return None because the exemplar is now a prototype (speaker_id is set)
        let result =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", "mic", 10.5, 11.5)
                .await
                .unwrap();
        assert!(
            result.is_none(),
            "enrolled prototypes should not be resolved"
        );
    }

    #[tokio::test]
    async fn assign_block_speaker_with_null_cluster_enrolls_via_time_overlap() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        // Insert transcript with NULL speaker (cluster label)
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, source_device) VALUES (?, ?, 'text', '2026-01-01T00:00:00Z', ?, ?, ?)",
        )
        .bind("t1")
        .bind("m1")
        .bind(10.0)
        .bind(12.0)
        .bind("Microphone")
        .execute(&pool)
        .await
        .unwrap();

        // Create cluster cache with exemplars overlapping the transcript time range
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Verify transcript has NULL speaker
        let cluster = SpeakerRepository::get_transcript_cluster(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        assert!(
            cluster.1.is_none(),
            "transcript should have NULL cluster label"
        );

        // Set the override (simulating assign_block_speaker's first step)
        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();

        // Now resolve cluster by time overlap and enroll
        let time_info = SpeakerRepository::get_transcript_time_info(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        let (_, Some(start), Some(end), Some(source_device)) = time_info else {
            panic!("expected time info with all fields");
        };
        let channel = if source_device == "System" {
            "system"
        } else {
            "mic"
        };
        let resolved =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", channel, start, end)
                .await
                .unwrap();
        assert_eq!(resolved.as_deref(), Some("SPEAKER_00"));

        // Enroll the resolved cluster
        let enrolled = SpeakerRepository::enroll_cluster(&pool, "m1", &resolved.unwrap(), &bob.id)
            .await
            .unwrap();
        assert!(enrolled > 0, "should enroll at least one prototype");

        // Verify Bob now has prototypes
        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&bob.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert!(!protos.is_empty(), "Bob should have enrolled prototypes");
    }

    #[tokio::test]
    async fn assign_block_speaker_with_null_cluster_no_overlap_still_labels() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        // Insert transcript with NULL speaker and time range that doesn't overlap any exemplars
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, source_device) VALUES (?, ?, 'text', '2026-01-01T00:00:00Z', ?, ?, ?)",
        )
        .bind("t1")
        .bind("m1")
        .bind(100.0)
        .bind(102.0)
        .bind("Microphone")
        .execute(&pool)
        .await
        .unwrap();

        // Create cluster cache with exemplars that don't overlap the transcript
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Set the override (label is applied)
        let written = SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();
        assert!(written, "override should be set");

        // Try to resolve cluster - should return None
        let time_info = SpeakerRepository::get_transcript_time_info(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        let (_, Some(start), Some(end), Some(source_device)) = time_info else {
            panic!("expected time info with all fields");
        };
        let channel = if source_device == "System" {
            "system"
        } else {
            "mic"
        };
        let resolved =
            SpeakerRepository::resolve_cluster_by_time_overlap(&pool, "m1", channel, start, end)
                .await
                .unwrap();
        assert!(resolved.is_none(), "no overlap should return None");

        // Verify the label is still applied (display name resolves to Bob)
        let display = SpeakerRepository::get_transcript_display_name(&pool, "t1")
            .await
            .unwrap();
        assert_eq!(
            display.as_deref(),
            Some("Bob"),
            "label should be applied even without enrollment"
        );
    }

    #[tokio::test]
    async fn assign_block_speaker_with_valid_cluster_uses_direct_path() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        // Insert transcript with valid speaker (cluster label)
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        // Create cluster cache
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 2.0,
            start_secs: Some(10.0),
            end_secs: Some(12.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();

        // Verify transcript has valid speaker
        let cluster = SpeakerRepository::get_transcript_cluster(&pool, "t1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            cluster.1.as_deref(),
            Some("SPEAKER_00"),
            "transcript should have cluster label"
        );

        // Set the override
        SpeakerRepository::set_transcript_override(&pool, "t1", &bob.id)
            .await
            .unwrap();

        // Enroll using the direct cluster label (existing behavior)
        let enrolled = SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &bob.id)
            .await
            .unwrap();
        assert!(enrolled > 0, "should enroll at least one prototype");

        // Verify Bob now has prototypes
        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&bob.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert!(!protos.is_empty(), "Bob should have enrolled prototypes");
    }
}
