use anyhow::Result;
use chrono::{Local, Utc};
use log::{debug, info};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::path::{Path, PathBuf};

use super::encode::encode_single_audio; // Correct path to encode module

/// Sanitize a filename to be safe for filesystem use
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Check whether a (sanitized) meeting name already ends with a `YYYY-MM-DD_HH-MM`
/// timestamp, so `create_meeting_folder` does not append a second one.
fn ends_with_timestamp(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 16 {
        return false;
    }
    let suffix = &bytes[bytes.len() - 16..];
    suffix[4] == b'-'
        && suffix[7] == b'-'
        && suffix[10] == b'_'
        && suffix[13] == b'-'
        && (0..16).all(|i| match i {
            4 | 7 | 10 | 13 => true,
            _ => suffix[i].is_ascii_digit(),
        })
}

/// Create a meeting folder with timestamp and return the path
/// Creates structure: base_path/MeetingName_YYYY-MM-DD_HH-MM/
///                    ├── .checkpoints/  (for incremental saves, optional)
///
/// # Arguments
/// * `base_path` - Base directory for meetings
/// * `meeting_name` - Name of the meeting
/// * `create_checkpoints_dir` - Whether to create .checkpoints/ subdirectory (only needed when auto_save is true)
pub fn create_meeting_folder(
    base_path: &Path,
    meeting_name: &str,
    create_checkpoints_dir: bool,
) -> Result<PathBuf> {
    let sanitized_name = sanitize_filename(meeting_name);
    let folder_name = if ends_with_timestamp(&sanitized_name) {
        sanitized_name
    } else {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M").to_string();
        format!("{}_{}", sanitized_name, timestamp)
    };
    let mut meeting_folder = base_path.join(&folder_name);

    // Ensure uniqueness when a folder with the same name already exists (e.g. two
    // recordings started in the same minute) by appending a numeric counter.
    if meeting_folder.exists() {
        let mut counter = 1;
        loop {
            let candidate = base_path.join(format!("{}_{}", folder_name, counter));
            if !candidate.exists() {
                meeting_folder = candidate;
                break;
            }
            counter += 1;
        }
    }

    // Create main meeting folder
    std::fs::create_dir_all(&meeting_folder)?;

    // Only create .checkpoints subdirectory if requested (when auto_save is true)
    if create_checkpoints_dir {
        let checkpoints_dir = meeting_folder.join(".checkpoints");
        std::fs::create_dir_all(&checkpoints_dir)?;
        log::info!(
            "Created meeting folder with checkpoints: {}",
            meeting_folder.display()
        );
    } else {
        log::info!(
            "Created meeting folder without checkpoints: {}",
            meeting_folder.display()
        );
    }

    Ok(meeting_folder)
}

/// High-pass filter to remove low-frequency rumble and noise
/// Removes frequencies below cutoff_hz (typically 80-100 Hz for speech)
pub struct HighPassFilter {
    // First-order IIR filter coefficients
    alpha: f32,
    prev_input: f32,
    prev_output: f32,
}

impl HighPassFilter {
    /// Create a new high-pass filter
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `cutoff_hz` - Cutoff frequency in Hz (typical: 80-100 Hz for speech)
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let sample_rate_f = sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
        let dt = 1.0 / sample_rate_f;
        let alpha = rc / (rc + dt);

        info!(
            "Initializing high-pass filter: cutoff={}Hz @ {}Hz",
            cutoff_hz, sample_rate
        );

        Self {
            alpha,
            prev_input: 0.0,
            prev_output: 0.0,
        }
    }

    /// Apply high-pass filter to audio samples
    /// Uses first-order IIR (Infinite Impulse Response) filter
    pub fn process(&mut self, samples: &[f32]) -> Vec<f32> {
        let mut output = Vec::with_capacity(samples.len());

        for &sample in samples {
            // First-order high-pass IIR filter formula:
            // y[n] = alpha * (y[n-1] + x[n] - x[n-1])
            let filtered = self.alpha * (self.prev_output + sample - self.prev_input);

            self.prev_input = sample;
            self.prev_output = filtered;

            output.push(filtered);
        }

        output
    }
}

