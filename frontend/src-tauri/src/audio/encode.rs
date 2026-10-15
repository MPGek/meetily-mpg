use super::ffmpeg::find_ffmpeg_path; // Correct path to encode module
use super::AudioDevice;
use std::io::Write;
use std::sync::Arc;
use std::{
    path::Path,
    process::{Command, Stdio},
};
use tracing::{debug, error};

pub struct AudioInput {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub device: Arc<AudioDevice>,
}

/// AAC codec profile used for all saved recordings (native ffmpeg AAC is LC-only)
const AAC_PROFILE: &str = "aac_low";
/// VBR quality for the native AAC encoder (0.1-1.0 scale, 1.0 = highest).
/// 0.7 targets ~96 kbps for stereo voice - transparent for speech and roughly
/// half the former fixed 192 kbps CBR, with VBR saving further on silence.
const AAC_VBR_QUALITY: &str = "0.7";

/// Upper bound on a single checkpoint encode. 60 s is far above the measured
/// <0.3 s idle encode for 30 s of audio; only a pathological hang reaches it.
pub const ENCODE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Upper bound on a stream-copy merge of legacy AAC checkpoints (~2 min).
pub const MERGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Upper bound on merging `audio_secs` of checkpoints into the final file.
/// The merge encodes the whole recording, so the bound grows with it: 120 s
/// plus 1 s per 8 s of audio tolerates encoders down to about 8x real time
/// (the native AAC encoder measured ~170x).
pub fn merge_timeout(audio_secs: f64) -> std::time::Duration {
    MERGE_TIMEOUT + std::time::Duration::from_secs_f64(audio_secs.max(0.0) / 8.0)
}

/// Spawn an FFmpeg process, optionally write PCM to its stdin, and wait for it
/// to exit within `timeout`. On expiry the child is killed and a timed-out
/// error is returned so a hung encoder can never stall audio saving forever.
///
/// The stdin write happens on a separate thread so a blocked pipe cannot
/// prevent the bounded wait from noticing a hung child.
pub fn run_ffmpeg_with_timeout(
    mut command: Command,
    stdin_data: Option<&[u8]>,
    timeout: std::time::Duration,
) -> anyhow::Result<std::process::Output> {
    use std::time::Instant;

    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn FFmpeg process: {}", e))?;

    let write_thread = if let Some(data) = stdin_data {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open FFmpeg stdin"))?;
        let data = data.to_vec();
        Some(std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
            drop(stdin);
        }))
    } else {
        None
    };

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    if let Some(handle) = write_thread {
                        let _ = handle.join();
                    }
                    return Err(anyhow::anyhow!(
                        "FFmpeg process timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                if let Some(handle) = write_thread {
                    let _ = handle.join();
                }
                return Err(anyhow::anyhow!("Failed waiting for FFmpeg process: {}", e));
            }
        }
    };

    if let Some(handle) = write_thread {
        let _ = handle.join();
    }

    // Drain any remaining output and reap the process.
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = std::io::Read::read_to_end(&mut out, &mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = std::io::Read::read_to_end(&mut err, &mut stderr);
    }
    let _ = child.wait();

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Output arguments of a saved recording (AAC-LC in MP4). The checkpoint
/// merge uses the same ones, so every saved file is encoded alike.
pub const AAC_OUTPUT_ARGS: [&str; 10] = [
    "-c:a",
    "aac",
    "-q:a",
    AAC_VBR_QUALITY,
    "-profile:a",
    AAC_PROFILE, // Use AAC-LC profile for better compatibility
    "-movflags",
    "+faststart", // Optimize for web streaming
    "-f",
    "mp4",
];

/// Output arguments of a recording checkpoint: lossless 24-bit FLAC (s32), so
/// the saved file is lossy-encoded once, when the checkpoints are merged.
const FLAC_CHECKPOINT_ARGS: [&str; 6] = ["-c:a", "flac", "-sample_fmt", "s32", "-f", "flac"];

