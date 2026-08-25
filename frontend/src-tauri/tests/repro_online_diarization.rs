use std::path::Path;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::online_diarization::{DiarizationMode, OnlineDiarizationProcessor};
use app_lib::audio::recording_saver::TranscriptSegment;
use app_lib::audio::recording_state::{AudioChunk, DeviceType};
use tokio::sync::mpsc;

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

async fn run_mode(
    mode: DiarizationMode,
    audio_path: &Path,
    transcripts: &[TranscriptSegment],
    label: &str,
) {
    println!("===== MODE {:?} ({}) =====", mode, label);
    let decoded = match decode_audio_file(audio_path) {
        Ok(d) => d,
        Err(e) => {
            println!("  DECODE FAILED: {}", e);
            return;
        }
    };
    println!(
        "  decoded: {}s, {} ch, {} Hz",
        decoded.duration_seconds, decoded.channels, decoded.sample_rate
    );
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let (mic_chunks, sys_chunks) = (
        make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6),
        make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6),
    );
    println!("  chunks: mic={} sys={}", mic_chunks.len(), sys_chunks.len());

    let (turn_sender, mut turn_rx) = mpsc::unbounded_channel();
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });
    let mut processor = match OnlineDiarizationProcessor::new(
        mode,
        8,
        true,
        Path::new(&models_dir),
        Some(turn_sender),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("  PROCESSOR INIT FAILED: {}", e);
            return;
        }
    };
    println!("  processor init OK (mode {:?})", processor.mode());

    // Feed interleaved chunks like the real pipeline (mic+sys together).
    let max_len = mic_chunks.len().max(sys_chunks.len());
    for i in 0..max_len {
        if let Some(c) = mic_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
        if let Some(c) = sys_chunks.get(i) {
            processor.process_chunk(c.clone());
        }
    }
    println!("  fed all chunks; error state: {}", processor.is_in_error_state());

    // Drain any pending turns
    let mut turns = Vec::new();
    while let Ok(t) = turn_rx.try_recv() {
        turns.push(t);
    }
    println!("  live turns emitted: {}", turns.len());

    match processor.finalize(transcripts) {
        Ok((assignments, clusters, bindings)) => {
            println!(
                "  finalize OK: {} assignments, mic clusters {}, sys clusters {}, bindings {}",
                assignments.len(),
                clusters.mic.len(),
                clusters.sys.len(),
                bindings.len()
            );
            for a in &assignments {
                println!("    seq {} -> {}", a.sequence_id, a.speaker);
            }
        }
        Err(e) => {
            println!("  FINALIZE FAILED: {}", e);
        }
    }
}

#[tokio::test]
async fn reproduce_online_diarization_failures() {
    let fast_audio = "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-08-18_13-22/audio.mp4";
    let eff_audio = "C:/Users/vasiliy.kotov/Music/meetily-recordings/Meeting 2026-08-18_13-21/audio.mp4";

    // Transcripts from the DB (meeting a7323df9 - Fast meeting)
    let fast_transcripts = vec![
        tseg(1, 1.482, 7.71),
        tseg(2, 16.458, 25.354),
        tseg(3, 26.218, 43.476),
        tseg(4, 43.786, 51.508),
        tseg(5, 51.882, 53.856),
        tseg(6, 67.626, 71.776),
        tseg(7, 72.074, 80.598),
        tseg(8, 80.598, 98.816),
        tseg(9, 98.218, 100.768),
    ];
    let eff_transcripts = vec![
        tseg(1, 0.138, 3.668),
        tseg(2, 4.01, 10.282),
        tseg(3, 18.954, 24.832),
    ];

    println!("### FAST MODE (13-22 meeting) ###");
    run_mode(DiarizationMode::Fast, Path::new(fast_audio), &fast_transcripts, "13-22").await;

    println!();
    println!("### EFFICIENT MODE (13-21 meeting) ###");
    run_mode(DiarizationMode::Efficient, Path::new(eff_audio), &eff_transcripts, "13-21").await;
    println!();
    println!("### EFFICIENT MODE max_speakers=0 (13-21) ###");
    run_mode_ms(DiarizationMode::Efficient, Path::new(eff_audio), &eff_transcripts, "13-21-ms0", 0).await;
    println!();
    println!("### FAST MODE max_speakers=0 (13-22) ###");
    run_mode_ms(DiarizationMode::Fast, Path::new(fast_audio), &fast_transcripts, "13-22-ms0", 0).await;
    println!();
    println!("### EFFICIENT MODE long chunks (13-21) ###");
    run_mode_long(DiarizationMode::Efficient, Path::new(eff_audio), &eff_transcripts, "13-21-long").await;
    println!();
    println!("### FAST MODE long chunks (13-22) ###");
    run_mode_long(DiarizationMode::Fast, Path::new(fast_audio), &fast_transcripts, "13-22-long").await;
    println!();
    println!("### EFFICIENT MODE 16kHz 25s chunks (13-21) ###");
    run_mode_16k(DiarizationMode::Efficient, Path::new(eff_audio), &eff_transcripts, "13-21-16k25s", 25.0).await;
    println!();
    println!("### FAST MODE 16kHz 25s chunks (13-22) ###");
    run_mode_16k(DiarizationMode::Fast, Path::new(fast_audio), &fast_transcripts, "13-22-16k25s", 25.0).await;
    println!();
    println!("### SHORT-CHUNK KILL TEST (Efficient) ###");
    run_mode_short(DiarizationMode::Efficient, Path::new(eff_audio), &eff_transcripts, "short-kill").await;
}

