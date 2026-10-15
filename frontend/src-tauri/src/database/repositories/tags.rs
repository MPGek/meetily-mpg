use crate::database::models::{
    default_tag_color, DateTimeUtc, MeetingTag, MeetingTagWithUsage, MEETING_TAG_PALETTE,
};
use chrono::Utc;
use sqlx::{Error as SqlxError, SqlitePool};
use uuid::Uuid;

/// Meeting tag dictionary + meeting link repository.
/// Names are unique case-insensitively (see migration
/// `20260911000000_add_meeting_tags.sql` NOCASE index).
pub struct TagsRepository;

impl TagsRepository {
    /// Validate that a palette color key is known.
    pub fn is_valid_color(color: &str) -> bool {
        MEETING_TAG_PALETTE.contains(&color)
    }

    /// List all tags with usage counts, most-used first, then name.
    pub async fn list_tags(
        pool: &SqlitePool,
    ) -> Result<Vec<MeetingTagWithUsage>, SqlxError> {
        // LEFT JOIN so unused tags appear with usage_count 0.
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT t.id, t.name, t.color, COUNT(l.meeting_id) AS usage_count \
             FROM meeting_tags t LEFT JOIN meeting_tag_links l ON l.tag_id = t.id \
             GROUP BY t.id ORDER BY usage_count DESC, t.name COLLATE NOCASE ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, color, usage_count)| MeetingTagWithUsage {
                id,
                name,
                color,
                usage_count,
            })
            .collect())
    }

    /// Find a tag by case-insensitive name.
    pub async fn find_by_name(
        pool: &SqlitePool,
        name: &str,
    ) -> Result<Option<MeetingTag>, SqlxError> {
        sqlx::query_as::<_, MeetingTag>(
            "SELECT id, name, color, created_at, updated_at FROM meeting_tags \
             WHERE name = ? COLLATE NOCASE LIMIT 1",
        )
        .bind(name.trim())
        .fetch_optional(pool)
        .await
    }

    /// Get one tag by id.
    pub async fn get_tag(
        pool: &SqlitePool,
        tag_id: &str,
    ) -> Result<Option<MeetingTag>, SqlxError> {
        sqlx::query_as::<_, MeetingTag>(
            "SELECT id, name, color, created_at, updated_at FROM meeting_tags WHERE id = ?",
        )
        .bind(tag_id)
        .fetch_optional(pool)
        .await
    }

    /// Create a tag. Empty names are rejected; duplicates (NOCASE) return the
    /// existing row instead of inserting. Unknown colors fall back to the
    /// deterministic default for the name.
    pub async fn create_tag(
        pool: &SqlitePool,
        name: &str,
        color: Option<&str>,
    ) -> Result<MeetingTag, SqlxError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(SqlxError::Protocol("tag name cannot be empty".into()));
        }
        if let Some(existing) = Self::find_by_name(pool, trimmed).await? {
            return Ok(existing);
        }
        let resolved = match color {
            Some(c) if Self::is_valid_color(c) => c.to_string(),
            _ => default_tag_color(trimmed).to_string(),
        };
        let id = format!("tag-{}", Uuid::new_v4());
        let now = Utc::now();
        match sqlx::query(
            "INSERT INTO meeting_tags (id, name, color, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(trimmed)
        .bind(&resolved)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        {
            Ok(_) => Self::get_tag(pool, &id)
                .await?
                .ok_or_else(|| SqlxError::Protocol("inserted tag not found".into())),
            Err(SqlxError::Database(e)) if e.is_unique_violation() => {
                // Lost a concurrent-insert race; return the winner.
                Self::find_by_name(pool, trimmed)
                    .await?
                    .ok_or_else(|| SqlxError::Protocol("unique tag vanished after race".into()))
            }
            Err(e) => Err(e),
        }
    }

    /// Rename a tag, preserving all meeting links. Rejects empty names and
    /// names already taken by another tag (NOCASE).
    pub async fn rename_tag(
        pool: &SqlitePool,
        tag_id: &str,
        new_name: &str,
    ) -> Result<MeetingTag, SqlxError> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(SqlxError::Protocol("tag name cannot be empty".into()));
        }
        let existing = Self::get_tag(pool, tag_id)
            .await?
            .ok_or(SqlxError::RowNotFound)?;
        if existing.name.eq_ignore_ascii_case(trimmed) {
            // Only casing/whitespace changed: normalize the stored form.
            let now = Utc::now();
            sqlx::query("UPDATE meeting_tags SET name = ?, updated_at = ? WHERE id = ?")
                .bind(trimmed)
                .bind(now)
                .bind(tag_id)
                .execute(pool)
                .await?;
            return Self::get_tag(pool, tag_id)
                .await?
                .ok_or_else(|| SqlxError::Protocol("renamed tag not found".into()));
        }
        if Self::find_by_name(pool, trimmed).await?.is_some() {
            return Err(SqlxError::Protocol(format!(
                "tag name '{}' is already taken",
                trimmed
            )));
        }
        let now = Utc::now();
        sqlx::query("UPDATE meeting_tags SET name = ?, updated_at = ? WHERE id = ?")
            .bind(trimmed)
            .bind(now)
            .bind(tag_id)
            .execute(pool)
            .await?;
        Self::get_tag(pool, tag_id)
            .await?
            .ok_or_else(|| SqlxError::Protocol("renamed tag not found".into()))
    }

    /// Override a tag's palette color.
    pub async fn set_tag_color(
        pool: &SqlitePool,
        tag_id: &str,
        color: &str,
    ) -> Result<MeetingTag, SqlxError> {
        if !Self::is_valid_color(color) {
            return Err(SqlxError::Protocol(format!(
                "unknown tag color '{}'",
                color
            )));
        }
        let rows = sqlx::query("UPDATE meeting_tags SET color = ?, updated_at = ? WHERE id = ?")
            .bind(color)
            .bind(Utc::now())
            .bind(tag_id)
            .execute(pool)
            .await?;
        if rows.rows_affected() == 0 {
            return Err(SqlxError::RowNotFound);
        }
        Self::get_tag(pool, tag_id)
            .await?
            .ok_or_else(|| SqlxError::Protocol("updated tag not found".into()))
    }

    /// Delete a tag and all its meeting links. Meetings are untouched.
    pub async fn delete_tag(pool: &SqlitePool, tag_id: &str) -> Result<bool, SqlxError> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM meeting_tag_links WHERE tag_id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        let res = sqlx::query("DELETE FROM meeting_tags WHERE id = ?")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(res.rows_affected() > 0)
    }

    /// Link a tag to a meeting. Idempotent: existing links are a no-op.
    /// Returns true when a new link was created.
    pub async fn assign_tag(
        pool: &SqlitePool,
        meeting_id: &str,
        tag_id: &str,
    ) -> Result<bool, SqlxError> {
        if meeting_id.trim().is_empty() || tag_id.trim().is_empty() {
            return Err(SqlxError::Protocol("meeting_id/tag_id cannot be empty".into()));
        }
        // Guard against linking to missing rows with a clear error.
        let meeting_exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM meetings WHERE id = ?")
                .bind(meeting_id)
                .fetch_optional(pool)
                .await?;
        if meeting_exists.is_none() {
            return Err(SqlxError::RowNotFound);
        }
        if Self::get_tag(pool, tag_id).await?.is_none() {
            return Err(SqlxError::RowNotFound);
        }
        let res = sqlx::query(
            "INSERT OR IGNORE INTO meeting_tag_links (meeting_id, tag_id, created_at) \
             VALUES (?, ?, ?)",
        )
        .bind(meeting_id)
        .bind(tag_id)
        .bind(Utc::now())
        .execute(pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Remove a tag from a meeting. The dictionary entry survives.
    pub async fn unassign_tag(
        pool: &SqlitePool,
        meeting_id: &str,
        tag_id: &str,
    ) -> Result<bool, SqlxError> {
        let res = sqlx::query(
            "DELETE FROM meeting_tag_links WHERE meeting_id = ? AND tag_id = ?",
        )
        .bind(meeting_id)
        .bind(tag_id)
        .execute(pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Batch-load tags for a set of meetings: meeting_id -> tags (ordered by
    /// tag name). Missing meetings map to empty vecs at the call site.
    pub async fn tags_for_meetings(
        pool: &SqlitePool,
        meeting_ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<MeetingTag>>, SqlxError> {
        use std::collections::HashMap;
        let mut map: HashMap<String, Vec<MeetingTag>> = HashMap::new();
        if meeting_ids.is_empty() {
            return Ok(map);
        }
        // Build a bounded IN list; meeting lists are small (hundreds max).
        let placeholders = meeting_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT l.meeting_id, t.id, t.name, t.color, t.created_at, t.updated_at \
             FROM meeting_tag_links l JOIN meeting_tags t ON t.id = l.tag_id \
             WHERE l.meeting_id IN ({}) ORDER BY t.name COLLATE NOCASE ASC",
            placeholders
        );
        let mut q = sqlx::query_as::<_, (String, String, String, String, DateTimeUtc, DateTimeUtc)>(&sql);
        for id in meeting_ids {
            q = q.bind(id);
        }
        for (meeting_id, id, name, color, created_at, updated_at) in q.fetch_all(pool).await? {
            map.entry(meeting_id).or_default().push(MeetingTag {
                id,
                name,
                color,
                created_at,
                updated_at,
            });
        }
        Ok(map)
    }
}