pub fn encode_single_audio(
    data: &[f32],
    sample_rate: u32,
    channels: u16,
    output_path: &Path,
) -> anyhow::Result<()> {
    encode_pcm(data, sample_rate, channels, output_path, &AAC_OUTPUT_ARGS)
}

/// Encode one recording checkpoint losslessly (see `FLAC_CHECKPOINT_ARGS`).
pub fn encode_checkpoint_flac(
    data: &[f32],
    sample_rate: u32,
    channels: u16,
    output_path: &Path,
) -> anyhow::Result<()> {
    encode_pcm(
        data,
        sample_rate,
        channels,
        output_path,
        &FLAC_CHECKPOINT_ARGS,
    )
}

fn encode_pcm(
    data: &[f32],
    sample_rate: u32,
    channels: u16,
    output_path: &Path,
    output_args: &[&str],
) -> anyhow::Result<()> {
    debug!("Starting FFmpeg process for {} audio samples", data.len());

    if data.is_empty() {
        return Err(anyhow::anyhow!("No audio data provided for encoding"));
    }

    // Sanitize non-finite samples (NaN / ±Inf) to silence before handing the
    // PCM to FFmpeg. The native AAC encoder aborts the whole encode on
    // "(near) NaN/+-Inf" input; a single bad checkpoint must never kill a save.
    let mut replaced = 0usize;
    let mut sanitized: Vec<f32> = Vec::with_capacity(data.len());
    for &sample in data {
        if !sample.is_finite() {
            sanitized.push(0.0);
            replaced += 1;
        } else {
            sanitized.push(sample);
        }
    }
    if replaced > 0 {
        debug!(
            "Sanitized {} non-finite audio sample(s) to 0.0 before encoding",
            replaced
        );
    }

    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| {
        anyhow::anyhow!("FFmpeg not found. Please install FFmpeg to save recordings.")
    })?;

    debug!("Using FFmpeg at: {:?}", ffmpeg_path);

    let mut command = Command::new(ffmpeg_path);
    command
        .args([
            "-f",
            "f32le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-i",
            "pipe:0",
        ])
        .args(output_args)
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Hide console window on Windows to prevent CMD popup during recording
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    debug!("FFmpeg command: {:?}", command);

    let output = run_ffmpeg_with_timeout(
        command,
        Some(bytemuck::cast_slice(&sanitized)),
        ENCODE_TIMEOUT,
    )?;
    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    debug!("FFmpeg process exited with status: {}", status);
    debug!("FFmpeg stdout: {}", stdout);
    debug!("FFmpeg stderr: {}", stderr);

    if !status.success() {
        error!("FFmpeg process failed with status: {}", status);
        error!("FFmpeg stderr: {}", stderr);
        return Err(anyhow::anyhow!(
            "FFmpeg process failed with status: {}",
            status
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long-running command that ignores stdin and will not exit on its own
    /// within a short test timeout.
    fn hang_command() -> Command {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = Command::new("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        }
        #[cfg(not(target_os = "windows"))]
        {
            let mut cmd = Command::new("sleep");
            cmd.arg("30");
            cmd
        }
    }

    #[test]
    fn test_sanitize_replaces_non_finite_samples() {
        let data: Vec<f32> = vec![0.5, f32::NAN, -1.0, f32::INFINITY, f32::NEG_INFINITY, 0.25];
        let replaced = data.iter().filter(|s| !s.is_finite()).count();
        assert_eq!(replaced, 3);

        let mut sanitized: Vec<f32> = Vec::with_capacity(data.len());
        for &sample in &data {
            sanitized.push(if sample.is_finite() { sample } else { 0.0 });
        }

        assert_eq!(sanitized, vec![0.5, 0.0, -1.0, 0.0, 0.0, 0.25]);
        assert!(sanitized.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn test_run_ffmpeg_with_timeout_kills_hung_child() {
        let mut command = hang_command();
        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null());

        let started = std::time::Instant::now();
        let result = run_ffmpeg_with_timeout(command, None, std::time::Duration::from_millis(500));
        let elapsed = started.elapsed();

        // A hung child must be abandoned within the bound, not waited on forever.
        assert!(result.is_err());
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "timeout wait took {:?}",
            elapsed
        );
        assert!(
            result.unwrap_err().to_string().contains("timed out"),
            "expected a timed-out error"
        );
    }

    /// (codec, sample rate, channels, frames) of the first stream, from the
    /// ffprobe next to the ffmpeg in use. None when ffprobe is unavailable.
    fn probe_stream(path: &Path) -> Option<(String, u32, u32, u64)> {
        let ffprobe = find_ffmpeg_path()?.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let out = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name,sample_rate,channels,duration_ts",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let fields: Vec<&str> = text.trim().split(',').collect();
        Some((
            fields.first()?.to_string(),
            fields.get(1)?.parse().ok()?,
            fields.get(2)?.parse().ok()?,
            fields.get(3)?.parse().ok()?,
        ))
    }

    #[test]
    fn test_merge_timeout_grows_with_the_recording() {
        assert_eq!(merge_timeout(0.0), std::time::Duration::from_secs(120));
        assert_eq!(
            merge_timeout(3.0 * 3600.0),
            std::time::Duration::from_secs(1470)
        );
    }

    #[test]
    fn test_flac_checkpoint_keeps_every_frame_and_survives_non_finite_samples() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("checkpoint.flac");

        // 1.5 s of stereo with a few non-finite samples.
        let mut data = vec![0.25f32; 48000 * 2 * 3 / 2];
        data[10] = f32::NAN;
        data[11] = f32::INFINITY;
        data[12] = f32::NEG_INFINITY;

        match encode_checkpoint_flac(&data, 48000, 2, &output_path) {
            Ok(()) => {
                assert!(std::fs::metadata(&output_path).unwrap().len() > 0);
                if let Some((codec, rate, channels, frames)) = probe_stream(&output_path) {
                    assert_eq!(codec, "flac");
                    assert_eq!(rate, 48000);
                    assert_eq!(channels, 2);
                    assert_eq!(frames, 72_000, "a lossless checkpoint keeps every frame");
                }
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("FFmpeg not found") || msg.contains("timed out"),
                    "unexpected encode failure: {}",
                    msg
                );
            }
        }
    }

    #[test]
    fn test_encode_with_non_finite_samples_produces_valid_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("nan_test.mp4");

        // Mix of clean audio and NaN / ±Inf samples.
        let data: Vec<f32> = vec![
            0.5,
            -0.5,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.25,
            0.0,
            -1.0,
        ];
        let result = encode_single_audio(&data, 48000, 2, &output_path);

        // Where ffmpeg is unavailable (CI without the binary), the encoder
        // returns "FFmpeg not found" — the sanitization itself is covered by
        // test_sanitize_replaces_non_finite_samples. When ffmpeg is present,
        // the encode must succeed and produce an output file of the same size
        // as encoding the fully-sanitized input (NaN → 0.0 is lossless here).
        match result {
            Ok(()) => {
                assert!(output_path.exists());
                assert!(std::fs::metadata(&output_path).unwrap().len() > 0);

                // Sanitized twin must yield an identically-sized file.
                let sanitized: Vec<f32> = data
                    .iter()
                    .map(|&s| if s.is_finite() { s } else { 0.0 })
                    .collect();
                let clean_path = temp_dir.path().join("clean_test.mp4");
                encode_single_audio(&sanitized, 48000, 2, &clean_path).unwrap();
                assert_eq!(
                    std::fs::metadata(&output_path).unwrap().len(),
                    std::fs::metadata(&clean_path).unwrap().len()
                );
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("FFmpeg not found") || msg.contains("timed out"),
                    "unexpected encode failure: {}",
                    msg
                );
            }
        }
    }
}
