//! Corpus-wide speaker replacement and its read-only preview.

use super::{SpeakerRepository, SPEAKER_EMBEDDING_MODEL};
use sqlx::{Error as SqlxError, SqlitePool};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplaceResult {
    pub affected_meetings: i64,
    pub affected_clusters: i64,
    pub affected_transcripts: i64,
}

impl SpeakerRepository {
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
            let (cnt,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?",
            )
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
            let centroids = SpeakerRepository::get_cluster_centroids(pool, &meeting_id)
                .await
                .unwrap_or_default();
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
            let existing_rows = SpeakerRepository::get_meeting_speakers(pool, &meeting_id)
                .await
                .unwrap_or_default();
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
                if let Some(m) = crate::audio::speaker_recognition::best_match(
                    &c.centroid,
                    c.channel.as_deref(),
                    &prototypes,
                ) {
                    let _ = SpeakerRepository::set_auto_binding_if_unbound(
                        pool,
                        &meeting_id,
                        &c.cluster_label,
                        &m.speaker_id,
                        m.score as f64,
                    )
                    .await;
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
            let (cnt,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?",
            )
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[tokio::test]
    async fn replace_preserves_user_binding_and_override_and_is_atomic() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        insert_transcript(&pool, "t1", "m1", "SPEAKER_00").await;
        insert_transcript(&pool, "t2", "m1", "SPEAKER_01").await;
        // Alice auto-bound to SPEAKER_00, Bob user-bound to SPEAKER_01
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[0.1; 4]),
            &[Exemplar {
                embedding: emb(&[0.1; 4]),
                duration_secs: 1.0,
                start_secs: Some(1.0),
                end_secs: Some(2.0),
            }],
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_01",
            "mic",
            &emb(&[0.2; 4]),
            &[Exemplar {
                embedding: emb(&[0.2; 4]),
                duration_secs: 1.0,
                start_secs: Some(3.0),
                end_secs: Some(4.0),
            }],
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::set_auto_binding_if_unbound(&pool, "m1", "SPEAKER_00", &alice.id, 0.9)
            .await
            .unwrap();
        SpeakerRepository::set_user_binding(&pool, "m1", "SPEAKER_01", &bob.id)
            .await
            .unwrap();
        SpeakerRepository::set_transcript_override(&pool, "t2", &bob.id)
            .await
            .unwrap();
        // Enroll Alice's prototype so she has voiceprint to delete
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();

        let res = SpeakerRepository::replace_speaker(&pool, &alice.id, Some(&bob.id))
            .await
            .unwrap();
        assert!(res.affected_meetings >= 1);
        assert!(res.affected_clusters >= 1);
        // User binding must survive
        let rows = SpeakerRepository::get_meeting_speakers(&pool, "m1")
            .await
            .unwrap();
        let sp01 = rows
            .iter()
            .find(|r| r.cluster_label == "SPEAKER_01")
            .unwrap();
        assert_eq!(sp01.speaker_id.as_deref(), Some(bob.id.as_str()));
        assert_eq!(sp01.matched_by.as_deref(), Some("user"));
        // Auto row should now be Bob
        let sp00 = rows
            .iter()
            .find(|r| r.cluster_label == "SPEAKER_00")
            .unwrap();
        assert_eq!(sp00.speaker_id.as_deref(), Some(bob.id.as_str()));
        // Transcript override preserved
        assert_eq!(
            SpeakerRepository::get_transcript_display_name(&pool, "t2")
                .await
                .unwrap()
                .as_deref(),
            Some("Bob")
        );
        // Source prototypes deleted
        let cnt: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cnt.0, 0);
    }
}
