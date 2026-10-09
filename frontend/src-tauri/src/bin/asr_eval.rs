//! asr-eval: developer-only headless harness that transcribes a recorded
//! meeting (stereo: left = microphone, right = system) through Meetily's own
//! live and offline transcription paths and writes the rows as JSON for the
//! transcript_compare benchmark. No Tauri app, database or settings store.
//!
//! offline: the retranscription recipe — decode, per-channel resample to
//! 16 kHz, `VadConfig::batch()` VAD, `merge_segments(.., 2000.0, 25 s)`,
//! skip < 1600 samples.
//! live: the recording pipeline's recipe — the channel at its device rate
//! into `ContinuousVadProcessor` with `VadConfig::live()`, 200 ms dispatch
//! windows, flush on a >= 500 ms gap or > 25 s of pending speech,
//! `merge_segments(.., 500.0, 25 s)`, the live minimum length, then flush at
//! stop. A file replay has no dropped samples, so VAD-counter time is the file
//! offset.
//! offline decodes with `transcribe_audio_with_confidence` and no prompt, as
//! retranscription does; live decodes with `transcribe_audio_with_tokens` and
//! the channel's previous transcript as the initial prompt, as the single live
//! transcription worker does.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::retranscription::resample_channel_to_16k;
use app_lib::audio::vad::{
    get_speech_chunks, merge_segments, ContinuousVadProcessor, SpeechSegment, VadConfig,
};
use app_lib::whisper_engine::WhisperEngine;
use clap::Parser;

const MAX_SEGMENT_SAMPLES: usize = 25 * 16000;

#[derive(Parser)]
#[command(
    name = "asr-eval",
    about = "Transcribe a stereo meeting recording through Meetily's live and/or offline paths."
)]
struct Args {
    /// Recording (audio.mp4: left = microphone, right = system)
    input: PathBuf,
    /// Directory holding the ggml model file (catalog file name, e.g. ggml-large-v3.bin)
    #[arg(long)]
    models_dir: PathBuf,
    /// Whisper catalog model name (large-v3, large-v3-turbo, ...)
    #[arg(long, default_value = "large-v3")]
    model: String,
    /// Transcription language as the app passes it
    #[arg(long, default_value = "ru")]
    language: String,
    /// live, offline or both
    #[arg(long, default_value = "both")]
    mode: String,
    /// Output directory; writes asr_meetily-rs-<model>-<mode>.json
    #[arg(long)]
    out_dir: PathBuf,
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = Args::parse();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    match rt.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("asr-eval: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The recording pipeline's live chunking for one channel at its device rate.
fn live_segments(samples: &[f32], sample_rate: u32) -> Result<Vec<SpeechSegment>, String> {
    let config = VadConfig::live();
    let min_samples = config.min_segment_samples;
    let mut vad = ContinuousVadProcessor::new(sample_rate, config).map_err(|e| e.to_string())?;
    let threshold = (sample_rate as f32 * 0.2) as usize;
    let mut pending: Vec<SpeechSegment> = Vec::new();
    let mut out: Vec<SpeechSegment> = Vec::new();

    let flush = |pending: &mut Vec<SpeechSegment>, out: &mut Vec<SpeechSegment>| {
        if pending.is_empty() {
            return;
        }
        let merged = merge_segments(pending, 500.0, MAX_SEGMENT_SAMPLES);
        pending.clear();
        out.extend(merged.into_iter().filter(|s| s.samples.len() >= min_samples));
    };

    for block in samples.chunks(threshold) {
        let segments = vad.process_audio(block).map_err(|e| e.to_string())?;
        let gap = segments.first().and_then(|first| {
            pending
                .last()
                .map(|last| first.start_timestamp_ms - last.end_timestamp_ms >= 500.0)
        });
        if gap.unwrap_or(false) {
            flush(&mut pending, &mut out);
        }
        pending.extend(segments);
        let total_ms: f64 = pending
            .iter()
            .map(|s| s.end_timestamp_ms - s.start_timestamp_ms)
            .sum();
        if total_ms > 25_000.0 {
            flush(&mut pending, &mut out);
        }
    }
    pending.extend(vad.flush().map_err(|e| e.to_string())?);
    flush(&mut pending, &mut out);
    Ok(out)
}

/// The retranscription chunking for one channel already at 16 kHz.
fn offline_segments(samples_16k: &[f32]) -> Result<Vec<SpeechSegment>, String> {
    let segments = get_speech_chunks(samples_16k, VadConfig::batch()).map_err(|e| e.to_string())?;
    Ok(merge_segments(&segments, 2000.0, MAX_SEGMENT_SAMPLES)
        .into_iter()
        .filter(|s| s.samples.len() >= 1600)
        .collect())
}

async fn run(args: Args) -> Result<(), String> {
    let modes: Vec<&str> = match args.mode.as_str() {
        "both" => vec!["live", "offline"],
        "live" => vec!["live"],
        "offline" => vec!["offline"],
        other => return Err(format!("unknown --mode {other}")),
    };
    let decoded = decode_audio_file(&args.input).map_err(|e| e.to_string())?;
    if decoded.channels != 2 {
        return Err(format!("{} is not stereo", args.input.display()));
    }
    let (left, right) = decoded.extract_channels();
    let channels = [
        ("Microphone", left.unwrap_or_default()),
        ("System", right.unwrap_or_default()),
    ];

    let engine = WhisperEngine::new_with_models_dir(Some(args.models_dir.clone()))
        .map_err(|e| e.to_string())?;
    engine.discover_models().await.map_err(|e| e.to_string())?;
    engine.load_model(&args.model).await.map_err(|e| e.to_string())?;

    for mode in modes {
        let started = Instant::now();
        let mut rows = Vec::new();
        for (source, samples) in &channels {
            let mut previous = String::new();
            let segments = if mode == "live" {
                live_segments(samples, decoded.sample_rate)?
            } else {
                offline_segments(&resample_channel_to_16k(samples, decoded.sample_rate))?
            };
            eprintln!("{mode} {source}: {} chunks", segments.len());
            for seg in segments {
                let language = Some(args.language.clone());
                let text = if mode == "live" {
                    let prompt = (!previous.is_empty()).then(|| previous.clone());
                    engine.transcribe_audio_with_tokens(seg.samples, language, prompt).await.map(|r| r.0)
                } else {
                    engine.transcribe_audio_with_confidence(seg.samples, language, None).await.map(|r| r.0)
                }
                .map_err(|e| e.to_string())?;
                let text = text.trim();
                if !text.is_empty() {
                    previous = text.to_string();
                    rows.push(serde_json::json!({
                        "start": seg.start_timestamp_ms / 1000.0,
                        "end": seg.end_timestamp_ms / 1000.0,
                        "text": text,
                        "source": source,
                    }));
                }
            }
        }
        rows.sort_by(|a, b| a["start"].as_f64().partial_cmp(&b["start"].as_f64()).unwrap());
        let name = format!("meetily-rs-{}-{mode}", args.model);
        let doc = serde_json::json!({
            "model": name,
            "elapsed": started.elapsed().as_secs_f64(),
            "segments": rows,
        });
        std::fs::create_dir_all(&args.out_dir).map_err(|e| e.to_string())?;
        let path = args.out_dir.join(format!("asr_{name}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap())
            .map_err(|e| e.to_string())?;
        println!(
            "{name} done {} rows {:.0} s",
            doc["segments"].as_array().unwrap().len(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}
