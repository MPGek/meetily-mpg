//! Diagnostic replay for the "most segments collapse onto one cluster at stop"
//! report (openspec change `live-word-level-diarization`, task 6.1 evidence;
//! root-cause work belongs to `05b-live-diarization-accuracy`).
//!
//! Feeds a real recorded session straight into `OnlineDiarizationProcessor::
//! process_chunk` and then `finalize()`, with the session's real transcripts
//! (tokens included) loaded from a JSON dump. This path **bypasses every mpsc
//! channel** in the recording pipeline, so if the collapse reproduces here it
//! cannot have been caused by the bounded-channel change (`04-recording-lock-
//! hardening`).
//!
//! Personal-machine harness (hardcoded local recording path + models dir),
//! matching the existing `repro_online_diarization.rs` / `repro_full_stop.rs`
//! convention — not part of CI.

use std::collections::HashMap;
use std::path::Path;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::online_diarization::{DiarizationMode, OnlineDiarizationProcessor};
use app_lib::audio::recording_saver::TranscriptSegment;
use app_lib::audio::recording_state::{AudioChunk, DeviceType};
use app_lib::audio::token_assignment::Token;

const AUDIO: &str =
    "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-09-21_15-00/audio.mp4";
const TRANSCRIPTS_JSON: &str = r"C:\Users\VASILI~1.KOT\AppData\Local\Temp\claude\C--Users-vasiliy-kotov-Work-Own-meetily-mpg\60888f22-91f5-4a21-9d1f-5080e0728b3e\scratchpad\replay_transcripts.json";

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

fn load_transcripts() -> Vec<TranscriptSegment> {
    let raw = std::fs::read_to_string(TRANSCRIPTS_JSON).expect("transcripts json dump");
    let items: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("parse json");
    items
        .into_iter()
        .map(|v| {
            let tokens = v.get("tokens").and_then(|t| t.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        Some(Token {
                            text: t.get("text")?.as_str()?.to_string(),
                            start: t.get("start")?.as_f64()? as f32,
                            end: t.get("end")?.as_f64()? as f32,
                            refined: t
                                .get("refined")
                                .and_then(|r| r.as_bool())
                                .unwrap_or(false),
                        })
                    })
                    .collect::<Vec<_>>()
            });
            TranscriptSegment {
                id: v["id"].as_str().unwrap_or_default().to_string(),
                text: v["text"].as_str().unwrap_or_default().to_string(),
                audio_start_time: v["audio_start_time"].as_f64().unwrap_or(0.0),
                audio_end_time: v["audio_end_time"].as_f64().unwrap_or(0.0),
                duration: v["duration"].as_f64().unwrap_or(0.0),
                display_time: String::new(),
                confidence: 1.0,
                sequence_id: v["sequence_id"].as_u64().unwrap_or(0),
                source_device: v["source_device"]
                    .as_str()
                    .unwrap_or("Microphone")
                    .to_string(),
                tokens,
            }
        })
        .collect()
}

fn report(label: &str, assignments: &[app_lib::audio::online_diarization::SpeakerAssignment]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for a in assignments {
        *counts.entry(a.speaker.clone()).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by_key(|e| std::cmp::Reverse(e.1));
    let total = assignments.len().max(1);
    println!("=== {} : cluster distribution (top 10) ===", label);
    for (name, n) in ranked.iter().take(10) {
        println!(
            "  {:20} {:4}  {:5.1}%",
            name,
            n,
            *n as f64 / total as f64 * 100.0
        );
    }
    if let Some((top, n)) = ranked.first() {
        println!(
            ">>> {}: TOP CLUSTER {} = {}/{} = {:.1}% | distinct clusters = {}",
            label,
            top,
            n,
            total,
            *n as f64 / total as f64 * 100.0,
            ranked.len()
        );
    }
}

#[tokio::test]
async fn replay_reproduces_stop_time_cluster_distribution_without_any_channels() {
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });

    let transcripts = load_transcripts();
    println!("transcripts loaded: {}", transcripts.len());
    let with_tokens = transcripts
        .iter()
        .filter(|t| t.tokens.as_ref().map(|v| !v.is_empty()).unwrap_or(false))
        .count();
    println!("  with tokens: {}", with_tokens);

    let decoded = decode_audio_file(Path::new(AUDIO)).expect("decode real recording");
    println!(
        "decoded: {:.1}s, {} ch, {} Hz",
        decoded.duration_seconds, decoded.channels, decoded.sample_rate
    );
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let mic_chunks = make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6);
    let sys_chunks = make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6);
    println!("chunks: mic={} sys={}", mic_chunks.len(), sys_chunks.len());

    // Two runs over the same audio + transcripts, both with no prototype
    // store (isolates raw cluster assignment from registry name matching, so
    // "everything became <one name>" is measured as "everything became one
    // cluster", independent of recognition):
    //   1. Fast, max_speakers = 0  — exactly the user's settings.
    //   2. Efficient, max_speakers = 10 — the real participant count, the
    //      workaround candidate until change 05 makes the live path honour
    //      the tuning settings and the speaker ceiling.
    for (label, mode, max_speakers) in [
        ("FAST max=0 (user's settings)", DiarizationMode::Fast, 0usize),
        ("EFFICIENT max=10", DiarizationMode::Efficient, 10usize),
    ] {
        let mut processor = match OnlineDiarizationProcessor::new(
            mode,
            max_speakers,
            true,
            Path::new(&models_dir),
            None,
            None,
        ) {
            Ok(p) => p,
            Err(e) => {
                println!("{}: processor init FAILED: {}", label, e);
                continue;
            }
        };

        let max_len = mic_chunks.len().max(sys_chunks.len());
        for i in 0..max_len {
            if let Some(c) = mic_chunks.get(i) {
                processor.process_chunk(c.clone());
            }
            if let Some(c) = sys_chunks.get(i) {
                processor.process_chunk(c.clone());
            }
        }
        println!(
            "{}: fed all chunks; error state: {}",
            label,
            processor.is_in_error_state()
        );

        match processor.finalize(&transcripts) {
            Ok((assignments, clusters, _bindings, _display_pass)) => {
                println!(
                    "{}: finalize -> {} assignments, mic clusters {}, sys clusters {}",
                    label,
                    assignments.len(),
                    clusters.mic.len(),
                    clusters.sys.len()
                );
                report(label, &assignments);
            }
            Err(e) => println!("{}: FINALIZE FAILED: {}", label, e),
        }
        println!();
    }
}
