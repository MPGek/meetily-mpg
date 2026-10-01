//! Shared fixtures for the speaker repository tests.

use super::{Exemplar, SpeakerRepository, SPEAKER_EMBEDDING_MODEL};
use crate::database::models::embedding_to_bytes;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

pub(super) async fn setup_pool() -> SqlitePool {
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

pub(super) async fn insert_meeting(pool: &SqlitePool, id: &str) {
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

pub(super) fn emb(vals: &[f32]) -> Vec<f32> {
    vals.to_vec()
}

pub(super) async fn insert_transcript_window(
    pool: &SqlitePool,
    id: &str,
    meeting_id: &str,
    speaker: Option<&str>,
    start: f64,
    end: f64,
    source_device: &str,
) {
    sqlx::query(
        "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, speaker, audio_start_time, audio_end_time, source_device)
         VALUES (?, ?, 'text', '2026-01-01T00:00:00Z', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(meeting_id)
    .bind(speaker)
    .bind(start)
    .bind(end)
    .bind(source_device)
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_transcript(pool: &SqlitePool, id: &str, meeting_id: &str, speaker: &str) {
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

pub(super) async fn insert_prototype_with_clip(
    pool: &SqlitePool,
    id: &str,
    speaker_id: &str,
    meeting_id: &str,
    clip: Option<&[u8]>,
) {
    sqlx::query(
        "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at)
         VALUES (?, ?, 'titanet_large', 'mic', 3.0, ?, ?, 'SPEAKER_00', 10.0, 13.0, ?, ?, ?, 0, NULL, '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(embedding_to_bytes(&[0.5f32; 192]))
    .bind(speaker_id)
    .bind(meeting_id)
    .bind(clip)
    .bind(clip.map(|_| "opus"))
    .bind(clip.map(|_| 16000i64))
    .execute(pool)
    .await
    .unwrap();
}

/// Insert an unassigned cache row (meeting-owned) with an optional clip.
pub(super) async fn insert_cache_row(
    pool: &SqlitePool,
    id: &str,
    meeting_id: &str,
    cluster_label: &str,
    embedding: &[f32],
    clip: Option<&[u8]>,
) {
    sqlx::query(
        "INSERT INTO speaker_embeddings (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, audio_blob, audio_codec, audio_sample_rate, is_verified, verified_at, created_at)
         VALUES (?, ?, 'titanet_large', 'mic', 3.0, NULL, ?, ?, 10.0, 13.0, ?, ?, ?, 0, NULL, '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(embedding_to_bytes(embedding))
    .bind(meeting_id)
    .bind(cluster_label)
    .bind(clip)
    .bind(clip.map(|_| "opus"))
    .bind(clip.map(|_| 16000i64))
    .execute(pool)
    .await
    .unwrap();
}

/// A cluster cache of `coherent` collinear exemplars (durations 1..=n,
/// windows 10s apart) plus the given `(embedding, duration)` outliers.
pub(super) async fn cache_with_outliers(
    pool: &SqlitePool,
    coherent: usize,
    outliers: &[([f32; 4], f64)],
) {
    let mut ex: Vec<Exemplar> = (0..coherent)
        .map(|i| Exemplar {
            embedding: emb(&[10.0 + i as f32, 1.0, 0.0, 0.0]),
            duration_secs: (i + 1) as f64,
            start_secs: Some(10.0 * i as f32),
            end_secs: Some(10.0 * i as f32 + (i + 1) as f32),
        })
        .collect();
    for (k, (v, d)) in outliers.iter().enumerate() {
        ex.push(Exemplar {
            embedding: emb(v),
            duration_secs: *d,
            start_secs: Some(500.0 + 10.0 * k as f32),
            end_secs: Some(500.0 + 10.0 * k as f32 + *d as f32),
        });
    }
    SpeakerRepository::write_cluster_cache(
        pool,
        "m1",
        "SPEAKER_00",
        "mic",
        &emb(&[1.0, 0.0, 0.0, 0.0]),
        &ex,
        SPEAKER_EMBEDDING_MODEL,
    )
    .await
    .unwrap();
}