pub fn audio_to_mono(audio: &[f32], channels: u16) -> Vec<f32> {
    let mut mono_samples = Vec::with_capacity(audio.len() / channels as usize);

    // For microphone arrays (> 2 channels), only use first 2 channels
    // Many microphone arrays have auxiliary channels for beam-forming/noise cancellation
    // that can contain anti-phase signals. Averaging all channels can cause destructive
    // interference resulting in near-zero output.
    let effective_channels = if channels > 2 { 2 } else { channels };

    // Iterate over the audio slice in chunks, each containing `channels` samples
    for chunk in audio.chunks(channels as usize) {
        // Sum only the first effective_channels (typically 1-2 for mic arrays)
        let sum: f32 = chunk.iter().take(effective_channels as usize).sum();

        // Calculate the average mono sample using effective channel count
        let mono_sample = sum / effective_channels as f32;

        // Store the computed mono sample
        mono_samples.push(mono_sample);
    }

    mono_samples
}

/// High-quality audio resampling with adaptive parameters based on sample rate ratio
///
/// This function automatically selects the best resampling parameters based on:
/// - Sample rate ratio (upsampling vs downsampling)
/// - Quality requirements (integer ratios get optimized paths)
/// - Anti-aliasing needs
///
/// Supports all common sample rates: 8kHz, 16kHz, 24kHz, 44.1kHz, 48kHz, etc.
pub fn resample(input: &[f32], from_sample_rate: u32, to_sample_rate: u32) -> Result<Vec<f32>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Fast path: No resampling needed
    if from_sample_rate == to_sample_rate {
        return Ok(input.to_vec());
    }

    let ratio = to_sample_rate as f64 / from_sample_rate as f64;

    // Adaptive parameters based on sample rate ratio
    let (sinc_len, interpolation_type, oversampling) = if ratio >= 2.0 {
        // Large upsampling (e.g., 8kHz → 16kHz, 16kHz → 48kHz, 24kHz → 48kHz)
        // Needs high quality to avoid artifacts
        debug!(
            "High-quality upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
            from_sample_rate, to_sample_rate, ratio
        );
        (
            512,                          // Longer sinc for smoother interpolation
            SincInterpolationType::Cubic, // Cubic for best quality
            512,                          // Higher oversampling
        )
    } else if ratio >= 1.5 {
        // Moderate upsampling (e.g., 32kHz → 48kHz)
        debug!(
            "Moderate upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
            from_sample_rate, to_sample_rate, ratio
        );
        (384, SincInterpolationType::Cubic, 384)
    } else if ratio > 1.0 {
        // Small upsampling (e.g., 44.1kHz → 48kHz)
        debug!(
            "Small upsampling: {}Hz → {}Hz (ratio: {:.2}x)",
            from_sample_rate, to_sample_rate, ratio
        );
        (256, SincInterpolationType::Linear, 256)
    } else if ratio <= 0.5 {
        // Large downsampling (e.g., 48kHz → 16kHz, 48kHz → 8kHz)
        // Needs strong anti-aliasing
        debug!(
            "Anti-aliased downsampling: {}Hz → {}Hz (ratio: {:.2}x)",
            from_sample_rate, to_sample_rate, ratio
        );
        (
            512,                          // Longer sinc for anti-aliasing
            SincInterpolationType::Cubic, // Cubic for quality
            512,
        )
    } else {
        // Moderate downsampling (e.g., 48kHz → 24kHz, 48kHz → 32kHz)
        debug!(
            "Moderate downsampling: {}Hz → {}Hz (ratio: {:.2}x)",
            from_sample_rate, to_sample_rate, ratio
        );
        (384, SincInterpolationType::Linear, 384)
    };

    let params = SincInterpolationParameters {
        sinc_len,
        f_cutoff: 0.95, // Preserve most of the frequency content
        interpolation: interpolation_type,
        oversampling_factor: oversampling,
        window: WindowFunction::BlackmanHarris2, // Best window for audio
    };

    let mut resampler = SincFixedIn::<f32>::new(
        ratio,
        2.0, // Maximum relative deviation
        params,
        input.len(),
        1, // Mono
    )?;

    let waves_in = vec![input.to_vec()];
    let waves_out = resampler.process(&waves_in, None)?;

    debug!(
        "Resampling complete: {} samples → {} samples",
        input.len(),
        waves_out[0].len()
    );

    Ok(waves_out.into_iter().next().unwrap())
}

