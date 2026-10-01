//! Voiceprint storage statistics.

use super::SpeakerRepository;
use sqlx::{Error as SqlxError, SqlitePool};

/// Voiceprint storage statistics (change requirement: storage visibility).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpeakerStorageStats {
    pub registry_count: i64,
    pub prototype_count: i64,
    pub cache_count: i64,
    pub total_bytes: i64,
    /// Total bytes of stored voice-clip blobs (separate from embeddings).
    pub audio_bytes: i64,
    /// Number of rows carrying a stored voice clip.
    pub clip_count: i64,
}

impl SpeakerRepository {
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
        let audio_bytes: (i64,) =
            sqlx::query_as("SELECT COALESCE(SUM(LENGTH(audio_blob)), 0) FROM speaker_embeddings")
                .fetch_one(pool)
                .await?;
        let clip_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE audio_blob IS NOT NULL")
                .fetch_one(pool)
                .await?;
        Ok(SpeakerStorageStats {
            registry_count: registry_count.0,
            prototype_count: prototype_count.0,
            cache_count: cache_count.0,
            total_bytes: total_bytes.0,
            audio_bytes: audio_bytes.0,
            clip_count: clip_count.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[tokio::test]
    async fn storage_stats_match_raw_sum() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        let exemplars = vec![
            Exemplar {
                embedding: emb(&[1.0, 2.0, 3.0, 4.0]),
                duration_secs: 1.0,
                start_secs: Some(10.0),
                end_secs: Some(11.0),
            },
            Exemplar {
                embedding: emb(&[5.0, 6.0, 7.0, 8.0]),
                duration_secs: 2.0,
                start_secs: Some(20.0),
                end_secs: Some(22.0),
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
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        // Each 4-d f32 embedding is 16 bytes; 2 rows total (both reparented, provenance retained but not counted as cache).
        assert_eq!(stats.total_bytes, (4 * 4 * 2) as i64);
        assert_eq!(stats.registry_count, 1);
        assert_eq!(stats.prototype_count, 2);
        assert_eq!(stats.cache_count, 0);
    }

    #[tokio::test]
    async fn storage_stats_disambiguates_provenanced_prototype_and_cache() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        // Two clusters, 2 exemplars each
        let exemplars0 = vec![
            Exemplar {
                embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
                duration_secs: 1.0,
                start_secs: Some(10.0),
                end_secs: Some(11.0),
            },
            Exemplar {
                embedding: emb(&[2.0, 0.0, 0.0, 0.0]),
                duration_secs: 2.0,
                start_secs: Some(12.0),
                end_secs: Some(14.0),
            },
        ];
        let exemplars1 = vec![
            Exemplar {
                embedding: emb(&[3.0, 0.0, 0.0, 0.0]),
                duration_secs: 1.5,
                start_secs: Some(20.0),
                end_secs: Some(21.5),
            },
            Exemplar {
                embedding: emb(&[4.0, 0.0, 0.0, 0.0]),
                duration_secs: 2.5,
                start_secs: Some(22.0),
                end_secs: Some(24.5),
            },
        ];
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
        // Enroll only SPEAKER_00 -> 2 prototypes retaining meeting_id, but cache count must exclude them
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 2);
        assert_eq!(
            stats.cache_count, 2,
            "only unassigned SPEAKER_01 caches count"
        );
    }

    #[tokio::test]
    async fn voiceprint_storage_stats_separates_audio_bytes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let clip = vec![7u8; 100];
        insert_prototype_with_clip(&pool, "emb-1", &alice.id, "m1", Some(&clip)).await;
        insert_prototype_with_clip(&pool, "emb-2", &alice.id, "m1", None).await;

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 2);
        assert_eq!(stats.clip_count, 1, "only one row carries a clip");
        assert_eq!(stats.audio_bytes, 100, "audio bytes counted separately");
        // Embedding bytes unchanged: 2 rows x 192 f32 x 4 bytes.
        assert_eq!(stats.total_bytes, 2 * 192 * 4);

        // Blob round-trips through the audio accessor.
        let loaded = SpeakerRepository::get_voiceprint_audio(&pool, "emb-1")
            .await
            .unwrap()
            .expect("clip present");
        assert_eq!(loaded.0, clip);
        assert_eq!(loaded.1, "opus");
        assert!(
            SpeakerRepository::get_voiceprint_audio(&pool, "emb-2")
                .await
                .unwrap()
                .is_none(),
            "legacy row has no clip"
        );

        // Meeting-cache bulk verify.
        insert_meeting(&pool, "m2").await;
        let exemplars: Vec<Exemplar> = vec![Exemplar {
            embedding: emb(&[9.0; 4]),
            duration_secs: 2.0,
            start_secs: Some(1.0),
            end_secs: Some(3.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool, "m2", "SPEAKER_01", "system", &emb(&[9.0; 4]), &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        assert_eq!(
            SpeakerRepository::verify_meeting_caches(&pool, "m2").await.unwrap(),
            1
        );
    }
}
