//! Voiceprint browser: listing, rejection, reconfirmation, verification,
//! clip playback and bulk removal of voiceprints or unconfirmed caches.

use super::SpeakerRepository;
use crate::database::models::{bytes_to_embedding, SpeakerEmbedding};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};

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
    /// Whether the row carries a self-contained voice clip (blob playback).
    pub has_audio: bool,
    /// 0 = unverified, 1 = user-confirmed the voice is correct.
    #[sqlx(default)]
    pub is_verified: i64,
    pub created_at: crate::database::models::DateTimeUtc,
    /// Computed on read for enrolled prototypes (never stored): the
    /// prototype looks foreign to its owner (guard-prototype-enrollment).
    #[sqlx(default)]
    pub suspect: bool,
    /// Cosine to the mean of the owner's other prototypes, when assessed.
    #[sqlx(default)]
    pub own_similarity: Option<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpeakerVoiceprints {
    pub speaker_id: String,
    pub speaker_name: String,
    pub is_me: bool,
    pub prototype_count: usize,
    pub unverified_count: usize,
    pub suspect_count: usize,
    pub prototypes: Vec<VoiceprintRow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MeetingVoiceprints {
    pub meeting_id: String,
    pub meeting_title: String,
    pub unverified_count: usize,
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
pub struct ClearAllResult {
    pub deleted_prototypes: i64,
    pub deleted_caches: i64,
    pub total_deleted: i64,
}

/// Result of a bulk purge of the unconfirmed cache layer: how many cache rows
/// were removed and the storage they reclaimed (embeddings and stored clips).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PurgeUnconfirmedCachesResult {
    pub deleted_caches: i64,
    pub deleted_embedding_bytes: i64,
    pub deleted_clip_count: i64,
    pub deleted_clip_bytes: i64,
}

impl SpeakerRepository {
    // ===== Voiceprint browser (voiceprint-provenance) =====

    /// List voiceprints grouped by speaker (prototypes) and by meeting (unconfirmed caches).
    /// - When `speaker_id_filter` is Some, only that speaker's prototypes are returned.
    /// - When `unconfirmed_only` is true, the `speakers` branch is omitted.
    ///
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
            let speakers: Vec<crate::database::models::Speaker> = if let Some(sid) =
                speaker_id_filter
            {
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

            // Suspect flags are computed from every owner's prototypes (the
            // nearer-to-another-speaker rule needs the others), then applied
            // to the speakers listed here. Enhanced family only, like
            // recognition.
            let assessed = {
                let rows: Vec<(String, String, Vec<u8>)> = sqlx::query_as(
                    "SELECT id, speaker_id, embedding FROM speaker_embeddings
                     WHERE speaker_id IS NOT NULL AND model = ?",
                )
                .bind(crate::audio::embedder::ENHANCED_MODEL_TAG)
                .fetch_all(pool)
                .await?;
                let items: Vec<(String, String, Vec<f32>)> = rows
                    .into_iter()
                    .map(|(id, owner, e)| (id, owner, bytes_to_embedding(&e)))
                    .collect();
                crate::database::repositories::enrollment_guard::assess_prototypes(&items)
            };

            for sp in speakers {
                let mut rows: Vec<VoiceprintRow> = sqlx::query_as::<_, VoiceprintRow>(
                    "SELECT se.id, se.model, se.channel, se.duration_secs, se.speaker_id, se.meeting_id, se.cluster_label, se.audio_start_time, se.audio_end_time, COALESCE(m.title, CASE WHEN se.meeting_id IS NOT NULL THEN 'deleted meeting' ELSE NULL END) as meeting_title, (se.audio_blob IS NOT NULL) as has_audio, se.is_verified, se.created_at
                     FROM speaker_embeddings se LEFT JOIN meetings m ON m.id = se.meeting_id
                     WHERE se.speaker_id = ?
                     ORDER BY se.created_at DESC",
                )
                .bind(&sp.id)
                .fetch_all(pool)
                .await?;
                for r in rows.iter_mut() {
                    if let Some(a) = assessed.get(&r.id) {
                        r.suspect = a.suspect;
                        r.own_similarity = Some(a.own_similarity);
                    }
                }
                let count = rows.len();
                let unverified = rows.iter().filter(|r| r.is_verified == 0).count();
                let suspect = rows.iter().filter(|r| r.suspect).count();
                speakers_out.push(SpeakerVoiceprints {
                    speaker_id: sp.id,
                    speaker_name: sp.name,
                    is_me: sp.is_me,
                    prototype_count: count,
                    unverified_count: unverified,
                    suspect_count: suspect,
                    prototypes: rows,
                });
            }
        }

        // Unconfirmed caches grouped by meeting, unless filtered to a single speaker
        if speaker_id_filter.is_none() {
            let cache_rows: Vec<VoiceprintRow> = sqlx::query_as::<_, VoiceprintRow>(
                "SELECT se.id, se.model, se.channel, se.duration_secs, se.speaker_id, se.meeting_id, se.cluster_label, se.audio_start_time, se.audio_end_time, COALESCE(m.title, 'deleted meeting') as meeting_title, (se.audio_blob IS NOT NULL) as has_audio, se.is_verified, se.created_at
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
                let title = row
                    .meeting_title
                    .clone()
                    .unwrap_or_else(|| "deleted meeting".to_string());
                grouped
                    .entry(mid.clone())
                    .or_insert_with(|| (title.clone(), Vec::new()))
                    .1
                    .push(row);
                // ensure title updated if first row had fallback
                if let Some(entry) = grouped.get_mut(&mid) {
                    if entry.0 == "deleted meeting" && title != "deleted meeting" {
                        entry.0 = title;
                    }
                }
            }
            for (mid, (title, caches)) in grouped {
                let unverified = caches.iter().filter(|r| r.is_verified == 0).count();
                unconfirmed_out.push(MeetingVoiceprints {
                    meeting_id: mid,
                    meeting_title: title,
                    unverified_count: unverified,
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
                    let res =
                        sqlx::query("UPDATE speaker_embeddings SET speaker_id = NULL WHERE id = ?")
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
            let (cnt,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                    .bind(sid)
                    .fetch_one(pool)
                    .await?;
            cnt
        } else {
            0
        };

        Ok(RejectResult {
            speaker_id,
            remaining_prototypes: remaining,
        })
    }

    /// Reconfirm a cache (or demoted) voiceprint as a speaker's prototype.
    /// Enforces per-person cap (reuse enforce_prototype_cap), no provenance change.
    /// The row becomes unverified for its new owner until explicitly verified.
    /// Never refused for low similarity - the user has decided (design D4) -
    /// but the row's cosine to the mean of the target's existing prototypes is
    /// returned (None when the target holds fewer than two) so the UI can warn.
    pub async fn reconfirm_voiceprint(
        pool: &SqlitePool,
        id: &str,
        speaker_id: &str,
    ) -> Result<Option<f32>, SqlxError> {
        let mut tx = pool.begin().await?;
        let row_emb: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT embedding FROM speaker_embeddings WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let target_embs: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT embedding FROM speaker_embeddings WHERE speaker_id = ? AND id <> ?",
        )
        .bind(speaker_id)
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let similarity = row_emb.and_then(|(row,)| {
            if target_embs.len() < 2 {
                return None;
            }
            let row = bytes_to_embedding(&row);
            let dim = row.len();
            let mut mean = vec![0.0f32; dim];
            for (b,) in &target_embs {
                let e = crate::audio::diarization::identity::matching::l2_normalize(
                    &bytes_to_embedding(b),
                );
                for (m, x) in mean.iter_mut().zip(&e) {
                    *m += x;
                }
            }
            Some(crate::audio::diarization::identity::matching::cosine_similarity(&row, &mean))
        });
        let rows =
            sqlx::query("UPDATE speaker_embeddings SET speaker_id = ?, is_verified = 0, verified_at = NULL WHERE id = ?")
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
        Ok(similarity)
    }

    /// Mark a single voiceprint as user-verified (voice is correct).
    /// Display-only flag: touches nothing but `is_verified`/`verified_at`.
    /// Returns true when the row existed.
    pub async fn verify_voiceprint(pool: &SqlitePool, id: &str) -> Result<bool, SqlxError> {
        let now = Utc::now();
        let rows = sqlx::query(
            "UPDATE speaker_embeddings SET is_verified = 1, verified_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected() > 0)
    }

    /// Mark every voiceprint of a speaker (or every cache of a meeting when
    /// `speaker_id` is None and `meeting_id` is Some) as verified.
    /// Returns the number of rows updated.
    pub async fn verify_speaker(
        pool: &SqlitePool,
        speaker_id: &str,
    ) -> Result<u64, SqlxError> {
        let now = Utc::now();
        let rows = sqlx::query(
            "UPDATE speaker_embeddings SET is_verified = 1, verified_at = ? WHERE speaker_id = ? AND is_verified = 0",
        )
        .bind(now)
        .bind(speaker_id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected())
    }

    /// Mark every unconfirmed cache of a meeting as verified.
    /// Returns the number of rows updated.
    pub async fn verify_meeting_caches(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<u64, SqlxError> {
        let now = Utc::now();
        let rows = sqlx::query(
            "UPDATE speaker_embeddings SET is_verified = 1, verified_at = ? WHERE meeting_id = ? AND speaker_id IS NULL AND is_verified = 0",
        )
        .bind(now)
        .bind(meeting_id)
        .execute(pool)
        .await?;
        Ok(rows.rows_affected())
    }

    /// Load a voiceprint's stored audio clip. Returns the raw bytes plus the
    /// codec tag, or `None` when the row has no clip (legacy row).
    pub async fn get_voiceprint_audio(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<(Vec<u8>, String)>, SqlxError> {
        let row: Option<(Option<Vec<u8>>, Option<String>)> = sqlx::query_as(
            "SELECT audio_blob, audio_codec FROM speaker_embeddings WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.and_then(|(blob, codec)| {
            blob.map(|b| (b, codec.unwrap_or_else(|| "opus".to_string())))
        }))
    }

    /// Bulk removal of all voiceprints and cached embeddings.
    /// Deletes every row from `speaker_embeddings` (both prototypes with
    /// `speaker_id` set and unassigned caches with `meeting_id`/`cluster_label`),
    /// returning counts of what was removed. The `speakers` registry itself is
    /// NOT deleted. Callers should also clear any in-memory `PrototypeStore`
    /// after this completes.
    pub async fn clear_all_voiceprints(pool: &SqlitePool) -> Result<ClearAllResult, SqlxError> {
        let proto: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NOT NULL")
                .fetch_one(pool)
                .await?;
        let caches: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL AND meeting_id IS NOT NULL",
        )
        .fetch_one(pool)
        .await?;
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings")
            .fetch_one(pool)
            .await?;
        sqlx::query("DELETE FROM speaker_embeddings")
            .execute(pool)
            .await?;
        Ok(ClearAllResult {
            deleted_prototypes: proto.0,
            deleted_caches: caches.0,
            total_deleted: total.0,
        })
    }

    /// Bulk removal of the unconfirmed cache layer only. Deletes every
    /// `speaker_embeddings` row owned by a meeting cluster (`speaker_id IS NULL`),
    /// leaving enrolled prototypes, the speaker registry, cluster bindings and
    /// their centroids, expected-speaker allowlists, and transcript overrides
    /// untouched. The aggregates are read inside the same transaction as the
    /// delete, so the reported totals describe exactly the removed rows.
    /// Purged caches are reproducible only by re-running diarization.
    pub async fn purge_unconfirmed_caches(
        pool: &SqlitePool,
    ) -> Result<PurgeUnconfirmedCachesResult, SqlxError> {
        let mut tx = pool.begin().await?;

        let (deleted_caches, deleted_embedding_bytes, deleted_clip_count, deleted_clip_bytes): (
            i64,
            i64,
            i64,
            i64,
        ) = sqlx::query_as(
            "SELECT COUNT(*),
                    COALESCE(SUM(LENGTH(embedding)), 0),
                    COUNT(audio_blob),
                    COALESCE(SUM(LENGTH(audio_blob)), 0)
             FROM speaker_embeddings
             WHERE speaker_id IS NULL",
        )
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM speaker_embeddings WHERE speaker_id IS NULL")
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(PurgeUnconfirmedCachesResult {
            deleted_caches,
            deleted_embedding_bytes,
            deleted_clip_count,
            deleted_clip_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;
    use super::*;
    use crate::database::models::{embedding_to_bytes, MeetingSpeaker};

    #[tokio::test]
    async fn list_voiceprints_grouped_shapes() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_meeting(&pool, "m2").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let exemplars = vec![Exemplar {
            embedding: emb(&[1.0, 0.0, 0.0, 0.0]),
            duration_secs: 1.0,
            start_secs: Some(5.0),
            end_secs: Some(6.0),
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
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m2",
            "SPEAKER_01",
            "system",
            &emb(&[0.0; 4]),
            &exemplars,
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();

        let browser = SpeakerRepository::list_voiceprints(&pool, None, false, None, None)
            .await
            .unwrap();
        // One speaker with 1 prototype
        assert_eq!(browser.speakers.len(), 1);
        assert_eq!(browser.speakers[0].prototype_count, 1);
        assert_eq!(
            browser.speakers[0].prototypes[0].meeting_id.as_deref(),
            Some("m1")
        );
        assert_eq!(
            browser.speakers[0].prototypes[0].audio_start_time,
            Some(5.0)
        );
        // Unconfirmed branch grouped by meeting (m2 only, m1's prototypes not counted)
        assert_eq!(browser.unconfirmed.len(), 1);
        assert_eq!(browser.unconfirmed[0].meeting_id, "m2");
        assert_eq!(browser.unconfirmed[0].caches.len(), 1);

        // Filtered by speaker
        let filtered =
            SpeakerRepository::list_voiceprints(&pool, Some(&alice.id), false, None, None)
                .await
                .unwrap();
        assert_eq!(filtered.speakers.len(), 1);
        assert!(
            filtered.unconfirmed.is_empty(),
            "speaker filter must not include unconfirmed"
        );

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
        let browser2 = SpeakerRepository::list_voiceprints(&pool, None, false, None, None)
            .await
            .unwrap();
        let alice_rows = browser2
            .speakers
            .iter()
            .find(|s| s.speaker_id == alice.id)
            .unwrap();
        assert!(
            alice_rows
                .prototypes
                .iter()
                .any(|r| r.meeting_id.is_none() && r.audio_start_time.is_none()),
            "legacy row must have NULL provenance"
        );
    }

    #[tokio::test]
    async fn reject_demote_and_reconfirm_enforces_cap() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let exemplars: Vec<Exemplar> = (0..3)
            .map(|i| Exemplar {
                embedding: emb(&[1.0 + i as f32, 0.0, 0.0, 0.0]),
                duration_secs: i as f64 + 1.0,
                start_secs: Some(i as f32 * 10.0),
                end_secs: Some(i as f32 * 10.0 + 1.0),
            })
            .collect();
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
        let rows: Vec<SpeakerEmbedding> = sqlx::query_as::<_, SpeakerEmbedding>("SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at FROM speaker_embeddings WHERE speaker_id = ? ORDER BY created_at DESC")
            .bind(&alice.id)
            .fetch_all(&pool)
            .await
            .unwrap();
        let first_id = rows[0].id.clone();
        // Demote (not permanent) -> should become unconfirmed cache
        let res = SpeakerRepository::reject_voiceprint(&pool, &first_id, false)
            .await
            .unwrap();
        assert_eq!(res.speaker_id.as_deref(), Some(alice.id.as_str()));
        let remaining: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining.0, 2);
        // Must be excluded from recognition
        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
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
        SpeakerRepository::reconfirm_voiceprint(&pool, &first_id, &alice.id)
            .await
            .unwrap();
        let protos2 = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            SPEAKER_EMBEDDING_MODEL,
        )
        .await
        .unwrap();
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
        SpeakerRepository::reconfirm_voiceprint(&pool, &cache_id, &alice.id)
            .await
            .unwrap();
        let cnt: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(cnt.0 <= PER_PERSON_PROTOTYPE_CAP as i64);
    }

    #[tokio::test]
    async fn voiceprint_clip_columns_default_to_legacy_without_audio_file() {
        let pool = setup_pool().await;
        // Meeting without folder_path: no audio file resolvable, so clips
        // must be absent and rows must read as legacy (unverified, no clip).
        insert_meeting(&pool, "m1").await;
        let exemplars: Vec<Exemplar> = vec![Exemplar {
            embedding: emb(&[1.0, 2.0, 3.0, 4.0]),
            duration_secs: 3.0,
            start_secs: Some(10.0),
            end_secs: Some(13.0),
        }];
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[1.0, 2.0, 3.0, 4.0]),
            &exemplars,
            "titanet_large",
        )
        .await
        .unwrap();

        let row: SpeakerEmbedding = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at FROM speaker_embeddings WHERE meeting_id = 'm1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.audio_blob.is_none(), "no clip without an audio file");
        assert_eq!(row.is_verified, 0, "new rows start unverified");

        let browser = SpeakerRepository::list_voiceprints(&pool, None, false, None, None)
            .await
            .unwrap();
        assert!(!browser.unconfirmed[0].caches[0].has_audio);
        assert_eq!(browser.unconfirmed[0].caches[0].is_verified, 0);
        assert_eq!(browser.unconfirmed[0].unverified_count, 1);
    }

    #[tokio::test]
    async fn voiceprint_verify_flag_transitions() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let clip = vec![0x4Fu8, 0x67, 0x67, 0x53];
        insert_prototype_with_clip(&pool, "emb-1", &alice.id, "m1", Some(&clip)).await;
        insert_prototype_with_clip(&pool, "emb-2", &alice.id, "m1", None).await;

        // Verify sets only the flag + timestamp.
        assert!(SpeakerRepository::verify_voiceprint(&pool, "emb-1").await.unwrap());
        assert!(!SpeakerRepository::verify_voiceprint(&pool, "emb-missing").await.unwrap());
        let row: SpeakerEmbedding = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at FROM speaker_embeddings WHERE id = 'emb-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.is_verified, 1);
        assert!(row.verified_at.is_some());
        assert_eq!(row.audio_blob, Some(clip), "clip untouched by verify");
        assert_eq!(row.speaker_id.as_deref(), Some(alice.id.as_str()));

        // Bulk verify covers the rest; already-verified rows are skipped.
        assert_eq!(SpeakerRepository::verify_speaker(&pool, &alice.id).await.unwrap(), 1);
        assert_eq!(SpeakerRepository::verify_speaker(&pool, &alice.id).await.unwrap(), 0);

        // Reconfirm resets verification for the new owner.
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        SpeakerRepository::reconfirm_voiceprint(&pool, "emb-1", &bob.id).await.unwrap();
        let row: SpeakerEmbedding = sqlx::query_as::<_, SpeakerEmbedding>(
            "SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at FROM speaker_embeddings WHERE id = 'emb-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.is_verified, 0, "reconfirm resets verification");
        assert!(row.verified_at.is_none());
        assert_eq!(row.speaker_id.as_deref(), Some(bob.id.as_str()));

        // Browser surfaces per-group unverified counts.
        let browser = SpeakerRepository::list_voiceprints(&pool, None, false, None, None)
            .await
            .unwrap();
        let bob_group = browser.speakers.iter().find(|s| s.speaker_id == bob.id).unwrap();
        assert_eq!(bob_group.unverified_count, 1);
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_deletes_only_caches() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        for i in 0..3 {
            insert_prototype_with_clip(&pool, &format!("proto-{i}"), &alice.id, "m1", None).await;
        }
        for i in 0..4 {
            insert_cache_row(
                &pool,
                &format!("cache-{i}"),
                "m1",
                "SPEAKER_00",
                &[i as f32; 4],
                None,
            )
            .await;
        }

        let before = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(before.prototype_count, 3);
        assert_eq!(before.cache_count, 4);

        let res = SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();
        assert_eq!(res.deleted_caches, 4);

        let after = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(after.prototype_count, 3, "prototypes must survive the purge");
        assert_eq!(after.cache_count, 0, "every cache row must be removed");

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            "titanet_large",
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), 3, "prototypes still load for recognition");
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_preserves_bindings_and_overrides() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        let centroid = embedding_to_bytes(&[7.0f32; 192]);
        sqlx::query(
            "INSERT INTO meeting_speakers (meeting_id, cluster_label, speaker_id, centroid, channel, matched_by, match_score)
             VALUES ('m1', 'SPEAKER_00', ?, ?, 'mic', 'auto', 0.81)",
        )
        .bind(&alice.id)
        .bind(&centroid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO meeting_expected_speakers (meeting_id, speaker_id) VALUES ('m1', ?)")
            .bind(&alice.id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, speaker_override_id)
             VALUES ('t1', 'm1', 'text', '2026-01-01T00:00:00Z', 'SPEAKER_00', ?)",
        )
        .bind(&alice.id)
        .execute(&pool)
        .await
        .unwrap();

        for i in 0..3 {
            insert_cache_row(
                &pool,
                &format!("cache-{i}"),
                "m1",
                "SPEAKER_00",
                &[i as f32; 4],
                None,
            )
            .await;
        }

        SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();

        // Registry speaker intact.
        assert_eq!(SpeakerRepository::list_speakers(&pool).await.unwrap().len(), 1);

        // Cluster binding, centroid, and provenance intact.
        let binding: MeetingSpeaker = sqlx::query_as::<_, MeetingSpeaker>(
            "SELECT meeting_id, cluster_label, speaker_id, centroid, channel, matched_by, match_score
             FROM meeting_speakers WHERE meeting_id = 'm1' AND cluster_label = 'SPEAKER_00'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(binding.speaker_id.as_deref(), Some(alice.id.as_str()));
        assert_eq!(binding.centroid.as_deref(), Some(centroid.as_slice()));
        assert_eq!(binding.channel.as_deref(), Some("mic"));
        assert_eq!(binding.matched_by.as_deref(), Some("auto"));
        assert_eq!(binding.match_score, Some(0.81));

        // Allowlist intact.
        let expected = SpeakerRepository::get_expected_speakers(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(expected, vec![alice.id.clone()]);

        // Per-transcript override intact.
        let override_id: Option<String> =
            sqlx::query_scalar("SELECT speaker_override_id FROM transcripts WHERE id = 't1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(override_id.as_deref(), Some(alice.id.as_str()));

        // Re-match still runs from the cached centroid.
        let centroids = SpeakerRepository::get_cluster_centroids(&pool, "m1")
            .await
            .unwrap();
        assert_eq!(centroids.len(), 1);
        assert_eq!(centroids[0].centroid, vec![7.0f32; 192]);
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_reports_reclaimed_storage() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // A prototype clip must survive and stay out of the reported totals.
        let proto_clip = vec![1u8; 50];
        insert_prototype_with_clip(&pool, "proto-1", &alice.id, "m1", Some(&proto_clip)).await;

        let clip_a = vec![2u8; 100];
        let clip_b = vec![3u8; 25];
        insert_cache_row(&pool, "cache-a", "m1", "SPEAKER_00", &[1.0; 192], Some(&clip_a)).await;
        insert_cache_row(&pool, "cache-b", "m1", "SPEAKER_01", &[2.0; 192], Some(&clip_b)).await;
        insert_cache_row(&pool, "cache-c", "m1", "SPEAKER_02", &[3.0; 192], None).await;

        let res = SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();
        assert_eq!(res.deleted_caches, 3);
        assert_eq!(
            res.deleted_embedding_bytes,
            3 * 192 * 4,
            "three 192-d f32 embeddings"
        );
        assert_eq!(res.deleted_clip_count, 2, "only rows carrying a clip count");
        assert_eq!(res.deleted_clip_bytes, 125);

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 1);
        assert_eq!(stats.clip_count, 1);
        assert_eq!(stats.audio_bytes, 50);
        let loaded = SpeakerRepository::get_voiceprint_audio(&pool, "proto-1")
            .await
            .unwrap()
            .expect("prototype clip survives");
        assert_eq!(loaded.0, proto_clip);
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_on_empty_layer_is_a_noop() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        insert_prototype_with_clip(&pool, "proto-1", &alice.id, "m1", None).await;

        let res = SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();
        assert_eq!(res.deleted_caches, 0);
        assert_eq!(res.deleted_embedding_bytes, 0);
        assert_eq!(res.deleted_clip_count, 0);
        assert_eq!(res.deleted_clip_bytes, 0);

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(stats.prototype_count, 1);
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_keeps_enrollment_semantics() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_meeting(&pool, "m2").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // m1: a fully enrolled cluster (prototypes keep m1 provenance).
        let enrolled_exemplars: Vec<Exemplar> = (0..4)
            .map(|i| Exemplar {
                embedding: emb(&[10.0 + i as f32, 1.0, 0.0, 0.0]),
                duration_secs: 5.0 + i as f64,
                start_secs: Some(i as f32),
                end_secs: Some(i as f32 + 5.0),
            })
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m1",
            "SPEAKER_00",
            "mic",
            &emb(&[9.0; 4]),
            &enrolled_exemplars,
            "titanet_large",
        )
        .await
        .unwrap();
        assert_eq!(
            SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
                .await
                .unwrap(),
            4
        );

        // m2: a cluster still sitting as caches only.
        let pending_exemplars: Vec<Exemplar> = (0..3)
            .map(|i| Exemplar {
                embedding: emb(&[10.0 + i as f32, 2.0, 0.0, 0.0]),
                duration_secs: 4.0 + i as f64,
                start_secs: Some(i as f32),
                end_secs: Some(i as f32 + 4.0),
            })
            .collect();
        SpeakerRepository::write_cluster_cache(
            &pool,
            "m2",
            "SPEAKER_01",
            "mic",
            &emb(&[8.0; 4]),
            &pending_exemplars,
            "titanet_large",
        )
        .await
        .unwrap();

        let res = SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();
        assert_eq!(res.deleted_caches, 3);

        // Enrollment from a purged cluster is a documented no-op, not an error.
        // (`enroll_cluster` returns the speaker's prototype count, so a fresh
        // speaker must stay at zero.)
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        assert_eq!(
            SpeakerRepository::enroll_cluster(&pool, "m2", "SPEAKER_01", &bob.id)
                .await
                .unwrap(),
            0,
            "no caches left to enroll"
        );

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            "titanet_large",
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), 4, "pre-existing prototypes still load");
    }

    #[tokio::test]
    async fn purge_unconfirmed_caches_keeps_prototypes_with_live_provenance() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();

        // 12 exemplars: best K=8 enroll, 4 stay as caches in the SAME meeting the
        // prototypes point at, so prototype provenance outlives the purge.
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
            &emb(&[99.0; 4]),
            &exemplars,
            "titanet_large",
        )
        .await
        .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();

        let before = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(before.prototype_count, ENROLLMENT_BEST_K as i64);
        assert_eq!(before.cache_count, 4);

        let res = SpeakerRepository::purge_unconfirmed_caches(&pool)
            .await
            .unwrap();
        assert_eq!(res.deleted_caches, 4);

        let stats = SpeakerRepository::storage_stats(&pool).await.unwrap();
        assert_eq!(
            stats.prototype_count, ENROLLMENT_BEST_K as i64,
            "provenanced prototypes survive the purge"
        );
        assert_eq!(stats.cache_count, 0);

        let protos = SpeakerRepository::load_prototypes(
            &pool,
            Some(std::slice::from_ref(&alice.id)),
            "titanet_large",
        )
        .await
        .unwrap();
        assert_eq!(protos.len(), ENROLLMENT_BEST_K);

        // Prototypes still point at the meeting whose caches were purged.
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
        }
    }

    #[tokio::test]
    async fn reconfirm_accepts_a_low_similarity_row_and_reports_the_similarity() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        // Alice holds four coherent prototypes; a foreign-sounding cache row
        // (the longest, so it is never part of the cluster enrollment below).
        cache_with_outliers(&pool, 4, &[([0.0, 0.0, 1.0, 0.0], 50.0)]).await;
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        let outlier_id: (String,) = sqlx::query_as(
            "SELECT id FROM speaker_embeddings WHERE speaker_id IS NULL AND duration_secs >= 50.0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let similarity = SpeakerRepository::reconfirm_voiceprint(&pool, &outlier_id.0, &alice.id)
            .await
            .unwrap()
            .expect("target holds enough prototypes to compare");
        assert!(similarity < crate::database::repositories::enrollment_guard::COHERENCE_THRESHOLD, "got {similarity}");

        let owner: (Option<String>,) =
            sqlx::query_as("SELECT speaker_id FROM speaker_embeddings WHERE id = ?")
                .bind(&outlier_id.0)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(owner.0.as_deref(), Some(alice.id.as_str()), "the user's decision stands");
    }

    #[tokio::test]
    async fn reconfirm_into_a_speaker_with_few_prototypes_reports_no_similarity() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        cache_with_outliers(&pool, 0, &[([1.0, 0.0, 0.0, 0.0], 5.0)]).await;
        let id: (String,) =
            sqlx::query_as("SELECT id FROM speaker_embeddings WHERE speaker_id IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        let similarity = SpeakerRepository::reconfirm_voiceprint(&pool, &id.0, &alice.id)
            .await
            .unwrap();
        assert_eq!(similarity, None);
    }

    #[tokio::test]
    async fn list_voiceprints_flags_a_foreign_prototype_without_writing_anything() {
        let enhanced = crate::audio::embedder::ENHANCED_MODEL_TAG;
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        insert_meeting(&pool, "m2").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        let mk = |base: [f32; 4], n: usize| -> Vec<Exemplar> {
            (0..n)
                .map(|i| Exemplar {
                    embedding: emb(&[
                        base[0] * (10.0 + i as f32),
                        base[1] * (10.0 + i as f32),
                        base[2] * (10.0 + i as f32),
                        base[3] * (10.0 + i as f32),
                    ]),
                    duration_secs: (i + 1) as f64,
                    start_secs: Some(10.0 * i as f32),
                    end_secs: Some(10.0 * i as f32 + 1.0),
                })
                .collect()
        };
        let mut alice_ex = mk([1.0, 0.1, 0.0, 0.0], 5);
        // A row that sounds like Bob, filed under Alice by hand.
        alice_ex.push(Exemplar {
            embedding: emb(&[0.0, 0.1, 12.0, 0.0]),
            duration_secs: 50.0,
            start_secs: Some(500.0),
            end_secs: Some(501.0),
        });
        for (meeting, ex) in [("m1", &alice_ex), ("m2", &mk([0.0, 0.1, 1.0, 0.0], 5))] {
            SpeakerRepository::write_cluster_cache(
                &pool,
                meeting,
                "SPEAKER_00",
                "mic",
                &emb(&[1.0, 0.0, 0.0, 0.0]),
                ex,
                enhanced,
            )
            .await
            .unwrap();
        }
        SpeakerRepository::enroll_cluster(&pool, "m1", "SPEAKER_00", &alice.id)
            .await
            .unwrap();
        SpeakerRepository::enroll_cluster(&pool, "m2", "SPEAKER_00", &bob.id)
            .await
            .unwrap();
        let odd: (String,) = sqlx::query_as(
            "SELECT id FROM speaker_embeddings WHERE speaker_id IS NULL AND duration_secs >= 50.0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        SpeakerRepository::reconfirm_voiceprint(&pool, &odd.0, &alice.id)
            .await
            .unwrap();

        let snapshot = |pool: SqlitePool| async move {
            sqlx::query_as::<_, (String, Option<String>, i64)>(
                "SELECT id, speaker_id, is_verified FROM speaker_embeddings ORDER BY id",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        let before = snapshot(pool.clone()).await;
        let browser = SpeakerRepository::list_voiceprints(&pool, None, false, None, None)
            .await
            .unwrap();
        assert_eq!(before, snapshot(pool.clone()).await, "listing writes nothing");

        let a = browser.speakers.iter().find(|s| s.speaker_id == alice.id).unwrap();
        let b = browser.speakers.iter().find(|s| s.speaker_id == bob.id).unwrap();
        assert_eq!(a.suspect_count, 1);
        assert_eq!(b.suspect_count, 0);
        assert!(a.prototypes.iter().find(|r| r.id == odd.0).unwrap().suspect);
        assert!(a.prototypes.iter().filter(|r| r.id != odd.0).all(|r| !r.suspect));
        assert!(
            browser
                .unconfirmed
                .iter()
                .flat_map(|m| &m.caches)
                .all(|r| !r.suspect && r.own_similarity.is_none()),
            "cache rows are never assessed"
        );
    }
}
