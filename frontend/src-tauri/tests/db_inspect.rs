use sqlx::SqlitePool;

#[tokio::test]
async fn inspect_meeting_0818_1322() {
    let pool = SqlitePool::connect("sqlite://C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/meeting_minutes.sqlite?mode=ro")
        .await
        .expect("open db");

    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('meetings')")
        .fetch_all(&pool)
        .await
        .expect("query columns");
    println!("=== MEETING COLUMNS ===");
    println!("{:?}", cols);

    let meetings: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, title, diarization_status, created_at FROM meetings ORDER BY created_at DESC LIMIT 12",
    )
    .fetch_all(&pool)
    .await
    .expect("query meetings");
    println!("=== MEETINGS ===");
    for (meeting_id, title, _, created_at) in &meetings {
        println!("{} | {} | {}", meeting_id, title, created_at);
    }

    let rows: Vec<(String, String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT m.id, m.title, m.diarization_status, m.created_at,
                (SELECT COUNT(*) FROM transcripts t WHERE t.meeting_id = m.id) AS tc
         FROM meetings m ORDER BY m.created_at DESC LIMIT 20",
    )
    .fetch_all(&pool)
    .await
    .expect("query all");
    println!("=== ALL MEETINGS ===");
    for r in &rows {
        println!("{:?}", r);
    }

    println!("=== MEETING_SPEAKERS count per meeting ===");
    let ms_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meeting_speakers")
        .fetch_one(&pool)
        .await
        .expect("count ms");
    println!("total meeting_speakers rows: {}", ms_total);
    let ms_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT meeting_id, COUNT(*) FROM meeting_speakers GROUP BY meeting_id",
    )
    .fetch_all(&pool)
    .await
    .expect("query ms counts");
    println!("{:?}", ms_rows);

    println!("=== CACHE rows by meeting (speaker_embeddings, no speaker_id) ===");
    let caches: Vec<(String, i64)> = sqlx::query_as(
        "SELECT meeting_id, COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL GROUP BY meeting_id ORDER BY 2 DESC LIMIT 10",
    )
    .fetch_all(&pool)
    .await
    .expect("query caches");
    println!("{:?}", caches);

    println!("=== SPEAKER_EMBEDDINGS ownership counts ===");
    let own: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT speaker_id, COUNT(*) FROM speaker_embeddings GROUP BY speaker_id",
    )
    .fetch_all(&pool)
    .await
    .expect("query emb");
    println!("by speaker: {:?}", own);
    let own2: Vec<(String, i64)> = sqlx::query_as(
        "SELECT meeting_id, COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NULL GROUP BY meeting_id",
    )
    .fetch_all(&pool)
    .await
    .expect("query emb2");
    println!("caches by meeting: {:?}", own2);

    println!("=== SPEAKERS registry ===");
    let sp: Vec<(String, String)> = sqlx::query_as("SELECT id, name FROM speakers")
        .fetch_all(&pool)
        .await
        .expect("query speakers");
    println!("{:?}", sp);

    println!("=== MIGRATIONS ===");
    let migs: Vec<(i64, String, String)> = sqlx::query_as("SELECT version, description, installed_on FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&pool)
        .await
        .expect("query migrations");
    for m in &migs {
        println!("{:?}", m);
    }

    println!("=== SETTINGS (diarization) ===");
    let prefs: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM app_settings WHERE key LIKE '%diariz%' OR key LIKE '%speaker%' OR key LIKE '%expected%'",
    )
    .fetch_all(&pool)
    .await
    .expect("query settings");
    for p in &prefs {
        println!("{:?}", p);
    }

    println!("=== MEETING_SPEAKERS detail (working meetings) ===");
    let ms_rows: Vec<(String, String, Option<String>, Option<String>, Option<f64>)> = sqlx::query_as(
        "SELECT meeting_id, cluster_label, speaker_id, matched_by, match_score FROM meeting_speakers WHERE meeting_id IN ('meeting-55484233-81b6-430f-9f65-69645c79d632','meeting-9b26ad47-55ca-485e-a530-de2db25d35fe') ORDER BY meeting_id, cluster_label",
    )
    .fetch_all(&pool)
    .await
    .expect("query ms detail");
    for r in &ms_rows {
        println!("{:?}", r);
    }

    println!("=== meeting_expected_speakers ===");
    let exp: Vec<(String, String)> = sqlx::query_as("SELECT meeting_id, speaker_id FROM meeting_expected_speakers")
        .fetch_all(&pool)
        .await
        .expect("query expected");
    println!("{:?}", exp);

    println!("=== meetings diarization_status/speaker_names ===");
    let m2: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT title, diarization_status, speaker_names FROM meetings WHERE created_at LIKE '2026-08-18%' ORDER BY created_at",
    )
    .fetch_all(&pool)
    .await
    .expect("query m2");
    for r in &m2 {
        println!("{:?}", r);
    }

    let targets = vec![
        "meeting-a7323df9-9601-47c1-baa3-be22588af544",
        "meeting-4a5e51a9-0923-4903-a472-12a8125130d1",
    ];
    for meeting_id in targets {
        let title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id = ?")
            .bind(meeting_id)
            .fetch_one(&pool)
            .await
            .expect("query title");
        println!("=== TRANSCRIPTS for {} ({}) ===", meeting_id, title);
        let rows: Vec<(String, String, Option<f64>, Option<f64>, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, substr(transcript, 1, 60), audio_start_time, audio_end_time, source_device, speaker, speaker_label FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
        )
        .bind(meeting_id)
        .fetch_all(&pool)
        .await
        .expect("query transcripts");
        for r in &rows {
            println!("id={} start={:?} end={:?} dev={:?} spk={:?} label={:?} | {}",
                r.0, r.2, r.3, r.4, r.5, r.6, r.1);
        }

        let ms: Vec<(String, Option<String>, Option<String>, Option<f64>)> = sqlx::query_as(
            "SELECT cluster_label, speaker_id, matched_by, match_score FROM meeting_speakers WHERE meeting_id = ? ORDER BY cluster_label",
        )
        .bind(meeting_id)
        .fetch_all(&pool)
        .await
        .expect("query meeting_speakers");
        println!("=== MEETING_SPEAKERS ===");
        for r in &ms {
            println!("{:?}", r);
        }
    }
}
