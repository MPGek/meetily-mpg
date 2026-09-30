//! online-eval: developer-only headless harness that replays a WAV through
//! Meetily's production *live* diarization path (Fast mode) and writes the
//! finalized RTTM plus a streaming event sidecar. No Tauri app, database,
//! recording session, or voiceprint registry.
//!
//! The chunking in front of the processor is the production recipe, not a
//! reimplementation: the same `ContinuousVadProcessor`, the same 200 ms
//! dispatch window, the same 500 ms gap and 25 s accumulation flush triggers,
//! the same `merge_segments(.., 500.0, 25 * 16000)` and the same
//! `VadConfig::live()` minimum-length filter the recording pipeline uses
//! (add-online-diarization-eval D2). A fixed-window policy exists only as an
//! explicitly labelled ablation and never as the basis of a parity claim.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::diarization::streaming::processor::{EmittedTurn, OnlineDiarizationProcessor};
use app_lib::audio::diarization::telemetry::DiarizationMode;
use app_lib::audio::recording_state::{AudioChunk, DeviceType};
use app_lib::audio::vad::{merge_segments, ContinuousVadProcessor, SpeechSegment, VadConfig};
use clap::Parser;

/// The sample rate the live path works in; the normalized eval datasets are
/// already at this rate, so the harness refuses anything else rather than
/// resampling with a different resampler than production uses.
const LIVE_SAMPLE_RATE: u32 = 16000;

/// 200 ms of input samples, the production VAD dispatch window.
const VAD_DISPATCH_SECS: f32 = 0.2;

/// Flush when a new segment starts this far beyond the pending tail.
const FLUSH_GAP_MS: f64 = 500.0;

/// Flush when the pending segments would exceed this much speech.
const FLUSH_ACCUMULATED_MS: f64 = 25_000.0;

#[derive(Parser)]
#[command(
    name = "online-eval",
    about = "Replay a WAV through Meetily's live (Fast-mode) diarization and write RTTM + event sidecar."
)]
struct Args {
    /// Input WAV file path (16 kHz mono, as the normalized datasets ship)
    input: PathBuf,
    /// Output RTTM path (default: <input stem>.rttm next to --out-dir or the input)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Directory for the run's artifacts; created when missing
    #[arg(long)]
    out_dir: Option<PathBuf>,
    /// Explicit diarization models directory (skips the default search fallback)
    #[arg(long)]
    models_dir: Option<PathBuf>,
    /// Maximum number of speakers (omit for the configured ceiling)
    #[arg(long)]
    max_speakers: Option<i32>,
    /// Chunking policy: production (default, the recording pipeline's recipe)
    /// or fixed:<secs> (an ablation, never valid for a parity claim)
    #[arg(long, default_value = "production")]
    chunking: String,
    /// Recording URI field in RTTM lines (default: input file stem)
    #[arg(long)]
    uri: Option<String>,
    /// Skip the stop-time speaker refinement (05b D1), so the finalized
    /// hypothesis is the identities the session showed live. The app runs the
    /// refinement by default, so this is the A/B arm, not the default.
    #[arg(long)]
    no_final_recluster: bool,
    /// AHC merge threshold for the stop-time refinement (default: the app's
    /// built-in refinement threshold). For sweeps; ignored with
    /// `--no-final-recluster`.
    #[arg(long)]
    final_recluster_threshold: Option<f32>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("online-eval: {e}");
            ExitCode::FAILURE
        }
    }
}

/// How the replay cuts the recording into the chunks the processor receives.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChunkingPolicy {
    /// The recording pipeline's own recipe. The only policy a parity claim or
    /// a gate may be based on.
    Production,
    /// Fixed-length windows with no VAD and no merge. An ablation: it changes
    /// the segmentation the pipeline sees, so its numbers are not comparable
    /// to production.
    FixedWindow { secs: f64 },
}