async fn run_mode_short(
    mode: DiarizationMode,
    audio_path: &Path,
    transcripts: &[TranscriptSegment],
    label: &str,
) {
    println!("===== MODE {:?} SHORT-CHUNK KILL TEST ({}) =====", mode, label);
    let decoded = match decode_audio_file(audio_path) {
        Ok(d) => d,
        Err(e) => {
            println!("  DECODE FAILED: {}", e);
            return;
        }
    };
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    // Mix of 0.15s (too short for embedder?) and normal chunks.
    let mut chunks = Vec::new();
    let chunk_len_short = (16000.0 * 0.15) as usize;
    let chunk_len_norm = (16000.0 * 3.0) as usize;
    let mut i = 0usize;
    let mut idx = 0u64;
    while i < mic.len() {
        let len = if idx % 5 == 0 { chunk_len_short } else { chunk_len_norm };
        let end = (i + len).min(mic.len());
        if end > i {
            chunks.push(AudioChunk {
                data: mic[i..end].to_vec(),
                sample_rate: 16000,
                timestamp: i as f64 / 16000.0,
                chunk_id: idx,
                device_type: DeviceType::Microphone,
                channels: 1,
            });
            idx += 1;
        }
        i = end;
    }
    println!("  chunks: {} (mixed 0.15s/3s)", chunks.len());
    let (turn_sender, _turn_rx) = mpsc::unbounded_channel();
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });
    let mut processor = match OnlineDiarizationProcessor::new(
        mode,
        8,
        true,
        Path::new(&models_dir),
        Some(turn_sender),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("  PROCESSOR INIT FAILED: {}", e);
            return;
        }
    };
    for c in &chunks {
        processor.process_chunk(c.clone());
        if processor.is_in_error_state() {
            println!("  ENGINE DIED on chunk {} (t={:.1}s, len={})", c.chunk_id, c.timestamp, c.data.len());
            break;
        }
    }
    println!("  final error state: {}", processor.is_in_error_state());
    match processor.finalize(transcripts) {
        Ok((assignments, clusters, _)) => println!(
            "  finalize OK: {} assignments, mic clusters {}, sys clusters {}",
            assignments.len(), clusters.mic.len(), clusters.sys.len()
        ),
        Err(e) => println!("  FINALIZE FAILED: {}", e),
    }
}

async fn run_mode_16k(
    mode: DiarizationMode,
    audio_path: &Path,
    transcripts: &[TranscriptSegment],
    label: &str,
    chunk_secs: f64,
) {
    println!("===== MODE {:?} 16kHz {:.0}s chunks ({}) =====", mode, chunk_secs, label);
    let decoded = match decode_audio_file(audio_path) {
        Ok(d) => d,
        Err(e) => {
            println!("  DECODE FAILED: {}", e);
            return;
        }
    };
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    // Resample 48k -> 16k like the pipeline does before sending.
    let mic16 = app_lib::audio::audio_processing::resample(&mic, decoded.sample_rate, 16000).unwrap_or_default();
    let sys16 = app_lib::audio::audio_processing::resample(&sys, decoded.sample_rate, 16000).unwrap_or_default();
    let (mic_chunks, sys_chunks) = (
        make_chunks(&mic16, 16000, DeviceType::Microphone, chunk_secs),
        make_chunks(&sys16, 16000, DeviceType::System, chunk_secs),
    );
    println!("  chunks: mic={} sys={} (16k)", mic_chunks.len(), sys_chunks.len());
    let (turn_sender, mut turn_rx) = mpsc::unbounded_channel();
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });
    let mut processor = match OnlineDiarizationProcessor::new(
        mode,
        8,
        true,
        Path::new(&models_dir),
        Some(turn_sender),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("  PROCESSOR INIT FAILED: {}", e);
            return;
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
    println!("  fed all chunks; error state: {}", processor.is_in_error_state());
    let mut turns = Vec::new();
    while let Ok(t) = turn_rx.try_recv() {
        turns.push(t);
    }
    println!("  live turns emitted: {}", turns.len());
    match processor.finalize(transcripts) {
        Ok((assignments, clusters, _)) => {
            println!(
                "  finalize OK: {} assignments, mic clusters {}, sys clusters {}",
                assignments.len(),
                clusters.mic.len(),
                clusters.sys.len()
            );
            for a in &assignments {
                println!("    seq {} -> {}", a.sequence_id, a.speaker);
            }
        }
        Err(e) => {
            println!("  FINALIZE FAILED: {}", e);
        }
    }
}

