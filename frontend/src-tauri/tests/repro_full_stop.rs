use std::path::Path;
use std::sync::Arc;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::online_diarization::{DiarizationMode, OnlineDiarizationProcessor, OnlineClusterEmbeddings, PrototypeStore};
use app_lib::audio::recording_saver::TranscriptSegment;
use app_lib::audio::recording_state::{AudioChunk, DeviceType};
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use std::sync::RwLock;

fn make_chunks(mono: &[f32], sample_rate: u32, device: DeviceType, chunk_secs: f64) -> Vec<AudioChunk> {
    let chunk_len = (sample_rate as f64 * chunk_secs) as usize;
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut idx = 0u64;
    while start < mono.len() {
        let end = (start + chunk_len).min(mono.len());
        if end > start {
            out.push(AudioChunk {
                data: mono[start..end].to_vec(),
                sample_rate,
                timestamp: start as f64 / sample_rate as f64,
                chunk_id: idx,
                device_type: device.clone(),
                channels: 1,
            });
            idx += 1;
        }
        start = end;
    }
    out
}

#[tokio::test]
async fn full_stop_flow_fast_1322() {
    let db_path = "sqlite://C:/Users/VASILI~1.KOT/AppData/Local/Temp/kilo/meetily_test.sqlite?mode=rwc";
    let pool = SqlitePool::connect(db_path).await.expect("open db");
    let meeting_id = "meeting-a7323df9-9601-47c1-baa3-be22588af544";
    let audio = "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-08-18_13-22/audio.mp4";

    let decoded = decode_audio_file(Path::new(audio)).expect("decode");
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let (mic_chunks, sys_chunks) = (
        make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6),
        make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6),
    );

    // Load real transcripts from the copied DB.
    let rows: Vec<(i64, f64, f64, String)> = sqlx::query_as(
        "SELECT rowid, audio_start_time, audio_end_time, source_device FROM transcripts WHERE meeting_id = ? ORDER BY audio_start_time",
    )
    .bind(meeting_id)
    .fetch_all(&pool)
    .await
    .expect("transcripts");
    println!("DB transcripts: {}", rows.len());
    let transcripts: Vec<TranscriptSegment> = rows
        .iter()
        .enumerate()
        .map(|(i, (_, s, e, dev))| TranscriptSegment {
            id: format!("t{}", i),
            text: String::new(),
            audio_start_time: *s,
            audio_end_time: *e,
            duration: e - s,
            display_time: String::new(),
            confidence: 1.0,
            sequence_id: i as u64,
            source_device: dev.clone(),
        })
        .collect();

    // Load prototype store like the real app (empty registry is fine).
    let store = Arc::new(RwLock::new(PrototypeStore::load(&pool, None, true).await.expect("store")));

    let (turn_sender, _turn_rx) = mpsc::unbounded_channel();
    let models_dir = "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models";
    let mut processor = OnlineDiarizationProcessor::new(
        DiarizationMode::Fast,
        8,
        true,
        Path::new(models_dir),
        Some(turn_sender),
        Some(store),
    )
    .expect("processor init");

    let max_len = mic_chunks.len().max(sys_chunks.len());
    for i in 0..max_len {
        if let Some(c) = mic_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
        if let Some(c) = sys_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
    }
    println!("error state: {}", processor.is_in_error_state());

    match processor.finalize(&transcripts) {
        Ok((assignments, clusters, bindings)) => {
            println!("finalize OK: {} assignments, mic={}, sys={}, bindings={}",
                assignments.len(), clusters.mic.len(), clusters.sys.len(), bindings.len());
            for a in &assignments {
                println!("  seq {} -> {}", a.sequence_id, a.speaker);
            }

            // Now persist like finalize_online_session does:
            app_lib::audio::diarization::persist_and_recognize_session(
                &pool, meeting_id, &clusters.mic, &clusters.sys, clusters.saw_system_audio,
            )
            .await
            .expect("persist");
            println!("persist OK");

            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM meeting_speakers WHERE meeting_id = ?")
                .bind(meeting_id)
                .fetch_one(&pool)
                .await
                .expect("count");
            println!("meeting_speakers rows after persist: {}", count);
        }
        Err(e) => println!("FINALIZE FAILED: {}", e),
    }
}
