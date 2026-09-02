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
    let config = DiarizationConfig::default();
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
