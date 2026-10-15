use crate::database::models::SummaryProcess;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::SqlitePool;
use tracing::{error, info as log_info};

pub struct SummaryProcessesRepository;

pub const INTERRUPTED_RUN_ERROR: &str =
    "Summary generation was interrupted because the app closed. Generate the summary again.";

impl SummaryProcessesRepository {
    /// Retrieves the current summary process state for a given meeting ID.
    pub async fn get_summary_data(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>("SELECT * FROM summary_processes WHERE meeting_id = ?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn update_meeting_summary(
        pool: &SqlitePool,
        meeting_id: &str,
        summary: &Value,
    ) -> Result<bool, sqlx::Error> {
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool = sqlx::query("SELECT 1 FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_optional(&mut *transaction)
            .await?
            .is_some();

        if !meeting_exists {
            log_info!(
                "Attempted to save summary for a non-existent meeting_id: {}",
                meeting_id
            );
            transaction.rollback().await?;
            return Ok(false);
        }

        let result_json = serde_json::to_string(summary);
        if result_json.is_err() {
            error!("Can't convert the json to string for saving to Database");
            transaction.rollback().await?;
            return Ok(false);
        }
        let now = Utc::now();

        sqlx::query("UPDATE summary_processes SET result = ?, updated_at = ? WHERE meeting_id = ?")
            .bind(result_json.unwrap())
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        sqlx::query("UPDATE meetings SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(meeting_id)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        log_info!(
            "Successfully updated summary and timestamp for meeting_id: {}",
            meeting_id
        );
        Ok(true)
    }

    pub async fn get_summary_data_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<SummaryProcess>, sqlx::Error> {
        sqlx::query_as::<_, SummaryProcess>(
            "SELECT p.* FROM summary_processes p JOIN transcript_chunks t ON p.meeting_id = t.meeting_id WHERE p.meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await
    }

    /// Starts a run: the row goes `PENDING` with `start_time = started_at`,
    /// which is the run's identity for every later terminal write.
    pub async fn create_or_reset_process(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        log_info!(
            "Creating or resetting summary process for meeting_id: {}",
            meeting_id
        );
        let now = Utc::now();
        sqlx::query(
            r#"
            INSERT INTO summary_processes (meeting_id, status, created_at, updated_at, start_time, result, error)
            VALUES (?, 'PENDING', ?, ?, ?, NULL, NULL)
            ON CONFLICT(meeting_id) DO UPDATE SET
                status = 'PENDING',
                updated_at = excluded.updated_at,
                start_time = excluded.start_time,
                result_backup = result,
                result_backup_timestamp = excluded.updated_at,
                result = result,
                error = NULL
            "#
        )
        .bind(meeting_id)
        .bind(now)
        .bind(now)
        .bind(started_at)
        .execute(pool)
        .await?;
        log_info!(
            "Backed up existing summary before regeneration for meeting_id: {}",
            meeting_id
        );
        Ok(())
    }

    /// Compare-and-set: applies only while the row is still the pending run
    /// that started at `started_at`. Returns whether it applied.
    pub async fn update_process_completed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        result: Value, // Keep this as Value to handle both old and new formats if needed
        chunk_count: i64,
        processing_time: f64,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();
        let result_str = serde_json::to_string(&result)
            .map_err(|e| sqlx::Error::Protocol(format!("Failed to serialize result: {}", e)))?;

        let applied = sqlx::query(
            r#"
            UPDATE summary_processes
            SET status = 'completed', result = ?, updated_at = ?, end_time = ?, chunk_count = ?, processing_time = ?, error = NULL, result_backup = NULL, result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#
        )
        .bind(result_str)
        .bind(now)
        .bind(now)
        .bind(chunk_count)
        .bind(processing_time)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?
        .rows_affected()
            == 1;
        if applied {
            log_info!(
                "Summary completed and backup cleared for meeting_id: {}",
                meeting_id
            );
        }
        Ok(applied)
    }

    /// Compare-and-set like `update_process_completed`; restores the backup.
    pub async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error: &str,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        let applied = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(error)
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?
        .rows_affected()
            == 1;
        if applied {
            log_info!(
                "Summary generation failed and backup restored for meeting_id: {}",
                meeting_id
            );
        }
        Ok(applied)
    }

    /// Compare-and-set like `update_process_completed`; restores the backup.
    pub async fn update_process_cancelled(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let now = Utc::now();

        // Restore from backup if it exists, otherwise keep current result
        let applied = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'cancelled',
                updated_at = ?,
                end_time = ?,
                error = 'Generation was cancelled by user',
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(meeting_id)
        .bind(started_at)
        .execute(pool)
        .await?
        .rows_affected()
            == 1;
        if applied {
            log_info!(
                "Marked summary process as cancelled and restored backup for meeting_id: {}",
                meeting_id
            );
        }
        Ok(applied)
    }

    /// Fails every row left `pending` by a previous app process. Call once at
    /// startup, before any run can start, so it cannot race a live run.
    pub async fn fail_interrupted_runs(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
        let now = Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE summary_processes
            SET
                status = 'failed',
                error = ?,
                updated_at = ?,
                end_time = ?,
                result = COALESCE(result_backup, result),
                result_backup = NULL,
                result_backup_timestamp = NULL
            WHERE LOWER(status) = 'pending'
            "#,
        )
        .bind(INTERRUPTED_RUN_ERROR)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
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
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m1', 'M', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    /// A completed earlier summary, then a new pending run that backed it up.
    async fn pending_regeneration(pool: &SqlitePool) -> DateTime<Utc> {
        let first = Utc::now() - Duration::seconds(10);
        SummaryProcessesRepository::create_or_reset_process(pool, "m1", first)
            .await
            .unwrap();
        assert!(SummaryProcessesRepository::update_process_completed(
            pool,
            "m1",
            first,
            serde_json::json!({"markdown": "old"}),
            1,
            1.0
        )
        .await
        .unwrap());
        let started_at = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(pool, "m1", started_at)
            .await
            .unwrap();
        started_at
    }

    async fn row(pool: &SqlitePool) -> SummaryProcess {
        SummaryProcessesRepository::get_summary_data(pool, "m1")
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn completed_then_cancelled_keeps_completed() {
        let pool = setup_pool().await;
        let run = pending_regeneration(&pool).await;

        assert!(SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            run,
            serde_json::json!({"markdown": "new"}),
            2,
            1.0
        )
        .await
        .unwrap());
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(&pool, "m1", run)
                .await
                .unwrap()
        );

        let row = row(&pool).await;
        assert_eq!(row.status, "completed");
        assert!(row.result.unwrap().contains("new"));
    }

