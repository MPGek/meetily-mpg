use crate::api::{MeetingDetails, MeetingTranscript};
use crate::database::models::{MeetingModel, Transcript};
use chrono::Utc;
use sqlx::{Connection, Error as SqlxError, SqliteConnection, SqlitePool};
use tracing::{error, info};

/// Transcript display columns with the speaker display name resolved at read
/// time: `speaker` keeps the raw cluster label; `speaker_label` becomes
/// `COALESCE(so.name, s.name, t.speaker_label)` via LEFT JOINs onto the
/// per-transcript override (design D10), then `meeting_speakers` -> `speakers`
/// (design D3). Legacy `speaker_label` is the fallback when neither a
/// per-block override nor a cluster registry binding exists.
const TRANSCRIPT_DISPLAY_SELECT: &str = "SELECT t.id, t.meeting_id, t.transcript, t.timestamp, t.summary, t.action_items, t.key_points, t.audio_start_time, t.audio_end_time, t.duration, t.source_device, t.speaker, COALESCE(so.name, s.name, t.speaker_label) AS speaker_label, CASE WHEN t.speaker_override_id IS NOT NULL THEN 'user' WHEN ms.speaker_id IS NOT NULL THEN COALESCE(ms.matched_by, 'auto') ELSE 'fallback' END AS speaker_matched_by, ms.match_score AS speaker_match_score FROM transcripts t LEFT JOIN speakers so ON so.id = t.speaker_override_id LEFT JOIN meeting_speakers ms ON ms.meeting_id = t.meeting_id AND ms.cluster_label = t.speaker LEFT JOIN speakers s ON s.id = ms.speaker_id";

pub struct MeetingsRepository;

impl MeetingsRepository {
    pub async fn get_meetings(pool: &SqlitePool) -> Result<Vec<MeetingModel>, sqlx::Error> {
        let meetings =
            sqlx::query_as::<_, MeetingModel>("SELECT * FROM meetings ORDER BY created_at DESC")
                .fetch_all(pool)
                .await?;
        Ok(meetings)
    }