// Alias for compatibility with existing code
pub fn resample_audio(input: &[f32], from_sample_rate: u32, to_sample_rate: u32) -> Vec<f32> {
    match resample(input, from_sample_rate, to_sample_rate) {
        Ok(result) => result,
        Err(e) => {
            debug!("Resampling failed: {}, returning original audio", e);
            input.to_vec()
        }
    }
}

/// Fast resampling optimized for transcription preprocessing
///
pub fn write_audio_to_file(
    audio: &[f32],
    sample_rate: u32,
    output_path: &Path,
    device: &str,
    skip_encoding: bool,
) -> Result<String> {
    write_audio_to_file_with_meeting_name(
        audio,
        sample_rate,
        output_path,
        device,
        skip_encoding,
        None,
    )
}

pub fn write_audio_to_file_with_meeting_name(
    audio: &[f32],
    sample_rate: u32,
    output_path: &Path,
    device: &str,
    skip_encoding: bool,
    meeting_name: Option<&str>,
) -> Result<String> {
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let sanitized_device_name = device.replace(['/', '\\'], "_");

    // Create meeting folder if meeting name is provided
    let final_output_path = if let Some(name) = meeting_name {
        let sanitized_meeting_name = sanitize_filename(name);
        let meeting_folder = output_path.join(&sanitized_meeting_name);

        // Create the meeting folder if it doesn't exist
        if !meeting_folder.exists() {
            std::fs::create_dir_all(&meeting_folder)?;
        }

        meeting_folder
    } else {
        output_path.to_path_buf()
    };

    let file_path = final_output_path
        .join(format!("{}_{}.mp4", sanitized_device_name, timestamp))
        .to_str()
        .expect("Failed to create valid path")
        .to_string();
    let file_path_clone = file_path.clone();
    // Run FFmpeg in a separate task
    if !skip_encoding {
        encode_single_audio(
            audio,
            sample_rate,
            2, // Stereo (left = microphone, right = system audio), matching the incremental saver
            Path::new(&file_path),
        )?;
    }
    Ok(file_path_clone)
}

/// Write transcript text to a file alongside the recording (legacy plain text format)
pub fn write_transcript_to_file(
    transcript_text: &str,
    output_path: &Path,
    meeting_name: Option<&str>,
) -> Result<String> {
    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    // Create meeting folder if meeting name is provided (same logic as audio)
    let final_output_path = if let Some(name) = meeting_name {
        let sanitized_meeting_name = sanitize_filename(name);
        let meeting_folder = output_path.join(&sanitized_meeting_name);

        // Create the meeting folder if it doesn't exist
        if !meeting_folder.exists() {
            std::fs::create_dir_all(&meeting_folder)?;
        }

        meeting_folder
    } else {
        output_path.to_path_buf()
    };

    let file_path = final_output_path.join(format!("transcript_{}.txt", timestamp));

    // Write transcript to file
    std::fs::write(&file_path, transcript_text)?;

    Ok(file_path.to_string_lossy().to_string())
}