impl ChunkingPolicy {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.eq_ignore_ascii_case("production") {
            return Ok(Self::Production);
        }
        if let Some(rest) = raw.strip_prefix("fixed:") {
            let secs: f64 = rest
                .trim()
                .parse()
                .map_err(|_| format!("invalid --chunking '{raw}' (expected fixed:<secs>)"))?;
            if !(secs.is_finite() && secs > 0.0) {
                return Err(format!("invalid --chunking '{raw}': window must be > 0"));
            }
            return Ok(Self::FixedWindow { secs });
        }
        Err(format!(
            "invalid --chunking '{raw}' (expected production or fixed:<secs>)"
        ))
    }

    fn label(&self) -> String {
        match self {
            Self::Production => "production".to_string(),
            Self::FixedWindow { secs } => format!("fixed:{secs}"),
        }
    }

    /// Whether this policy reproduces the recording pipeline's segmentation.
    fn production_faithful(&self) -> bool {
        matches!(self, Self::Production)
    }
}

/// Replay the production chunking recipe over a decoded recording.
///
/// The one fidelity nuance (D2): production anchors VAD-counter time against a
/// real capture clock because a device can drop samples. A file replay has no
/// drops, so that mapping is the identity and VAD-counter time *is* the file
/// offset. The caller asserts the resulting times against the recording
/// duration.
fn production_chunks(samples: &[f32], sample_rate: u32) -> Result<Vec<AudioChunk>, String> {
    let vad_config = VadConfig::live();
    let min_segment_samples = vad_config.min_segment_samples;
    let mut vad = ContinuousVadProcessor::new(sample_rate, vad_config)
        .map_err(|e| format!("failed to create the live VAD processor: {e}"))?;

    let dispatch_threshold = (sample_rate as f32 * VAD_DISPATCH_SECS) as usize;
    let mut pending: Vec<SpeechSegment> = Vec::new();
    let mut chunks: Vec<AudioChunk> = Vec::new();
    let mut buffer: Vec<f32> = Vec::with_capacity(dispatch_threshold * 2);

    let dispatch = |buffer: &mut Vec<f32>,
                        vad: &mut ContinuousVadProcessor,
                        pending: &mut Vec<SpeechSegment>,
                        chunks: &mut Vec<AudioChunk>|
     -> Result<(), String> {
        let accumulated: Vec<f32> = std::mem::take(buffer);
        let mut segments = vad
            .process_audio(&accumulated)
            .map_err(|e| format!("VAD failed: {e}"))?;

        // Flush when the first new segment is distant from the pending tail.
        let should_flush = segments
            .first()
            .and_then(|first| {
                pending
                    .last()
                    .map(|last| (first.start_timestamp_ms - last.end_timestamp_ms) >= FLUSH_GAP_MS)
            })
            .unwrap_or(false);
        if should_flush {
            flush_pending(pending, min_segment_samples, chunks);
        }

        pending.append(&mut segments);

        let total_ms: f64 = pending
            .iter()
            .map(|s| s.end_timestamp_ms - s.start_timestamp_ms)
            .sum();
        if total_ms > FLUSH_ACCUMULATED_MS {
            flush_pending(pending, min_segment_samples, chunks);
        }
        Ok(())
    };

    for block in samples.chunks(dispatch_threshold) {
        buffer.extend_from_slice(block);
        if buffer.len() >= dispatch_threshold {
            dispatch(&mut buffer, &mut vad, &mut pending, &mut chunks)?;
        }
    }
    // Recording stop, as `flush_remaining_audio` does it: dispatch whatever is
    // still buffered, then force the VAD to close any speech still open (the
    // tail would otherwise never be reported), then merge and emit.
    if !buffer.is_empty() {
        dispatch(&mut buffer, &mut vad, &mut pending, &mut chunks)?;
    }
    match vad.flush() {
        Ok(mut tail) => pending.append(&mut tail),
        Err(e) => return Err(format!("VAD flush failed: {e}")),
    }
    flush_pending(&mut pending, min_segment_samples, &mut chunks);

    Ok(chunks)
}

/// The pipeline's flush: merge segments closer than 500 ms, drop anything
/// below the live minimum length, and emit one chunk per merged segment.
fn flush_pending(
    pending: &mut Vec<SpeechSegment>,
    min_segment_samples: usize,
    chunks: &mut Vec<AudioChunk>,
) {
    if pending.is_empty() {
        return;
    }
    let merged = merge_segments(pending, FLUSH_GAP_MS, 25 * LIVE_SAMPLE_RATE as usize);
    // The pipeline clears the accumulator after dispatching the merged set.
    pending.clear();
    for segment in merged {
        if segment.samples.len() < min_segment_samples {
            continue;
        }
        let chunk_id = chunks.len() as u64;
        chunks.push(AudioChunk {
            data: segment.samples,
            sample_rate: LIVE_SAMPLE_RATE,
            timestamp: segment.start_timestamp_ms / 1000.0,
            chunk_id,
            device_type: DeviceType::Microphone,
            channels: 1,
        });
    }
}

