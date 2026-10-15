//! diarize-eval: developer-only headless harness that runs Meetily's
//! production offline diarization pipeline on an input WAV and emits RTTM.
//! No Tauri app, database, recording session, or network access required.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use app_lib::audio::decoder::decode_audio_file;
use app_lib::audio::diarization::{diarize_wav_samples, DiarizationConfig};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "diarize-eval",
    about = "Diarize a WAV file with the Meetily offline pipeline and write RTTM."
)]
struct Args {
    /// Input WAV file path
    input: PathBuf,
    /// Output RTTM path (default: stdout)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Explicit diarization models directory (skips the default search fallback)
    #[arg(long)]
    models_dir: Option<PathBuf>,
    /// Maximum number of speakers (omit for automatic)
    #[arg(long)]
    max_speakers: Option<i32>,
    /// AHC merge threshold override (default: built-in 0.60)
    #[arg(long)]
    cluster_threshold: Option<f32>,
    /// Speaker-count ceiling override (default: built-in 128)
    #[arg(long)]
    max_clusters: Option<usize>,
    /// Same-speaker gap-merge window in seconds (default: built-in 0.3)
    #[arg(long)]
    gap_merge: Option<f32>,
    /// Clusterer kind: vbx|nmesc|ahc (default: built-in ahc, 6.2 sweep)
    #[arg(long)]
    clusterer: Option<String>,
    /// Dense embedding window in seconds; 0 = sparse one-embedding-per-segment
    /// (default: built-in 5.0)
    #[arg(long)]
    embed_window: Option<f32>,
    /// Calibrated binarization as onset,offset,min_on,min_off; 'off' disables
    /// (default: built-in hysteresis constants)
    #[arg(long)]
    binarization: Option<String>,
    /// Recording URI field in RTTM lines (default: input file stem)
    #[arg(long)]
    uri: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("diarize-eval: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parse a `--binarization` value: `off` disables calibrated binarization
/// (plain argmax), otherwise `onset,offset,min_on,min_off` floats.
fn parse_binarization(
    raw: &str,
) -> Result<Option<polyvoice::segmentation::BinarizationConfig>, String> {
    if raw.eq_ignore_ascii_case("off") || raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!(
            "invalid --binarization '{raw}' (expected off, or onset,offset,min_on,min_off)"
        ));
    }
    let nums: Vec<f32> = parts
        .iter()
        .map(|p| p.parse::<f32>().map_err(|_| format!("invalid --binarization float '{p}'")))
        .collect::<Result<_, _>>()?;
    Ok(Some(polyvoice::segmentation::BinarizationConfig {
        onset: nums[0],
        offset: nums[1],
        min_duration_on: nums[2],
        min_duration_off: nums[3],
    }))
}

fn run(args: Args) -> Result<(), String> {
    let decoded = decode_audio_file(&args.input)
        .map_err(|e| format!("failed to decode {}: {e}", args.input.display()))?;
    let uri = args
        .uri
        .clone()
        .or_else(|| {
            args.input
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let mut config = DiarizationConfig::default();
    if let Some(t) = args.cluster_threshold {
        config.cluster_threshold = t;
    }
    if let Some(c) = args.max_clusters {
        config.cluster_ceiling = c;
    }
    if let Some(g) = args.gap_merge {
        config.gap_merge_secs = g;
    }
    if let Some(k) = &args.clusterer {
        config.clusterer = app_lib::audio::diarization::ClustererKindSetting::parse(k)
            .ok_or_else(|| format!("invalid --clusterer '{k}' (expected vbx|nmesc|ahc)"))?;
    }
    if let Some(w) = args.embed_window {
        config.embed_window_secs = w;
    }
    if let Some(b) = &args.binarization {
        config.binarization = parse_binarization(b)?;
    }
    let (left, right) = decoded.extract_channels();
    let mut channels: Vec<(u32, Vec<f32>)> = Vec::new();
    if let Some(mic) = left {
        channels.push((1, mic));
    }
    if let Some(sys) = right {
        channels.push((2, sys));
    }
    if channels.is_empty() {
        return Err(format!(
            "no audio channels found in {}",
            args.input.display()
        ));
    }

    let mut out: Box<dyn Write> = match &args.out {
        Some(path) => Box::new(std::io::BufWriter::new(std::fs::File::create(path).map_err(
            |e| format!("cannot write {}: {e}", path.display()),
        )?)),
        None => Box::new(std::io::stdout()),
    };

    for (channel, samples) in &channels {
        let clusters = diarize_wav_samples(
            samples,
            decoded.sample_rate,
            args.max_speakers,
            &config,
            args.models_dir.as_deref(),
        )?;
        for seg in &clusters.segments {
            let duration = (seg.end - seg.start).max(0.0);
            writeln!(
                out,
                "SPEAKER {} {} {:.3} {:.3} <NA> <NA> SPEAKER_{:02} <NA> <NA>",
                uri, channel, seg.start, duration, seg.speaker
            )
            .map_err(|e| format!("failed to write RTTM output: {e}"))?;
        }
    }
    out.flush()
        .map_err(|e| format!("failed to flush RTTM output: {e}"))?;
    Ok(())
}