async fn run_mode_long(
    mode: DiarizationMode,
    audio_path: &Path,
    transcripts: &[TranscriptSegment],
    label: &str,
) {
    println!("===== MODE {:?} LONG CHUNKS ({}) =====", mode, label);
    let decoded = match decode_audio_file(audio_path) {
        Ok(d) => d,
        Err(e) => {
            println!("  DECODE FAILED: {}", e);
            return;
        }
    };
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    // Long 8-second chunks to mimic long VAD-merged segments.
    let (mic_chunks, sys_chunks) = (
        make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 8.0),
        make_chunks(&sys, decoded.sample_rate, DeviceType::System, 8.0),
    );
    println!("  chunks: mic={} sys={}", mic_chunks.len(), sys_chunks.len());
    let (turn_sender, mut turn_rx) = mpsc::unbounded_channel();
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });
    let mut processor = match OnlineDiarizationProcessor::new(
        mode,
        8,
        true,
        Path::new(&models_dir),
        Some(turn_sender),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("  PROCESSOR INIT FAILED: {}", e);
            return;
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
    println!("  fed all chunks; error state: {}", processor.is_in_error_state());
    let mut turns = Vec::new();
    while let Ok(t) = turn_rx.try_recv() {
        turns.push(t);
    }
    println!("  live turns emitted: {}", turns.len());
    match processor.finalize(transcripts) {
        Ok((assignments, clusters, _)) => {
            println!(
                "  finalize OK: {} assignments, mic clusters {}, sys clusters {}",
                assignments.len(),
                clusters.mic.len(),
                clusters.sys.len()
            );
        }
        Err(e) => {
            println!("  FINALIZE FAILED: {}", e);
        }
    }
}

async fn run_mode_ms(
    mode: DiarizationMode,
    audio_path: &Path,
    transcripts: &[TranscriptSegment],
    label: &str,
    max_speakers: usize,
) {
    println!("===== MODE {:?} ms={} ({}) =====", mode, max_speakers, label);
    let decoded = match decode_audio_file(audio_path) {
        Ok(d) => d,
        Err(e) => {
            println!("  DECODE FAILED: {}", e);
            return;
        }
    };
    let (left, right) = decoded.extract_channels();
    let mic = left.unwrap_or_default();
    let sys = right.unwrap_or_default();
    let (mic_chunks, sys_chunks) = (
        make_chunks(&mic, decoded.sample_rate, DeviceType::Microphone, 0.6),
        make_chunks(&sys, decoded.sample_rate, DeviceType::System, 0.6),
    );
    let (turn_sender, _turn_rx) = mpsc::unbounded_channel();
    let models_dir = std::env::var("MEETILY_MODELS_DIR").unwrap_or_else(|_| {
        "C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/models".to_string()
    });
    let mut processor = match OnlineDiarizationProcessor::new(
        mode,
        max_speakers,
        true,
        Path::new(&models_dir),
        Some(turn_sender),
        None,
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("  PROCESSOR INIT FAILED: {}", e);
            return;
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
    println!("  fed all chunks; error state: {}", processor.is_in_error_state());
    match processor.finalize(transcripts) {
        Ok((assignments, clusters, _)) => {
            println!(
                "  finalize OK: {} assignments, mic clusters {}, sys clusters {}",
                assignments.len(),
                clusters.mic.len(),
                clusters.sys.len()
            );
        }
        Err(e) => {
            println!("  FINALIZE FAILED: {}", e);
        }
    }
}

fn tseg(seq: u64, start: f64, end: f64) -> TranscriptSegment {
    TranscriptSegment {
        id: format!("seg_{}", seq),
        text: String::new(),
        audio_start_time: start,
        audio_end_time: end,
        duration: end - start,
        display_time: String::new(),
        confidence: 1.0,
        sequence_id: seq,
        source_device: "System".to_string(),
        tokens: None,
    }
}