/// The ablation policy: fixed windows, no VAD, no merge.
fn fixed_window_chunks(samples: &[f32], sample_rate: u32, secs: f64) -> Vec<AudioChunk> {
    let per_chunk = ((sample_rate as f64) * secs).max(1.0) as usize;
    samples
        .chunks(per_chunk)
        .enumerate()
        .map(|(i, block)| AudioChunk {
            data: block.to_vec(),
            sample_rate,
            timestamp: (i * per_chunk) as f64 / sample_rate as f64,
            chunk_id: i as u64,
            device_type: DeviceType::Microphone,
            channels: 1,
        })
        .collect()
}

/// One emission as the sidecar records it.
struct Emission {
    index: usize,
    start: f64,
    end: f64,
    speaker: String,
    stable: bool,
    display_name: Option<String>,
    matched_by: Option<String>,
    match_score: Option<f32>,
}

impl Emission {
    fn from(index: usize, emitted: &EmittedTurn) -> Self {
        Self {
            index,
            start: emitted.turn.start_time,
            end: emitted.turn.end_time,
            speaker: emitted.turn.speaker.clone(),
            stable: emitted.stable,
            display_name: emitted.turn.display_name.clone(),
            matched_by: emitted.turn.matched_by.clone(),
            match_score: emitted.turn.match_score,
        }
    }
}

/// The finalized timeline: the spans resolved so a later span wins the region
/// it covers (D6 — "the last label per region", which is what a saved
/// transcript would show). Adjacent same-speaker spans are joined.
///
/// The spans are what the stop-time `finalize` published for the channel: the
/// refined timeline when the refinement ran, the incremental turns when it did
/// not. Deriving this from the emission stream instead would leave the
/// refinement invisible to the measurement, and `live_final_flip` at zero for
/// ever.
fn finalized_timeline(
    spans: impl IntoIterator<Item = (f64, f64, String)>,
) -> Vec<(f64, f64, String)> {
    let mut out: Vec<(f64, f64, String)> = Vec::new();
    for (start, end, speaker) in spans {
        if end <= start {
            continue;
        }
        let mut kept: Vec<(f64, f64, String)> = Vec::with_capacity(out.len() + 1);
        for (a, b, label) in out.into_iter() {
            if b <= start || a >= end {
                kept.push((a, b, label));
                continue;
            }
            if a < start {
                kept.push((a, start, label.clone()));
            }
            if b > end {
                kept.push((end, b, label));
            }
        }
        kept.push((start, end, speaker));
        kept.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        out = kept;
    }

    let mut joined: Vec<(f64, f64, String)> = Vec::with_capacity(out.len());
    for (start, end, label) in out {
        match joined.last_mut() {
            Some((_, prev_end, prev_label))
                if *prev_label == label && (start - *prev_end).abs() < 1e-6 =>
            {
                *prev_end = end;
            }
            _ => joined.push((start, end, label)),
        }
    }
    joined
}

