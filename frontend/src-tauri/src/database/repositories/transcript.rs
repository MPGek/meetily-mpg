use crate::api::{TranscriptSearchResult, TranscriptSegment};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqlitePool};
use tracing::{error, info};
use uuid::Uuid;

pub struct TranscriptsRepository;

impl TranscriptsRepository {
    /// Saves a new meeting and its associated transcript segments.
    /// This function uses a transaction to ensure that either both the meeting
    /// and all its transcripts are saved, or none of them are.
    pub async fn save_transcript(
        pool: &SqlitePool,
        meeting_title: &str,
        transcripts: &[TranscriptSegment],
        folder_path: Option<String>,
    ) -> Result<String, SqlxError> {
        let meeting_id = format!("meeting-{}", Uuid::new_v4());

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now();

        // Recording start (change: recording-start-time): the folder's
        // metadata.json carries `created_at` from when recording began;
        // fall back to the save moment so `started_at` is never empty.
        let started_at = folder_path
            .as_deref()
            .and_then(|f| {
                crate::summary::metadata::read_recording_started_at_from_metadata(
                    std::path::Path::new(f),
                )
            })
            .unwrap_or(now);

        // 1. Create the new meeting
        let result = sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at, started_at, folder_path) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&meeting_id)
        .bind(meeting_title)
        .bind(now)
        .bind(now)
        .bind(started_at)
        .bind(&folder_path)
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
            error!("Failed to create meeting '{}': {}", meeting_title, e);
            transaction.rollback().await?;
            return Err(e);
        }

        info!("Successfully created meeting with id: {}", meeting_id);

        // 2. Save each transcript segment with audio timing fields and token timestamps (for diarization refinement)
        let mut has_speakers = false;
        for segment in transcripts {
            let transcript_id = format!("transcript-{}", Uuid::new_v4());
            // Serialize tokens if present (Word-level timestamps for diarization split)
            let tokens_json = segment
                .tokens
                .as_ref()
                .map(|v| {
                    // Value may already be array; ensure JSON string
                    if v.is_string() {
                        v.as_str().unwrap_or("").to_string()
                    } else {
                        serde_json::to_string(v).unwrap_or_default()
                    }
                })
                .filter(|s| !s.is_empty() && s != "null");
            let result = if tokens_json.is_some() {
                let res = sqlx::query(
                    "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, source_device, speaker, tokens)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&transcript_id)
                .bind(&meeting_id)
                .bind(&segment.text)
                .bind(&segment.timestamp)
                .bind(segment.audio_start_time)
                .bind(segment.audio_end_time)
                .bind(segment.duration)
                .bind(&segment.source_device)
                .bind(&segment.speaker)
                .bind(tokens_json.clone())
                .execute(&mut *transaction)
                .await;
                match res {
                    Ok(r) => Ok(r),
                    Err(e)
                        if e.to_string().contains("no such column")
                            || e.to_string().contains("has no column named") =>
                    {
                        // Pre-migration DB without tokens column: fallback without tokens
                        sqlx::query(
                            "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, source_device, speaker)
                             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
                        )
                        .bind(&transcript_id)
                        .bind(&meeting_id)
                        .bind(&segment.text)
                        .bind(&segment.timestamp)
                        .bind(segment.audio_start_time)
                        .bind(segment.audio_end_time)
                        .bind(segment.duration)
                        .bind(&segment.source_device)
                        .bind(&segment.speaker)
                        .execute(&mut *transaction)
                        .await
                    }
                    Err(e) => Err(e),
                }
            } else {
                sqlx::query(
                    "INSERT INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, source_device, speaker)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(&transcript_id)
                .bind(&meeting_id)
                .bind(&segment.text)
                .bind(&segment.timestamp)
                .bind(segment.audio_start_time)
                .bind(segment.audio_end_time)
                .bind(segment.duration)
                .bind(&segment.source_device)
                .bind(&segment.speaker)
                .execute(&mut *transaction)
                .await
            };

            if let Err(e) = result {
                error!(
                    "Failed to save transcript segment for meeting {}: {}",
                    meeting_id, e
                );
                transaction.rollback().await?;
                return Err(e);
            }
            if segment.speaker.is_some() {
                has_speakers = true;
            }
        }

        // 3. Mark the meeting as diarized when online/offline speaker labels
        // were provided with the save (online diarization writes at stop).
        if has_speakers {
            sqlx::query("UPDATE meetings SET diarization_status = 'complete' WHERE id = ?")
                .bind(&meeting_id)
                .execute(&mut *transaction)
                .await?;
        }

        info!(
            "Successfully saved {} transcript segments for meeting {}",
            transcripts.len(),
            meeting_id
        );

        // Commit the transaction
        transaction.commit().await?;

        Ok(meeting_id)
    }

    /// Searches for a query string within the transcripts.
    /// It returns a list of matching transcripts with context.
    pub async fn search_transcripts(
        pool: &SqlitePool,
        query: &str,
    ) -> Result<Vec<TranscriptSearchResult>, SqlxError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let search_query = format!("%{}%", query.to_lowercase());

        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT m.id, m.title, t.transcript, t.timestamp
             FROM meetings m
             JOIN transcripts t ON m.id = t.meeting_id
             WHERE LOWER(t.transcript) LIKE ?",
        )
        .bind(&search_query)
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|(id, title, transcript, timestamp)| {
                let match_context = Self::get_match_context(&transcript, query);
                TranscriptSearchResult {
                    id,
                    title,
                    match_context,
                    timestamp,
                }
            })
            .collect();

        Ok(results)
    }

    /// Helper function to extract a snippet of text around the first match of a query.
    fn get_match_context(transcript: &str, query: &str) -> String {
        let transcript_lower = transcript.to_lowercase();
        let query_lower = query.to_lowercase();

        match transcript_lower.find(&query_lower) {
            Some(match_index) => {
                let start_index = match_index.saturating_sub(100);
                let end_index = (match_index + query.len() + 100).min(transcript.len());

                let mut context = String::new();
                if start_index > 0 {
                    context.push_str("...");
                }
                context.push_str(&transcript[start_index..end_index]);
                if end_index < transcript.len() {
                    context.push_str("...");
                }
                context
            }
            None => transcript.chars().take(200).collect(), // Fallback to the start of the transcript
        }
    }
}