/// Write structured transcript with timestamps to JSON file
pub fn write_transcript_json_to_file(
    segments: &[super::recording_saver::TranscriptSegment],
    output_path: &Path,
    meeting_name: Option<&str>,
    audio_filename: &str,
    recording_duration: f64,
) -> Result<String> {
    use serde_json::json;

    let timestamp = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();

    // Create meeting folder if meeting name is provided
    let final_output_path = if let Some(name) = meeting_name {
        let sanitized_meeting_name = sanitize_filename(name);
        let meeting_folder = output_path.join(&sanitized_meeting_name);

        if !meeting_folder.exists() {
            std::fs::create_dir_all(&meeting_folder)?;
        }

        meeting_folder
    } else {
        output_path.to_path_buf()
    };

    let file_path = final_output_path.join(format!("transcript_{}.json", timestamp));

    // Create structured JSON transcript
    let transcript_json = json!({
        "version": "1.0",
        "recording_duration": recording_duration,
        "audio_file": audio_filename,
        "sample_rate": 48000,
        "created_at": Utc::now().to_rfc3339(),
        "meeting_name": meeting_name,
        "segments": segments,
    });

    // Write JSON to file with pretty formatting
    let json_string = serde_json::to_string_pretty(&transcript_json)?;
    std::fs::write(&file_path, json_string)?;

    Ok(file_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::{create_meeting_folder, ends_with_timestamp, HighPassFilter};
    use std::path::PathBuf;

    /// Gain in dB of the 80 Hz mic high-pass filter for a sine at `freq_hz`,
    /// measured over 1 s after 0.5 s of settling at 48 kHz.
    fn hpf_gain_db(freq_hz: f32) -> f32 {
        let sr = 48_000usize;
        let input: Vec<f32> = (0..sr * 3 / 2)
            .map(|n| 0.5 * (2.0 * std::f32::consts::PI * freq_hz * n as f32 / sr as f32).sin())
            .collect();
        let output = HighPassFilter::new(sr as u32, 80.0).process(&input);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        20.0 * (rms(&output[sr / 2..]) / rms(&input[sr / 2..])).log10()
    }

    #[test]
    fn high_pass_filter_passes_speech_and_cuts_rumble() {
        let speech = hpf_gain_db(1_000.0);
        assert!(speech.abs() <= 0.5, "1 kHz must pass at unity, got {speech:.2} dB");
        let rumble = hpf_gain_db(20.0);
        assert!(rumble <= -10.0, "20 Hz must be cut by >= 10 dB, got {rumble:.2} dB");
    }

    #[test]
    fn ends_with_timestamp_matches_expected_format() {
        assert!(ends_with_timestamp("Meeting 2026-08-12_18-44"));
        assert!(ends_with_timestamp("Retro 2026-08-12_18-44"));
        assert!(ends_with_timestamp("2026-08-12_18-44"));
    }

    #[test]
    fn ends_with_timestamp_rejects_other_names() {
        assert!(!ends_with_timestamp("Team sync"));
        assert!(!ends_with_timestamp("Meeting 12_08_26_16_30_12"));
        assert!(!ends_with_timestamp("Meeting 2026-08-12"));
        assert!(!ends_with_timestamp("Meeting 2026-08-12_18-4"));
        assert!(!ends_with_timestamp("Meeting 2026-08-12_18-44x"));
    }

    #[test]
    fn create_meeting_folder_keeps_single_timestamp_name() {
        let base = temp_dir_path();
        let folder =
            create_meeting_folder(&base, "Meeting 2026-08-12_18-44", false).expect("create ok");
        assert_eq!(
            folder.file_name().unwrap().to_str().unwrap(),
            "Meeting 2026-08-12_18-44"
        );
    }

    #[test]
    fn create_meeting_folder_appends_suffix_to_plain_name() {
        let base = temp_dir_path();
        let folder = create_meeting_folder(&base, "Team sync", false).expect("create ok");
        let name = folder.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("Team sync_"), "unexpected name: {name}");
        assert!(
            ends_with_timestamp(name),
            "missing timestamp suffix: {name}"
        );
    }

    #[test]
    fn create_meeting_folder_uses_counter_on_collision() {
        let base = temp_dir_path();
        let first = create_meeting_folder(&base, "Meeting 2026-08-12_18-44", false).expect("ok");
        let second = create_meeting_folder(&base, "Meeting 2026-08-12_18-44", false).expect("ok");
        let third = create_meeting_folder(&base, "Meeting 2026-08-12_18-44", false).expect("ok");
        assert_eq!(
            first.file_name().unwrap().to_str().unwrap(),
            "Meeting 2026-08-12_18-44"
        );
        assert_eq!(
            second.file_name().unwrap().to_str().unwrap(),
            "Meeting 2026-08-12_18-44_1"
        );
        assert_eq!(
            third.file_name().unwrap().to_str().unwrap(),
            "Meeting 2026-08-12_18-44_2"
        );
    }

    fn temp_dir_path() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "meetily_folder_test_{}_{unique}",
            std::process::id()
        ))
    }
}
