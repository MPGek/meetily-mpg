//! Speaker registry CRUD and the per-meeting expected-speaker allowlist.

use super::SpeakerRepository;
use crate::database::models::{MeetingExpectedSpeaker, Speaker};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};
use uuid::Uuid;

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
    pub async fn find_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Speaker>, SqlxError> {
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
        let rows = sqlx::query("UPDATE speakers SET name = ?, updated_at = ? WHERE id = ?")
            .bind(trimmed)
            .bind(now)
            .bind(speaker_id)
            .execute(pool)
            .await?;
        Ok(rows.rows_affected() > 0)
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[tokio::test]
    async fn find_or_create_is_idempotent_and_case_insensitive() {
        let pool = setup_pool().await;

        let a = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let a2 = SpeakerRepository::find_or_create_by_name(&pool, "alice")
            .await
            .unwrap();
        assert_eq!(a.id, a2.id, "case-insensitive lookup returns same row");

        let b = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();
        assert_ne!(a.id, b.id, "distinct names create distinct speakers");

        let list = SpeakerRepository::list_speakers(&pool).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn rename_speaker_updates_name() {
        let pool = setup_pool().await;
        let a = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        assert!(
            SpeakerRepository::rename_speaker(&pool, &a.id, "Alice Smith")
                .await
                .unwrap()
        );
        let found = SpeakerRepository::find_by_name(&pool, "alice smith")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, a.id);
        assert_eq!(found.name, "Alice Smith");
    }

    #[tokio::test]
    async fn expected_speakers_round_trip() {
        let pool = setup_pool().await;
        insert_meeting(&pool, "m1").await;
        let alice = SpeakerRepository::find_or_create_by_name(&pool, "Alice")
            .await
            .unwrap();
        let bob = SpeakerRepository::find_or_create_by_name(&pool, "Bob")
            .await
            .unwrap();

        SpeakerRepository::set_expected_speakers(&pool, "m1", &[alice.id.clone(), bob.id.clone()])
            .await
            .unwrap();
        let mut ids = SpeakerRepository::get_expected_speakers(&pool, "m1")
            .await
            .unwrap();
        ids.sort();
        let mut expected = vec![alice.id, bob.id];
        expected.sort();
        assert_eq!(ids, expected);

        // Empty set = match-all.
        SpeakerRepository::set_expected_speakers(&pool, "m1", &[])
            .await
            .unwrap();
        assert!(SpeakerRepository::get_expected_speakers(&pool, "m1")
            .await
            .unwrap()
            .is_empty());
    }
}