fn run(args: Args) -> Result<(), String> {
    let policy = ChunkingPolicy::parse(&args.chunking)?;
    let decoded = decode_audio_file(&args.input)
        .map_err(|e| format!("failed to decode {}: {e}", args.input.display()))?;
    if decoded.samples.is_empty() {
        return Err(format!("{} decoded to no audio", args.input.display()));
    }
    if decoded.sample_rate != LIVE_SAMPLE_RATE {
        return Err(format!(
            "{} is {} Hz; the live path works at {} Hz. Use the normalized dataset audio \
             (eval normalizes to 16 kHz mono) rather than resampling here with a different \
             resampler than production uses.",
            args.input.display(),
            decoded.sample_rate,
            LIVE_SAMPLE_RATE
        ));
    }
    // One canonical channel, fed as the microphone with no system device, so
    // the session is mono and labels come out as SPEAKER_NN (D3).
    let (left, _right) = decoded.extract_channels();
    let samples = left.ok_or_else(|| format!("no audio channel in {}", args.input.display()))?;
    let duration_secs = samples.len() as f64 / decoded.sample_rate as f64;

    let uri = args
        .uri
        .clone()
        .or_else(|| {
            args.input
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());

    let chunks = match policy {
        ChunkingPolicy::Production => production_chunks(&samples, decoded.sample_rate)?,
        ChunkingPolicy::FixedWindow { secs } => {
            fixed_window_chunks(&samples, decoded.sample_rate, secs)
        }
    };
    for chunk in &chunks {
        if chunk.timestamp > duration_secs + 1e-6 {
            return Err(format!(
                "chunk timestamp {:.3}s exceeds the recording duration {:.3}s",
                chunk.timestamp, duration_secs
            ));
        }
    }

    let models_dir = match &args.models_dir {
        Some(dir) => dir.clone(),
        None => app_lib::audio::diarization::resolve_models_dir_standalone(None)?,
    };
    // The app resolves its parameters from the process-wide settings; the
    // harness has no settings store, so it states its configuration itself
    // (the built-in defaults plus the flags it was given) and records the
    // refinement's parameters in the header.
    let final_recluster = !args.no_final_recluster;
    let config = app_lib::audio::diarization::DiarizationConfig {
        final_recluster,
        final_recluster_threshold: args
            .final_recluster_threshold
            .unwrap_or(app_lib::audio::diarization::DEFAULT_FINAL_RECLUSTER_THRESHOLD),
        ..app_lib::audio::diarization::DiarizationConfig::default()
    };
    let final_recluster_threshold = config.final_recluster_threshold;
    let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel::<EmittedTurn>();
    let mut processor = OnlineDiarizationProcessor::new(
        DiarizationMode::Fast,
        args.max_speakers.filter(|m| *m > 0).unwrap_or(0) as usize,
        false,
        &models_dir,
        None,
        None,
    )?;
    processor.attach_emission_sink(sink_tx);
    processor.set_session_config(config);

    let started = Instant::now();
    for chunk in &chunks {
        processor.process_chunk(chunk.clone());
    }
    let elapsed = started.elapsed().as_secs_f64();

    // Stop the way the app does. The finalized hypothesis is what the stop-time
    // pass produces, and the cost of producing it is measured on its own: it is
    // paid exactly when the user is waiting for the meeting to save (05b D1).
    // A failure here is a failure of the recording, not something to paper over
    // with the live labels.
    let finalize_started = Instant::now();
    let (_assignments, _clusters, _bindings, display_pass) = processor
        .finalize(&[])
        .map_err(|e| format!("stop-time finalize failed for {uri}: {e}"))?;
    let finalize_secs = finalize_started.elapsed().as_secs_f64();
    drop(processor);

    let mut emissions: Vec<Emission> = Vec::new();
    while let Ok(emitted) = sink_rx.try_recv() {
        emissions.push(Emission::from(emissions.len(), &emitted));
    }

    // One canonical channel is fed as the microphone (see above).
    let timeline = finalized_timeline(
        display_pass
            .channels
            .iter()
            .filter(|channel| channel.source_device == "Microphone")
            .flat_map(|channel| channel.spans.iter().cloned()),
    );
    let header = serde_json::json!({
        "record": "header",
        "uri": uri,
        "mode": "fast",
        "chunking": policy.label(),
        "production_faithful": policy.production_faithful(),
        "model_family": app_lib::audio::embedder::ENHANCED_MODEL_TAG,
        "sample_rate": decoded.sample_rate,
        "duration_secs": duration_secs,
        "chunks": chunks.len(),
        "emissions": emissions.len(),
        "finalized_segments": timeline.len(),
        "final_recluster": final_recluster,
        "final_recluster_threshold": final_recluster_threshold,
    });

    let out_dir = args.out_dir.clone();
    if let Some(dir) = &out_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let artifact = |suffix: &str| -> PathBuf {
        match &out_dir {
            Some(dir) => dir.join(format!("{uri}{suffix}")),
            None => args.input.with_extension(suffix.trim_start_matches('.')),
        }
    };

    let rttm_path = args.out.clone().unwrap_or_else(|| artifact(".rttm"));
    write_rttm(&rttm_path, &uri, &timeline)?;
    write_sidecar(&artifact(".events.jsonl"), &header, &emissions)?;
    // Wall-clock, so it lives outside the byte-identical artifacts: the
    // sidecar and the RTTM must be reproducible run to run (task 2.6), while
    // the real-time factor is a property of this machine (D7 — recorded,
    // never gated).
    write_timing(
        &artifact(".timing.json"),
        &uri,
        duration_secs,
        elapsed,
        finalize_secs,
    )?;

    println!(
        "online-eval: uri={uri} mode=fast chunking={} production_faithful={} model_family={} \
         sample_rate={} duration={:.3}s chunks={} emissions={} finalized={} final_recluster={} rtf={:.3}",
        policy.label(),
        policy.production_faithful(),
        app_lib::audio::embedder::ENHANCED_MODEL_TAG,
        decoded.sample_rate,
        duration_secs,
        chunks.len(),
        emissions.len(),
        timeline.len(),
        final_recluster,
        if duration_secs > 0.0 {
            elapsed / duration_secs
        } else {
            0.0
        }
    );
    Ok(())
}

fn write_rttm(path: &Path, uri: &str, timeline: &[(f64, f64, String)]) -> Result<(), String> {
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?,
    );
    for (start, end, label) in timeline {
        writeln!(
            out,
            "SPEAKER {} 1 {:.3} {:.3} <NA> <NA> {} <NA> <NA>",
            uri,
            start,
            (end - start).max(0.0),
            label
        )
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    }
    out.flush()
        .map_err(|e| format!("failed to flush {}: {e}", path.display()))
}