    #[tokio::test]
    async fn cancelled_then_completed_keeps_cancelled_with_restored_result() {
        let pool = setup_pool().await;
        let run = pending_regeneration(&pool).await;

        assert!(
            SummaryProcessesRepository::update_process_cancelled(&pool, "m1", run)
                .await
                .unwrap()
        );
        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            run,
            serde_json::json!({"markdown": "new"}),
            2,
            1.0
        )
        .await
        .unwrap());

        let row = row(&pool).await;
        assert_eq!(row.status, "cancelled");
        assert!(row.result.unwrap().contains("old"));
    }

    #[tokio::test]
    async fn stale_start_time_is_rejected_by_every_terminal_write() {
        let pool = setup_pool().await;
        let run = pending_regeneration(&pool).await;
        let stale = run - Duration::nanoseconds(1);

        assert!(!SummaryProcessesRepository::update_process_completed(
            &pool,
            "m1",
            stale,
            serde_json::json!({"markdown": "stale"}),
            1,
            1.0
        )
        .await
        .unwrap());
        assert!(
            !SummaryProcessesRepository::update_process_failed(&pool, "m1", stale, "boom")
                .await
                .unwrap()
        );
        assert!(
            !SummaryProcessesRepository::update_process_cancelled(&pool, "m1", stale)
                .await
                .unwrap()
        );

        let row = row(&pool).await;
        assert_eq!(row.status, "PENDING");
        assert_eq!(row.start_time, Some(run));
    }

    #[tokio::test]
    async fn interrupted_pending_run_is_failed_with_backup_restored() {
        let pool = setup_pool().await;
        sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m2', 'M2', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let done = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m2", done)
            .await
            .unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool,
            "m2",
            done,
            serde_json::json!({"markdown": "kept"}),
            1,
            1.0,
        )
        .await
        .unwrap();
        pending_regeneration(&pool).await;

        assert_eq!(
            SummaryProcessesRepository::fail_interrupted_runs(&pool)
                .await
                .unwrap(),
            1
        );

        let interrupted = row(&pool).await;
        assert_eq!(interrupted.status, "failed");
        assert_eq!(interrupted.error.as_deref(), Some(INTERRUPTED_RUN_ERROR));
        assert!(interrupted.result.unwrap().contains("old"));
        assert!(interrupted.result_backup.is_none());

        let completed = SummaryProcessesRepository::get_summary_data(&pool, "m2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.status, "completed");
        assert!(completed.result.unwrap().contains("kept"));
    }

    #[tokio::test]
    async fn start_time_reads_back_as_the_bound_value() {
        let pool = setup_pool().await;
        let started_at = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m1", started_at)
            .await
            .unwrap();

        assert_eq!(row(&pool).await.start_time, Some(started_at));
    }
}
