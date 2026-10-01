//! Cluster-to-speaker bindings in `meeting_speakers`: reads, user/auto
//! binding, confirmation, cached centroids and cluster-wide re-binding.

use super::SpeakerRepository;
use crate::database::models::{bytes_to_embedding, MeetingSpeaker};
use sqlx::{Error as SqlxError, SqlitePool};

/// A cluster centroid read back for re-matching (no audio re-processing).
#[derive(Debug, Clone)]
pub struct ClusterCentroid {
    pub cluster_label: String,
    pub channel: Option<String>,
    pub centroid: Vec<f32>,
}

impl SpeakerRepository {
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

    /// Read the capture channel recorded for a meeting's cluster, when known.
    /// Used to scope prototype cleanup to the cluster's own channel.
    pub async fn get_cluster_channel(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
    ) -> Result<Option<String>, SqlxError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT channel FROM meeting_speakers WHERE meeting_id = ? AND cluster_label = ?",
        )
        .bind(meeting_id)
        .bind(cluster_label)
        .fetch_optional(pool)
        .await?;
        Ok(row.and_then(|(channel,)| channel))
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

    /// Mark an automatically recognized binding as user-confirmed without
    /// changing the name and without enrolling new voiceprints. When
    /// `scope_all` is false (single block) the transcript's override is set to
    /// the cluster's bound speaker; in every case the cluster's `matched_by`
    /// flips to 'user' and its `match_score` is cleared, so the `(auto)`
    /// decoration no longer renders. Returns the number of bindings confirmed
    /// (0 when the block's cluster has no bound speaker to confirm).
    pub async fn confirm_speaker_binding(
        pool: &SqlitePool,
        transcript_id: &str,
        scope_all: bool,
    ) -> Result<usize, SqlxError> {
        // Resolve the transcript's cluster and its currently bound speaker.
        let cluster = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT t.meeting_id, t.speaker, ms.speaker_id
             FROM transcripts t
             LEFT JOIN meeting_speakers ms
               ON ms.meeting_id = t.meeting_id AND ms.cluster_label = t.speaker
             WHERE t.id = ?",
        )
        .bind(transcript_id)
        .fetch_optional(pool)
        .await?;
        let Some((meeting_id, Some(cluster_label), Some(bound_speaker_id))) = cluster else {
            return Ok(0);
        };

        // Flip the cluster binding to user provenance and clear its score.
        let rows = sqlx::query(
            "UPDATE meeting_speakers SET matched_by = 'user', match_score = NULL
             WHERE meeting_id = ? AND cluster_label = ? AND speaker_id = ?",
        )
        .bind(&meeting_id)
        .bind(&cluster_label)
        .bind(&bound_speaker_id)
        .execute(pool)
        .await?;

        let mut tx = pool.begin().await?;
        if !scope_all {
            // Single-block confirm: mark THIS transcript as user-owned so it
            // reads as user provenance via the override join.
            sqlx::query("UPDATE transcripts SET speaker_override_id = ? WHERE id = ?")
                .bind(&bound_speaker_id)
                .bind(transcript_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        Ok(rows.rows_affected() as usize)
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

    /// Re-bind a cluster to a speaker with cluster-wide scope: first demote the
    /// prototypes a previous binding left with another speaker (so they do not
    /// keep the cluster's audio and the new speaker gets a full seed set), then
    /// enroll the cluster's best-K cache rows. Shared by the offline binding
    /// commands and the online finalize path so both behave identically.
    pub async fn rebind_cluster(
        pool: &SqlitePool,
        meeting_id: &str,
        cluster_label: &str,
        channel: Option<&str>,
        speaker_id: &str,
    ) -> Result<usize, SqlxError> {
        Self::demote_foreign_prototypes(
            pool,
            meeting_id,
            cluster_label,
            channel,
            speaker_id,
            None,
        )
        .await?;
        Self::enroll_cluster(pool, meeting_id, cluster_label, speaker_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[tokio::test]
    async fn cluster_rebind_demotes_previous_speakers_prototypes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        let carol = SpeakerRepository::find_or_create_by_name(&pool, "Carol")
            .await
            .unwrap();

        let exemplars: Vec<Exemplar> = (0..12)
            .map(|i| Exemplar {
                embedding: emb(&[i as f32, 0.0, 0.0, 0.0]),
                duration_secs: i as f64,
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + i as f32),
            })
            .collect();
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

        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &bob.id)
            .await
            .unwrap();

        // Re-binding to Carol (shared offline/online path): demote Bob's
        // prototypes first, then enroll the best-K.
        let n = SpeakerRepository::rebind_cluster(
            &pool,
            "m1",
            "SPEAKER_00",
            Some("mic"),
            &carol.id,
        )
        .await
        .unwrap();
        assert_eq!(n, ENROLLMENT_BEST_K, "Carol receives a full best-K seed set");

        let bob_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&bob.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let carol_rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&carol.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bob_rows.0, 0, "previous speaker keeps nothing");
        assert_eq!(carol_rows.0, ENROLLMENT_BEST_K as i64);
    }

    #[tokio::test]
    async fn auto_binding_does_not_overwrite_user_binding() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        // Re-match tries to auto-assign Bob; user binding to Alice must win.
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &bob.id, 0.9)
            .await
            .unwrap();

        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        let row = rows
            .iter()
            .find(|r| r.cluster_label == "SPEAKER_00")
            .unwrap();
        assert_eq!(row.speaker_id.as_deref(), Some(alice.id.as_str()));
        assert_eq!(row.matched_by.as_deref(), Some("user"));
    }

    #[tokio::test]
    async fn confirm_cluster_binding_clears_score_and_sets_user() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        // Auto-recognized cluster with a score.
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.78)
            .await
            .unwrap();

        // scope_all=true confirms the whole cluster.
        let n = SpeakerRepository::confirm_speaker_binding(&pool, "t1", true)
            .await
            .unwrap();
        assert_eq!(n, 1);

        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        let row = rows
            .iter()
            .find(|r| r.cluster_label == "SPEAKER_00")
            .unwrap();
        assert_eq!(
            row.matched_by.as_deref(),
            Some("user"),
            "cluster flips to user"
        );
        assert_eq!(
            row.match_score, None,
            "match score is cleared so (auto) drops"
        );
        assert_eq!(
            row.speaker_id.as_deref(),
            Some(alice.id.as_str()),
            "speaker name unchanged"
        );
    }

    #[tokio::test]
    async fn confirm_single_block_sets_override() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.65)
            .await
            .unwrap();

        // scope_all=false confirms only this block.
        let n = SpeakerRepository::confirm_speaker_binding(&pool, "t1", false)
            .await
            .unwrap();
        assert_eq!(n, 1);

        // Block resolves as user provenance via the override, still to Alice.
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t1")
                .await
                .unwrap()
                .as_deref(),
            Some("Alice")
        );
        let override_id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT speaker_override_id FROM transcripts WHERE id = ?",
        )
        .bind("t1")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            override_id.as_deref(),
            Some(alice.id.as_str()),
            "single block marked user-owned"
        );
    }

    #[tokio::test]
    async fn confirm_does_not_duplicate_prototypes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        // Seed the cluster cache + enroll so Alice has prototypes.
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 2.0, 3.0, 4.0]),
            duration_secs: 1.0,
            start_secs: Some(10.0),
            end_secs: Some(11.0),
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

        // Auto-recognition also binds the cluster (score recorded on unused cache? no - binding).
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.9)
            .await
            .unwrap();
        let before = SpeakerRepository::storage_stats(&pool)
            .await
            .unwrap()
            .prototype_count;

        let n = SpeakerRepository::confirm_speaker_binding(&pool, "t1", true)
            .await
            .unwrap();
        assert_eq!(n, 1);

        let after = SpeakerRepository::storage_stats(&pool)
            .await
            .unwrap()
            .prototype_count;
        assert_eq!(
            after, before,
            "confirming must not create new speaker_embeddings"
        );
    }

    #[tokio::test]
    async fn confirm_with_no_bound_speaker_returns_zero() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;

        let n = SpeakerRepository::confirm_speaker_binding(&pool, "t1", false)
            .await
            .unwrap();
        assert_eq!(n, 0, "no cluster binding -> nothing confirmed, no error");
    }
}