    pub async fn delete_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        match delete_meeting_with_transaction(&mut transaction, meeting_id).await {
            Ok(success) => {
                if success {
                    transaction.commit().await?;
                    info!(
                        "Successfully deleted meeting {} and all associated data",
                        meeting_id
                    );
                    Ok(true)
                } else {
                    transaction.rollback().await?;
                    Ok(false)
                }
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                error!("Failed to delete meeting {}: {}", meeting_id, e);
                Err(e)
            }
        }
    }

    pub async fn get_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingDetails>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        // Get meeting details
        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path, diarization_status, speaker_names FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(&mut *transaction)
                .await?;

        if meeting.is_none() {
            transaction.rollback().await?;
            return Err(SqlxError::RowNotFound);
        }

        if let Some(meeting) = meeting {
            // Get all transcripts for this meeting with display names resolved
            // via meeting_speakers -> speakers (legacy speaker_label fallback).
            let transcripts =
                sqlx::query_as::<_, Transcript>(&format!("{} WHERE t.meeting_id = ?", TRANSCRIPT_DISPLAY_SELECT))
                    .bind(meeting_id)
                    .fetch_all(&mut *transaction)
                    .await?;

            transaction.commit().await?;

            // Convert Transcript to MeetingTranscript
            let meeting_transcripts = transcripts
                .into_iter()
                .map(|t| MeetingTranscript {
                    id: t.id,
                    text: t.transcript,
                    timestamp: t.timestamp,
                    audio_start_time: t.audio_start_time,
                    audio_end_time: t.audio_end_time,
                    duration: t.duration,
                    source_device: t.source_device,
                    speaker: t.speaker,
                    speaker_label: t.speaker_label,
                    speaker_matched_by: t.speaker_matched_by,
                    speaker_match_score: t.speaker_match_score,
                })
                .collect::<Vec<_>>();

            Ok(Some(MeetingDetails {
                id: meeting.id,
                title: meeting.title,
                created_at: meeting.created_at.0.to_rfc3339(),
                updated_at: meeting.updated_at.0.to_rfc3339(),
                transcripts: meeting_transcripts,
                diarization_status: meeting.diarization_status,
                speaker_names: meeting.speaker_names,
            }))
        } else {
            transaction.rollback().await?;
            Ok(None)
        }
    }

    /// Get meeting metadata without transcripts (for pagination)
    pub async fn get_meeting_metadata(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingModel>, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let meeting: Option<MeetingModel> =
            sqlx::query_as("SELECT id, title, created_at, updated_at, folder_path, diarization_status, speaker_names FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;

        Ok(meeting)
    }

    /// Get meeting transcripts with pagination support
    pub async fn get_meeting_transcripts_paginated(
        pool: &SqlitePool,
        meeting_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Transcript>, i64), SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        // Get total count of transcripts for this meeting
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?"
        )
        .bind(meeting_id)
        .fetch_one(pool)
        .await?;

        // Get paginated transcripts ordered by audio_start_time, with display
        // names resolved via meeting_speakers -> speakers (legacy fallback).
        let transcripts = sqlx::query_as::<_, Transcript>(
            &format!("{} WHERE t.meeting_id = ? ORDER BY t.audio_start_time ASC LIMIT ? OFFSET ?", TRANSCRIPT_DISPLAY_SELECT),
        )
        .bind(meeting_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok((transcripts, total.0))
    }

    pub async fn update_meeting_title(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() {
            return Err(SqlxError::Protocol(
                "meeting_id cannot be empty".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut transaction = conn.begin().await?;

        let now = Utc::now().naive_utc();

        let rows_affected =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
        if rows_affected.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_speaker_label(
        pool: &SqlitePool,
        meeting_id: &str,
        speaker: &str,
        label: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let rows = sqlx::query(
            "UPDATE transcripts SET speaker_label = ? WHERE meeting_id = ? AND speaker = ?"
        )
        .bind(label)
        .bind(meeting_id)
        .bind(speaker)
        .execute(&mut *transaction)
        .await?;

        if rows.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }

        let current_names: Option<String> = sqlx::query_scalar(
            "SELECT speaker_names FROM meetings WHERE id = ?"
        )
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?
        .flatten();

        let mut names_map: serde_json::Map<String, serde_json::Value> = match current_names {
            Some(json) => serde_json::from_str(&json).unwrap_or_default(),
            None => serde_json::Map::new(),
        };
        names_map.insert(speaker.to_string(), serde_json::Value::String(label.to_string()));

        let updated_json = serde_json::to_string(&names_map).unwrap_or_default();
        sqlx::query("UPDATE meetings SET speaker_names = ? WHERE id = ?")
            .bind(&updated_json)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }

    pub async fn update_diarization_status(
        pool: &SqlitePool,
        meeting_id: &str,
        status: &str,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query("UPDATE meetings SET diarization_status = ? WHERE id = ?")
            .bind(status)
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
    }

    pub async fn update_speaker_names(
        pool: &SqlitePool,
        meeting_id: &str,
        names_json: &str,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query("UPDATE meetings SET speaker_names = ? WHERE id = ?")
            .bind(names_json)
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
    }

    pub async fn update_transcript_speaker(
        pool: &SqlitePool,
        transcript_id: &str,
        speaker: &str,
    ) -> Result<bool, SqlxError> {
        let rows = sqlx::query("UPDATE transcripts SET speaker = ? WHERE id = ?")
            .bind(speaker)
            .bind(transcript_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
    }

    pub async fn get_transcripts_for_diarization(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Vec<Transcript>, SqlxError> {
        sqlx::query_as::<_, Transcript>(
            "SELECT * FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time ASC"
        )
        .bind(meeting_id)
        .fetch_all(pool)
        .await
    }

    pub async fn update_meeting_name(
        pool: &SqlitePool,
        meeting_id: &str,
        new_title: &str,
    ) -> Result<bool, SqlxError> {
        let mut transaction = pool.begin().await?;
        let now = Utc::now();

        // Update meetings table
        let meeting_update =
            sqlx::query("UPDATE meetings SET title = ?, updated_at = ? WHERE id = ?")
                .bind(new_title)
                .bind(now)
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;

        if meeting_update.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false); // Meeting not found
        }

        // Update transcript_chunks table
        sqlx::query("UPDATE transcript_chunks SET meeting_name = ? WHERE meeting_id = ?")
            .bind(new_title)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;
        Ok(true)
    }
}

async fn delete_meeting_with_transaction(
    transaction: &mut SqliteConnection,
    meeting_id: &str,
) -> Result<bool, SqlxError> {
    // Check if meeting exists
    let meeting_exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

    if meeting_exists.is_none() {
        error!("Meeting {} not found for deletion", meeting_id);
        return Ok(false);
    }

    // Delete from related tables in proper order
    // 1. Delete from transcript_chunks
    sqlx::query("DELETE FROM transcript_chunks WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 2. Delete from summary_processes
    sqlx::query("DELETE FROM summary_processes WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 3. Delete speaker-registry rows owned by this meeting. Cache rows in
    //    speaker_embeddings (meeting_id set) and the mapping/allowlist tables
    //    are meeting-scoped; enrolled prototypes (speaker_id set) are global
    //    and kept. FK enforcement is off, so these are manual cascades.
    sqlx::query("DELETE FROM speaker_embeddings WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM meeting_expected_speakers WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 4. Delete from transcripts
    sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    // 5. Finally, delete the meeting
    let result = sqlx::query("DELETE FROM meetings WHERE id = ?")
        .bind(meeting_id)
        .execute(&mut *transaction)
        .await?;

    Ok(result.rows_affected() > 0)
}