fn write_sidecar(
    path: &Path,
    header: &serde_json::Value,
    emissions: &[Emission],
) -> Result<(), String> {
    let mut out = std::io::BufWriter::new(
        std::fs::File::create(path).map_err(|e| format!("cannot write {}: {e}", path.display()))?,
    );
    writeln!(out, "{header}").map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    for emission in emissions {
        let record = serde_json::json!({
            "record": "emission",
            "index": emission.index,
            "start": emission.start,
            "end": emission.end,
            "speaker": emission.speaker,
            "stable": emission.stable,
            "display_name": emission.display_name,
            "matched_by": emission.matched_by,
            "match_score": emission.match_score,
        });
        writeln!(out, "{record}")
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    }
    out.flush()
        .map_err(|e| format!("failed to flush {}: {e}", path.display()))
}

fn write_timing(
    path: &Path,
    uri: &str,
    duration_secs: f64,
    elapsed_secs: f64,
    finalize_secs: f64,
) -> Result<(), String> {
    let rtf = if duration_secs > 0.0 {
        elapsed_secs / duration_secs
    } else {
        0.0
    };
    let payload = serde_json::json!({
        "uri": uri,
        "mode": "fast",
        "audio_secs": duration_secs,
        "wall_secs": elapsed_secs,
        "real_time_factor": rtf,
        // The stop-time pass on its own, kept out of `real_time_factor` so that
        // figure stays comparable with runs recorded before the pass existed.
        "finalize_secs": finalize_secs,
    });
    std::fs::write(path, format!("{payload}\n"))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built speech segment, as `ContinuousVadProcessor` would report
    /// one: sample-domain payload plus its recording-relative span in ms.
    fn segment(start_ms: f64, end_ms: f64) -> SpeechSegment {
        let samples = (((end_ms - start_ms) / 1000.0) * LIVE_SAMPLE_RATE as f64) as usize;
        SpeechSegment {
            samples: vec![0.1f32; samples],
            start_timestamp_ms: start_ms,
            end_timestamp_ms: end_ms,
            confidence: 0.9,
        }
    }

    /// Real speech from the normalized eval dataset, or `None` when it is not
    /// present so the test skips instead of asserting on synthetic audio the
    /// production VAD does not call speech.
    fn dataset_speech(seconds: f64) -> Option<Vec<f32>> {
        let explicit = std::env::var("MEETILY_EVAL_WAV")
            .ok()
            .map(std::path::PathBuf::from);
        let path = explicit.or_else(|| {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()?
                .parent()?
                .join("eval/data/voxconverse-dev/wav");
            std::fs::read_dir(dir)
                .ok()?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "wav"))
                .min()
        })?;
        let decoded = decode_audio_file(&path).ok()?;
        if decoded.sample_rate != LIVE_SAMPLE_RATE {
            return None;
        }
        let wanted = (LIVE_SAMPLE_RATE as f64 * seconds) as usize;
        let samples: Vec<f32> = decoded.samples.into_iter().take(wanted).collect();
        (!samples.is_empty()).then_some(samples)
    }

    #[test]
    fn chunking_policy_parses_production_and_the_ablation() {
        assert_eq!(
            ChunkingPolicy::parse("production").unwrap(),
            ChunkingPolicy::Production
        );
        assert!(ChunkingPolicy::parse("production").unwrap().production_faithful());
        let ablation = ChunkingPolicy::parse("fixed:0.6").unwrap();
        assert_eq!(ablation, ChunkingPolicy::FixedWindow { secs: 0.6 });
        assert!(!ablation.production_faithful());
        assert_eq!(ablation.label(), "fixed:0.6");
        assert!(ChunkingPolicy::parse("fixed:0").is_err());
        assert!(ChunkingPolicy::parse("sliding").is_err());
    }

    /// Task 2.2, the flush rules themselves: segments closer than 500 ms are
    /// one chunk, a 1 s gap splits, and anything below the live minimum length
    /// is dropped. Driven with hand-built segments so it holds without models
    /// or dataset audio.
    #[test]
    fn flush_merges_short_gaps_splits_long_ones_and_drops_short_speech() {
        let min_samples = VadConfig::live().min_segment_samples;
        let mut pending = vec![
            segment(0.0, 1200.0),    // speech
            segment(1300.0, 2000.0), // 100 ms later: merges with the first
            segment(3000.0, 3020.0), // 1 s gap before it, 1.08 s after: stays
                                     // alone and is dropped as too short
            segment(4100.0, 5200.0), // 1.08 s later: its own chunk
        ];
        let mut chunks = Vec::new();
        flush_pending(&mut pending, min_samples, &mut chunks);

        assert!(
            pending.is_empty(),
            "flushing consumes the pending segments, as the pipeline does"
        );
        assert_eq!(
            chunks.len(),
            2,
            "the 100 ms gap merges, the 1 s gaps split, and the isolated 20 ms              segment falls below the live minimum length"
        );
        assert!((chunks[0].timestamp - 0.0).abs() < 1e-9);
        assert!(
            (chunks[0].data.len() as f64 / LIVE_SAMPLE_RATE as f64 - 2.0).abs() < 0.01,
            "the merged chunk carries both segments plus the bridged 100 ms gap,              so it stays contiguous audio (1.2 + 0.1 + 0.7 s)"
        );
        assert!(
            (chunks[1].timestamp - 4.1).abs() < 1e-9,
            "the second chunk starts at the surviving segment, not at the dropped one"
        );
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_id, i as u64);
            assert_eq!(chunk.sample_rate, LIVE_SAMPLE_RATE);
            assert_eq!(chunk.channels, 1);
            assert_eq!(chunk.device_type, DeviceType::Microphone);
            assert!(chunk.data.len() >= min_samples);
        }
    }

    /// Task 2.2, the whole recipe over real speech: a known 1 s silence in the
    /// middle must appear as a chunk boundary, and no chunk may be timestamped
    /// past the recording.
    #[test]
    fn production_chunking_over_real_speech_respects_a_known_gap() {
        let Some(speech) = dataset_speech(4.0) else {
            eprintln!("skipping: no dataset speech (set MEETILY_EVAL_WAV)");
            return;
        };
        let half = speech.len() / 2;
        let mut samples = speech[..half].to_vec();
        let gap_start = samples.len() as f64 / LIVE_SAMPLE_RATE as f64;
        samples.extend(std::iter::repeat_n(0.0f32, LIVE_SAMPLE_RATE as usize));
        let gap_end = samples.len() as f64 / LIVE_SAMPLE_RATE as f64;
        samples.extend_from_slice(&speech[half..]);
        let duration = samples.len() as f64 / LIVE_SAMPLE_RATE as f64;

        let chunks = production_chunks(&samples, LIVE_SAMPLE_RATE).expect("chunking runs");
        eprintln!(
            "gap {:.3}-{:.3}s of {:.3}s; chunks: {:?}",
            gap_start,
            gap_end,
            duration,
            chunks
                .iter()
                .map(|c| (
                    (c.timestamp * 1000.0).round() / 1000.0,
                    (c.data.len() as f64 / LIVE_SAMPLE_RATE as f64 * 1000.0).round() / 1000.0
                ))
                .collect::<Vec<_>>()
        );
        assert!(!chunks.is_empty(), "real speech must produce chunks");
        for chunk in &chunks {
            assert!(
                chunk.timestamp <= duration + 1e-6,
                "chunk at {:.3}s is past the {:.3}s recording",
                chunk.timestamp,
                duration
            );
            assert!(chunk.data.len() >= VadConfig::live().min_segment_samples);
        }
        for pair in chunks.windows(2) {
            assert!(
                pair[1].timestamp > pair[0].timestamp,
                "chunk timestamps must increase"
            );
        }
        // No chunk may start inside the inserted silence, and speech on both
        // sides of it must be covered by different chunks.
        assert!(
            chunks
                .iter()
                .all(|c| c.timestamp <= gap_start + 0.25 || c.timestamp >= gap_end - 0.25),
            "a chunk started inside the inserted silence: {:?}",
            chunks.iter().map(|c| c.timestamp).collect::<Vec<_>>()
        );
        assert!(
            chunks.iter().any(|c| c.timestamp >= gap_end - 0.25),
            "the speech after the gap must be chunked too"
        );
        assert!(
            chunks.len() >= 2,
            "the 1 s silence must split the recording, got {} chunk(s) at {:?}",
            chunks.len(),
            chunks.iter().map(|c| c.timestamp).collect::<Vec<_>>()
        );
    }

    #[test]
    fn fixed_window_chunking_covers_the_recording_in_order() {
        let samples = vec![0.25f32; LIVE_SAMPLE_RATE as usize * 3];
        let chunks = fixed_window_chunks(&samples, LIVE_SAMPLE_RATE, 0.6);
        assert_eq!(chunks.len(), 5);
        assert!((chunks[0].timestamp - 0.0).abs() < 1e-9);
        assert!((chunks[4].timestamp - 2.4).abs() < 1e-9);
        assert_eq!(
            chunks.iter().map(|c| c.data.len()).sum::<usize>(),
            samples.len()
        );
    }

    /// D6: the finalized timeline keeps the *last* label for a region, so a
    /// span that relabels an earlier one wins it.
    #[test]
    fn finalized_timeline_resolves_revisions_last_wins() {
        let timeline = finalized_timeline(vec![
            (0.0, 2.0, "SPEAKER_00".to_string()),
            // a later span relabels the tail of the first
            (1.5, 2.5, "SPEAKER_01".to_string()),
        ]);
        assert_eq!(
            timeline,
            vec![
                (0.0, 1.5, "SPEAKER_00".to_string()),
                (1.5, 2.5, "SPEAKER_01".to_string()),
            ]
        );
    }

    #[test]
    fn finalized_timeline_joins_adjacent_same_speaker_spans() {
        let timeline = finalized_timeline(vec![
            (0.0, 1.0, "SPEAKER_00".to_string()),
            (1.0, 2.0, "SPEAKER_00".to_string()),
            (2.0, 3.0, "SPEAKER_01".to_string()),
        ]);
        assert_eq!(
            timeline,
            vec![
                (0.0, 2.0, "SPEAKER_00".to_string()),
                (2.0, 3.0, "SPEAKER_01".to_string()),
            ]
        );
    }

    /// A zero-length span carries no speech, so it must not reach the RTTM as
    /// a line of its own (the refinement labels one span per buffered window).
    #[test]
    fn finalized_timeline_drops_empty_spans() {
        let timeline = finalized_timeline(vec![
            (1.0, 1.0, "SPEAKER_00".to_string()),
            (1.0, 2.0, "SPEAKER_01".to_string()),
        ]);
        assert_eq!(timeline, vec![(1.0, 2.0, "SPEAKER_01".to_string())]);
    }
}
