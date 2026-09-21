//! Spike (openspec change `live-word-level-diarization`, task 1.1): verify
//! that Fast-mode stable turns published to `LiveTurnRegistry` are
//! append-only, time-ordered, and never revised, against a real recorded
//! multi-speaker session — not a live microphone recording, but the same
//! `OnlineDiarizationProcessor::process_chunk` code path a live recording
//! drives, fed from a stored recording so the spike is reproducible.
//!
//! Personal-machine harness (hardcoded path to a local recording + local
//! models directory), matching the existing `repro_online_diarization.rs` /
//! `repro_full_stop.rs` convention in this file — not part of CI.

use std::path::Path;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::live_diarization_reconcile::registry;
use app_lib::audio::online_diarization::{DiarizationMode, OnlineDiarizationProcessor};
use app_lib::audio::recording_state::{AudioChunk, DeviceType};

fn make_chunks(
    mono: &[f32],
    sample_rate: u32,
    device: DeviceType,
    chunk_secs: f64,
) -> Vec<AudioChunk> {
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
async fn fast_mode_stable_turns_are_monotonic_on_a_real_multi_speaker_recording() {
    // A real ~14.5-minute two-party meeting (mic + system audio channels) on
    // this machine — well over the 3+ minute / multi-speaker bar task 1.1
    // asks for.
    let audio_path =
        "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-09-18_15-10/audio.mp4";
    let models_dir = std::env::var("MEETILY_MODELS_DIR")
        .unwrap_or_else(|_| "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string());

    let decoded = decode_audio_file(Path::new(audio_path)).expect("decode real recording");
    println!(
        "decoded: {}s, {} ch, {} Hz",
        decoded.duration_seconds, decoded.channels, decoded.sample_rate
    );
    assert!(
        decoded.duration_seconds >= 180.0,
        "spike requires a 3+ minute recording, got {}s",
        decoded.duration_seconds
    );

    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let mic_chunks = make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6);
    let sys_chunks = make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6);
    println!("chunks: mic={} sys={}", mic_chunks.len(), sys_chunks.len());

    // Fresh registry: the process-wide singleton may carry state from an
    // earlier test in the same binary.
    let reg = registry();
    reg.clear();

    let mut processor = OnlineDiarizationProcessor::new(
        DiarizationMode::Fast,
        8,
        true,
        Path::new(&models_dir),
        None,
        None,
    )
    .expect("processor init (requires local models)");

    // Feed interleaved chunks, exactly like the real capture pipeline.
    let max_len = mic_chunks.len().max(sys_chunks.len());
    for i in 0..max_len {
        if let Some(c) = mic_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
        if let Some(c) = sys_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
    }
    assert!(
        !processor.is_in_error_state(),
        "engine entered an error state while processing the recording"
    );

    let mic_turns = reg.turns("Microphone");
    let sys_turns = reg.turns("System");
    println!(
        "stable turns published: Microphone={} System={}",
        mic_turns.len(),
        sys_turns.len()
    );
    for t in &mic_turns {
        println!("  MIC  {:.3}-{:.3} {}", t.start_time, t.end_time, t.speaker);
    }
    for t in &sys_turns {
        println!("  SYS  {:.3}-{:.3} {}", t.start_time, t.end_time, t.speaker);
    }

    assert!(
        !mic_turns.is_empty() || !sys_turns.is_empty(),
        "expected at least one stable turn from a 14+ minute two-party recording"
    );

    println!(
        "monotonic: Microphone={} System={}",
        reg.is_ordered("Microphone"),
        reg.is_ordered("System")
    );
    assert!(
        reg.is_ordered("Microphone"),
        "Microphone stable-turn stream went backwards in time — see warn logs above; \
         per task 1.1, D4's watermark must downgrade to the next-later-turn rule"
    );
    assert!(
        reg.is_ordered("System"),
        "System stable-turn stream went backwards in time — see warn logs above; \
         per task 1.1, D4's watermark must downgrade to the next-later-turn rule"
    );
}

/// Task 4.2: Efficient mode SHALL produce zero registry/live-diarization
/// activity. `Engine::Efficient` (online_diarization.rs) never calls
/// `live_diarization_reconcile::registry().publish(...)` — that call exists
/// only in the Fast-mode streaming-pipeline stable-turn branch — so this is a
/// structural guarantee, not a runtime flag; this test confirms it against a
/// real recording rather than only by reading the code.
#[tokio::test]
async fn efficient_mode_never_publishes_live_turns() {
    let audio_path =
        "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-09-18_15-10/audio.mp4";
    let models_dir = std::env::var("MEETILY_MODELS_DIR")
        .unwrap_or_else(|_| "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string());

    let decoded = decode_audio_file(Path::new(audio_path)).expect("decode real recording");
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let mic_chunks = make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6);
    let sys_chunks = make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6);

    let reg = registry();
    reg.clear();

    let mut processor = OnlineDiarizationProcessor::new(
        DiarizationMode::Efficient,
        8,
        true,
        Path::new(&models_dir),
        None,
        None,
    )
    .expect("processor init (requires local models)");

    let max_len = mic_chunks.len().max(sys_chunks.len());
    for i in 0..max_len {
        if let Some(c) = mic_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
        if let Some(c) = sys_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
    }
    assert!(!processor.is_in_error_state());

    assert!(
        !reg.has_any_turns(),
        "Efficient mode must never publish stable turns to the live diarization registry"
    );
}