#[cfg(test)]
mod started_at_tests {
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

    async fn read_started_at(pool: &SqlitePool, meeting_id: &str) -> (Option<String>, String) {
        let row: (Option<String>, String) =
            sqlx::query_as("SELECT started_at, created_at FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_one(pool)
                .await
                .unwrap();
        row
    }

    #[tokio::test]
    async fn save_uses_metadata_start_time() {
        let pool = setup_pool().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("metadata.json"),
            serde_json::json!({ "created_at": "2026-09-11T14:00:00Z" }).to_string(),
        )
        .unwrap();

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "M",
            &[],
            Some(dir.path().to_str().unwrap().to_string()),
        )
        .await
        .unwrap();

        let (started_at, created_at) = read_started_at(&pool, &meeting_id).await;
        assert_eq!(started_at.as_deref(), Some("2026-09-11T14:00:00+00:00"));
        assert_ne!(started_at.unwrap(), created_at);
    }

    #[tokio::test]
    async fn save_falls_back_when_metadata_unreadable() {
        let pool = setup_pool().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("metadata.json"), "{").unwrap();

        let meeting_id = TranscriptsRepository::save_transcript(
            &pool,
            "M",
            &[],
            Some(dir.path().to_str().unwrap().to_string()),
        )
        .await
        .unwrap();

        let (started_at, _) = read_started_at(&pool, &meeting_id).await;
        assert!(started_at.is_some(), "fallback must never store NULL");
    }

    #[tokio::test]
    async fn save_falls_back_without_folder() {
        let pool = setup_pool().await;
        let meeting_id = TranscriptsRepository::save_transcript(&pool, "M", &[], None)
            .await
            .unwrap();

        let (started_at, _) = read_started_at(&pool, &meeting_id).await;
        assert!(started_at.is_some(), "fallback must never store NULL");
    }
}
