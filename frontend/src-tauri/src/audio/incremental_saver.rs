use super::encode::{
    encode_checkpoint_flac, merge_timeout, run_ffmpeg_with_timeout, AAC_OUTPUT_ARGS,
    ENCODE_TIMEOUT, MERGE_TIMEOUT,
};
use super::recording_state::AudioChunk;
use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::ffmpeg::find_ffmpeg_path;

/// Audio data without device type (we only store mixed audio)
#[derive(Clone)]
struct AudioData {
    data: Vec<f32>,
    // sample_rate: u32,
}

/// Incremental audio saver that writes checkpoints every 30 seconds
/// to minimize memory usage and enable crash recovery
pub struct IncrementalAudioSaver {
    checkpoint_buffer: Vec<AudioData>,
    checkpoint_interval_samples: usize, // 30s at 48kHz stereo = 2,880,000 interleaved samples
    checkpoint_count: u32,
    /// Number of checkpoints whose encode failed and whose buffer segment was
    /// discarded (drop-and-continue). Exposed so the stop flow can surface a
    /// partial-audio warning instead of reporting a silent full-success save.
    failed_checkpoints: u32,
    /// Frames per channel written to successful checkpoints.
    frames_written: u64,
    checkpoints_dir: PathBuf,
    meeting_folder: PathBuf,
    sample_rate: u32,
}

impl IncrementalAudioSaver {
    /// Create a new incremental saver
    ///
    /// # Arguments
    /// * `meeting_folder` - Path to the meeting folder (contains .checkpoints/)
    /// * `sample_rate` - Sample rate of audio (typically 48000)
    pub fn new(meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        let checkpoints_dir = meeting_folder.join(".checkpoints");

        // Verify checkpoints directory exists
        if !checkpoints_dir.exists() {
            return Err(anyhow!(
                "Checkpoints directory does not exist: {}",
                checkpoints_dir.display()
            ));
        }

        Ok(Self {
            checkpoint_buffer: Vec::new(),
            // The buffer holds stereo-interleaved samples, so the sample
            // threshold is sample_rate * 30s * 2 channels. Counting interleaved
            // samples as mono frames previously halved real checkpoints to ~15s.
            checkpoint_interval_samples: sample_rate as usize * 30 * 2, // 30 seconds (stereo)
            checkpoint_count: 0,
            failed_checkpoints: 0,
            frames_written: 0,
            checkpoints_dir,
            meeting_folder,
            sample_rate,
        })
    }

    /// Add an audio chunk to the buffer
    /// Automatically saves a checkpoint when buffer reaches 30 seconds
    pub fn add_chunk(&mut self, chunk: AudioChunk) -> Result<()> {
        let audio_data = AudioData {
            data: chunk.data,
            // sample_rate: chunk.sample_rate,
        };

        self.checkpoint_buffer.push(audio_data);

        // Calculate total samples in buffer
        let total_samples: usize = self.checkpoint_buffer.iter().map(|c| c.data.len()).sum();

        // Save checkpoint when buffer reaches threshold (30 seconds);
        // save_checkpoint clears the buffer on both success and failure.
        if total_samples >= self.checkpoint_interval_samples {
            self.save_checkpoint()?;
        }

        Ok(())
    }

    /// Save current buffer as a checkpoint file
    ///
    /// On encode failure the affected buffered segment is discarded and
    /// checkpointing continues at the next boundary (drop-and-continue), so a
    /// single bad checkpoint can never wedge the accumulation task and silently
    /// lose the rest of the recording.
    fn save_checkpoint(&mut self) -> Result<()> {
        // Concatenate all chunks in buffer
        let audio_data: Vec<f32> = self
            .checkpoint_buffer
            .iter()
            .flat_map(|c| &c.data)
            .cloned()
            .collect();

        if audio_data.is_empty() {
            warn!("Attempted to save empty checkpoint, skipping");
            return Ok(());
        }

        // Generate checkpoint filename
        let checkpoint_path = self
            .checkpoints_dir
            .join(format!("audio_chunk_{:03}.flac", self.checkpoint_count));

        // Encode and save checkpoint
        if let Err(e) = encode_checkpoint_flac(
            &audio_data,
            self.sample_rate,
            2, // stereo
            &checkpoint_path,
        ) {
            error!(
                "Checkpoint encode failed ({}): {}",
                checkpoint_path.display(),
                e
            );
            self.failed_checkpoints += 1;
            self.checkpoint_buffer.clear();
            return Ok(());
        }

        let frames = audio_data.len() / 2; // interleaved stereo -> frames per channel
        let duration_seconds = frames as f32 / self.sample_rate as f32;
        self.checkpoint_count += 1;
        self.frames_written += frames as u64;
        self.checkpoint_buffer.clear();

        info!(
            "Saved checkpoint {}: {:.2}s of audio ({} samples)",
            self.checkpoint_count,
            duration_seconds,
            audio_data.len()
        );

        Ok(())
    }

    /// Finalize the recording: save final checkpoint, merge all checkpoints, cleanup
    ///
    /// Returns the path to the final merged audio.mp4 file
    pub async fn finalize(&mut self) -> Result<PathBuf> {
        info!("Finalizing incremental recording...");

        // Save final buffer if not empty
        if !self.checkpoint_buffer.is_empty() {
            info!(
                "Saving final checkpoint with remaining {} chunks",
                self.checkpoint_buffer.len()
            );
            self.save_checkpoint()?;
        }

        if self.checkpoint_count == 0 {
            return Err(anyhow!(
                "No audio checkpoints to merge - recording may have failed"
            ));
        }

        // Merge all checkpoints using FFmpeg concat
        let final_audio_path = self.meeting_folder.join("audio.mp4");
        self.merge_checkpoints(&final_audio_path).await?;

        // Clean up checkpoints directory
        info!("Cleaning up {} checkpoint files", self.checkpoint_count);
        if let Err(e) = std::fs::remove_dir_all(&self.checkpoints_dir) {
            warn!("Failed to clean up checkpoints directory: {}", e);
            // Non-fatal - user can manually delete
        }

        info!("Finalized recording: {}", final_audio_path.display());

        Ok(final_audio_path)
    }

    /// Merge all checkpoint files into final audio.mp4 using FFmpeg concat
    /// Uses the concat demuxer over the FLAC checkpoints and encodes AAC once
    async fn merge_checkpoints(&self, output: &Path) -> Result<()> {
        info!(
            "Merging {} checkpoints into final audio file...",
            self.checkpoint_count
        );

        // Create concat list file for FFmpeg
        let list_file = self.checkpoints_dir.join("concat_list.txt");
        let mut list_content = String::new();

        for i in 0..self.checkpoint_count {
            let checkpoint_path = self
                .checkpoints_dir
                .join(format!("audio_chunk_{:03}.flac", i));

            // Verify checkpoint exists
            if !checkpoint_path.exists() {
                return Err(anyhow!(
                    "Checkpoint file missing: {}",
                    checkpoint_path.display()
                ));
            }

            // Use absolute path for FFmpeg (required for safe mode)
            let abs_path = checkpoint_path.canonicalize()?;
            list_content.push_str(&format!("file '{}'\n", abs_path.display()));
        }

        std::fs::write(&list_file, list_content)?;

        let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| {
            anyhow!("FFmpeg not found. Please install FFmpeg to finalize recordings.")
        })?;
        info!("Using FFmpeg at: {:?}", ffmpeg_path);

        // Run FFmpeg concat command
        // One AAC encode of the lossless checkpoints keeps every sample; a
        // stream copy of per-checkpoint AAC files adds each one's encoder
        // delay and padding (~37 ms per checkpoint).

        let mut command = std::process::Command::new(ffmpeg_path);

        command
            .args([
                "-f",
                "concat", // Use concat demuxer
                "-safe",
                "0", // Allow absolute paths
                "-i",
                list_file.to_str().unwrap(),
            ])
            .args(AAC_OUTPUT_ARGS)
            .args([
                "-y", // Overwrite output file
                output.to_str().unwrap(),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Hide console window on Windows to prevent CMD popup during finalization
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        // Bound the merge (it encodes the whole recording) so a hung FFmpeg
        // cannot hang finalize forever
        let timeout = merge_timeout(self.audio_secs());
        let ffmpeg_output =
            tokio::task::spawn_blocking(move || run_ffmpeg_with_timeout(command, None, timeout))
                .await
                .map_err(|e| anyhow!("FFmpeg merge task panicked: {:?}", e))?;

        let ffmpeg_output = match ffmpeg_output {
            Ok(output) => output,
            Err(e) => {
                error!("FFmpeg merge failed: {}", e);
                return Err(anyhow!("FFmpeg concat failed: {}", e));
            }
        };

        if !ffmpeg_output.status.success() {
            let stderr = String::from_utf8_lossy(&ffmpeg_output.stderr);
            error!("FFmpeg merge failed: {}", stderr);
            return Err(anyhow!("FFmpeg concat failed: {}", stderr));
        }

        // Verify output file was created
        if !output.exists() {
            return Err(anyhow!(
                "Merged audio file was not created: {}",
                output.display()
            ));
        }

        info!(
            "Successfully merged {} checkpoints → {}",
            self.checkpoint_count,
            output.display()
        );

        Ok(())
    }

    /// Get the meeting folder path
    pub fn get_meeting_folder(&self) -> &PathBuf {
        &self.meeting_folder
    }

    /// Get current checkpoint count
    pub fn get_checkpoint_count(&self) -> u32 {
        self.checkpoint_count
    }

    /// Seconds of audio this saver holds: written checkpoints plus the
    /// buffered remainder.
    pub fn audio_secs(&self) -> f64 {
        let buffered: usize = self.checkpoint_buffer.iter().map(|c| c.data.len()).sum();
        (self.frames_written + buffered as u64 / 2) as f64 / self.sample_rate as f64
    }

    /// Upper bound on `finalize`: the last checkpoint encode plus the merge.
    pub fn finalize_budget(&self) -> std::time::Duration {
        ENCODE_TIMEOUT + merge_timeout(self.audio_secs())
    }

    /// Get the number of checkpoint encodes that failed and were discarded
    pub fn get_failed_checkpoints(&self) -> u32 {
        self.failed_checkpoints
    }

    /// Get checkpoint statistics for the consuming (stop) flow.
    /// Returns (checkpoint_count, sample_rate, failed_checkpoints).
    pub fn get_stats(&self) -> (u32, u32, u32) {
        (
            self.checkpoint_count,
            self.sample_rate,
            self.failed_checkpoints,
        )
    }
}

/// Audio recovery status for transcript recovery feature
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRecoveryStatus {
    pub status: String, // "success" | "partial" | "failed" | "none"
    pub chunk_count: u32,
    pub estimated_duration_seconds: f64,
    pub audio_file_path: Option<String>,
    pub message: String,
}

/// Recover audio from checkpoint files
/// This is called by the transcript recovery system to merge audio chunks after a crash
#[tauri::command]
pub async fn recover_audio_from_checkpoints(
    meeting_folder: String,
    _sample_rate: u32,
) -> Result<AudioRecoveryStatus, String> {
    info!("Starting audio recovery for folder: {}", meeting_folder);

    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(".checkpoints");

    // Check if checkpoints directory exists
    if !checkpoints_dir.exists() {
        info!(
            "No checkpoints directory found at: {}",
            checkpoints_dir.display()
        );
        return Ok(AudioRecoveryStatus {
            status: "none".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: "No audio checkpoints found".to_string(),
        });
    }

    // Scan for checkpoint files
    let checkpoint_files = list_checkpoints(&checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?;

    if checkpoint_files.is_empty() {
        info!(
            "No checkpoint files found in: {}",
            checkpoints_dir.display()
        );
        return Ok(AudioRecoveryStatus {
            status: "none".to_string(),
            chunk_count: 0,
            estimated_duration_seconds: 0.0,
            audio_file_path: None,
            message: "No audio checkpoint files found".to_string(),
        });
    }

    // Lossless checkpoints are encoded once; AAC checkpoints left by older
    // versions are stream-copied as before.
    let lossless = checkpoint_files
        .iter()
        .any(|path| path.extension().and_then(|s| s.to_str()) == Some("flac"));

    let chunk_count = checkpoint_files.len() as u32;
    let estimated_duration = (chunk_count as f64) * 30.0; // 30 seconds per chunk

    info!(
        "Found {} checkpoint files, estimated duration: {:.2}s",
        chunk_count, estimated_duration
    );

    // Create FFmpeg concat file
    let concat_file_path = checkpoints_dir.join("concat_list.txt");
    let mut concat_content = String::new();

    for entry in &checkpoint_files {
        let path = entry
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize path: {}", e))?;
        concat_content.push_str(&format!("file '{}'\n", path.display()));
    }

    std::fs::write(&concat_file_path, concat_content)
        .map_err(|e| format!("Failed to write concat file: {}", e))?;

    // Run FFmpeg to merge chunks
    let output_path = folder_path.join("audio.mp4");
    let output_path_str = output_path
        .to_str()
        .ok_or("Invalid output path")?
        .to_string();

    let ffmpeg_path = find_ffmpeg_path()
        .ok_or_else(|| "FFmpeg not found. Please install FFmpeg to recover audio.".to_string())?;
    info!("Using FFmpeg at: {:?}", ffmpeg_path);

    let mut command = std::process::Command::new(ffmpeg_path);

    command
        .args([
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            concat_file_path.to_str().unwrap(),
        ])
        .args(if lossless {
            &AAC_OUTPUT_ARGS[..]
        } else {
            &["-c", "copy"][..]
        })
        .args([
            "-y", // Overwrite if exists
            &output_path_str,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Hide console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    // Bound the recovery merge so a hung FFmpeg cannot hang recovery
    let timeout = if lossless {
        merge_timeout(estimated_duration)
    } else {
        MERGE_TIMEOUT
    };
    let ffmpeg_result =
        tokio::task::spawn_blocking(move || run_ffmpeg_with_timeout(command, None, timeout))
            .await
            .map_err(|e| format!("FFmpeg recovery task panicked: {:?}", e));

    match ffmpeg_result {
        Ok(Ok(output)) if output.status.success() => {
            // Clean up concat file
            let _ = std::fs::remove_file(concat_file_path);

            info!("Successfully recovered audio: {}", output_path_str);

            Ok(AudioRecoveryStatus {
                status: "success".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: Some(output_path_str),
                message: format!("Successfully recovered {} audio chunks", chunk_count),
            })
        }
        Ok(Ok(output)) => {
            let error = String::from_utf8_lossy(&output.stderr);
            error!("FFmpeg recovery failed: {}", error);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("FFmpeg failed: {}", error),
            })
        }
        Ok(Err(e)) => {
            error!("Failed to run FFmpeg: {}", e);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("Failed to run FFmpeg: {}", e),
            })
        }
        Err(e) => {
            error!("FFmpeg recovery task failed: {}", e);
            Ok(AudioRecoveryStatus {
                status: "failed".to_string(),
                chunk_count,
                estimated_duration_seconds: estimated_duration,
                audio_file_path: None,
                message: format!("FFmpeg recovery failed: {}", e),
            })
        }
    }
}

/// Clean up checkpoint files after successful recording or recovery
/// This command is called by the frontend after successful save to clean up checkpoint files
#[tauri::command]
pub async fn cleanup_checkpoints(meeting_folder: String) -> Result<(), String> {
    info!("Cleaning up checkpoints for folder: {}", meeting_folder);

    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(".checkpoints");

    if checkpoints_dir.exists() {
        std::fs::remove_dir_all(&checkpoints_dir)
            .map_err(|e| format!("Failed to remove checkpoints directory: {}", e))?;
        info!("Successfully cleaned up checkpoints directory");
    } else {
        info!("No checkpoints directory to clean up");
    }

    Ok(())
}

/// Check if a meeting folder has audio checkpoint files
/// Returns true if .checkpoints/ directory exists and contains checkpoint files
#[tauri::command]
pub async fn has_audio_checkpoints(meeting_folder: String) -> Result<bool, String> {
    let folder_path = PathBuf::from(&meeting_folder);
    let checkpoints_dir = folder_path.join(".checkpoints");

    // Check if checkpoints directory exists
    if !checkpoints_dir.exists() {
        return Ok(false);
    }

    let checkpoints = list_checkpoints(&checkpoints_dir)
        .map_err(|e| format!("Failed to read checkpoints directory: {}", e))?;

    Ok(!checkpoints.is_empty())
}

/// The `audio_chunk_<n>` checkpoints in `dir`, ordered by `n`: the FLAC ones
/// when there are any, otherwise AAC `.mp4` ones written by older versions.
fn list_checkpoints(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let paths = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    Ok(select_checkpoints(paths))
}

fn select_checkpoints(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let index = |path: &PathBuf, ext: &str| -> Option<u64> {
        if path.extension()?.to_str()? != ext {
            return None;
        }
        path.file_stem()?
            .to_str()?
            .strip_prefix("audio_chunk_")?
            .parse()
            .ok()
    };
    for ext in ["flac", "mp4"] {
        let mut found: Vec<(u64, PathBuf)> = paths
            .iter()
            .filter_map(|path| Some((index(path, ext)?, path.clone())))
            .collect();
        if !found.is_empty() {
            found.sort_by_key(|(n, _)| *n);
            return found.into_iter().map(|(_, path)| path).collect();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::super::recording_state::DeviceType;
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_checkpoint_creation() {
        // Create temp meeting folder
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Test_Meeting");
        std::fs::create_dir_all(&meeting_folder).unwrap();
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();

        // 60 seconds of stereo-interleaved audio (each 24000-sample chunk is
        // 0.25s of stereo audio) at a 30s checkpoint cadence produces exactly 2
        // checkpoints - not 4 as when interleaved samples were counted as mono.
        for i in 0..240 {
            // 240 chunks of 0.25s stereo each = 60s
            let chunk = AudioChunk {
                data: vec![0.5f32; 24000], // 0.25s stereo at 48kHz
                sample_rate: 48000,
                timestamp: i as f64 * 0.25, // timestamp in seconds
                chunk_id: i as u64,
                device_type: DeviceType::Microphone,
                channels: 2,
            };
            saver.add_chunk(chunk).unwrap();
        }

        // Verify 2 checkpoints created (30s each)
        assert_eq!(saver.checkpoint_count, 2);

        // Finalize and verify merge
        let final_path = saver.finalize().await.unwrap();
        assert!(final_path.exists());

        // Verify checkpoints directory deleted
        assert!(!meeting_folder.join(".checkpoints").exists());
    }

    #[tokio::test]
    async fn test_checkpoint_failure_drop_and_continue() {
        // Create temp meeting folder
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Failure_Test");
        std::fs::create_dir_all(&meeting_folder).unwrap();
        let checkpoints_dir = meeting_folder.join(".checkpoints");
        std::fs::create_dir_all(&checkpoints_dir).unwrap();

        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();

        // Remove the checkpoints directory so every checkpoint encode fails
        // (its output path's parent no longer exists).
        std::fs::remove_dir_all(&checkpoints_dir).unwrap();

        // Feed more than one checkpoint window worth of audio (~75s stereo).
        for i in 0..300 {
            let chunk = AudioChunk {
                data: vec![0.5f32; 24000], // 0.25s stereo at 48kHz
                sample_rate: 48000,
                timestamp: i as f64 * 0.25,
                chunk_id: i as u64,
                device_type: DeviceType::Microphone,
                channels: 2,
            };
            // Accumulation must keep accepting audio even while encodes fail.
            saver.add_chunk(chunk).unwrap();
        }

        // The failure was counted but did not wedge or stop accumulation.
        assert!(
            saver.failed_checkpoints >= 1,
            "expected at least one failed checkpoint"
        );
        assert_eq!(
            saver.checkpoint_count, 0,
            "no checkpoint should have succeeded while the dir is gone"
        );

        // Restore the checkpoints directory: subsequent audio must still
        // produce checkpoints (checkpointing continues after a failure).
        std::fs::create_dir_all(&checkpoints_dir).unwrap();

        for i in 300..430 {
            // another ~32.5s of stereo audio
            let chunk = AudioChunk {
                data: vec![0.5f32; 24000],
                sample_rate: 48000,
                timestamp: i as f64 * 0.25,
                chunk_id: i as u64,
                device_type: DeviceType::Microphone,
                channels: 2,
            };
            saver.add_chunk(chunk).unwrap();
        }

        // Later audio still produced checkpoints and the failure count is reported.
        assert!(
            saver.checkpoint_count >= 1,
            "checkpointing should continue after a failure"
        );
        assert!(saver.get_failed_checkpoints() >= 1);
    }

    /// Decode `path` to interleaved stereo f32 with the ffmpeg in use.
    /// None when ffmpeg is unavailable.
    fn decode_stereo(path: &Path) -> Option<Vec<f32>> {
        let out = std::process::Command::new(find_ffmpeg_path()?)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "f32le", "-ac", "2", "-ar", "48000", "-"])
            .output()
            .ok()?;
        out.status.success().then(|| {
            out.stdout
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        })
    }

    /// First frame of each click (left channel above 0.15), at most one per
    /// half second.
    fn click_frames(stereo: &[f32]) -> Vec<usize> {
        let mut clicks = Vec::new();
        let mut frame = 0;
        while frame * 2 < stereo.len() {
            if stereo[frame * 2].abs() > 0.15 {
                clicks.push(frame);
                frame += 24_000;
            } else {
                frame += 1;
            }
        }
        clicks
    }

    /// 0.25 s stereo chunks of `secs` seconds: quiet noise with a 1 ms click
    /// at the start of every second on both channels.
    fn clicked_chunks(secs: usize) -> Vec<AudioChunk> {
        let mut state = 7u32;
        (0..secs * 4)
            .map(|i| {
                let mut data = Vec::with_capacity(24_000);
                for f in 0..12_000 {
                    let frame = i * 12_000 + f;
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (state >> 8) as f32 / (1u32 << 24) as f32 * 0.02 - 0.01;
                    let v = if frame % 48_000 < 48 { 0.3 } else { noise };
                    data.push(v);
                    data.push(v);
                }
                AudioChunk {
                    data,
                    sample_rate: 48000,
                    timestamp: (i + 1) as f64 * 0.25,
                    chunk_id: i as u64,
                    device_type: DeviceType::Microphone,
                    channels: 2,
                }
            })
            .collect()
    }

    /// Clicks fed at every second must play within one AAC frame of that
    /// second, and the file must be as long as what was fed.
    fn assert_clicks_on_time(path: &Path, fed_secs: usize) {
        let Some(stereo) = decode_stereo(path) else {
            eprintln!("ffmpeg unavailable: skipping click timing check");
            return;
        };
        let frames = stereo.len() / 2;
        assert!(
            frames.abs_diff(fed_secs * 48_000) <= 1024,
            "decoded {frames} frames for {} fed",
            fed_secs * 48_000
        );
        let clicks = click_frames(&stereo);
        assert_eq!(clicks.len(), fed_secs);
        for (k, &at) in clicks.iter().enumerate() {
            assert!(
                at.abs_diff(k * 48_000) <= 1024,
                "click {k} at frame {at}, fed at {}",
                k * 48_000
            );
        }
    }

    #[tokio::test]
    async fn test_merged_checkpoints_keep_the_recorded_timing() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Timing_Test");
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();

        // 75 s: two full checkpoints and a 15 s remainder.
        for chunk in clicked_chunks(75) {
            saver.add_chunk(chunk).unwrap();
        }
        let final_path = match saver.finalize().await {
            Ok(path) => path,
            Err(e) if e.to_string().contains("FFmpeg not found") => return,
            Err(e) => panic!("finalize failed: {e}"),
        };

        assert_clicks_on_time(&final_path, 75);

        let ffprobe = find_ffmpeg_path().map(|p| {
            p.with_file_name(if cfg!(windows) {
                "ffprobe.exe"
            } else {
                "ffprobe"
            })
        });
        if let Some(Ok(out)) = ffprobe.map(|p| {
            std::process::Command::new(p)
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_name,profile,sample_rate,channels",
                    "-of",
                    "csv=p=0",
                ])
                .arg(&final_path)
                .output()
        }) {
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim(),
                "aac,LC,48000,2"
            );
        }
    }

    #[tokio::test]
    async fn test_recovery_of_flac_checkpoints_keeps_the_recorded_timing() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Crashed_Meeting");
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();

        // Two full checkpoints, then the app "crashes" (no finalize).
        for chunk in clicked_chunks(60) {
            saver.add_chunk(chunk).unwrap();
        }
        if saver.checkpoint_count == 0 {
            return; // ffmpeg unavailable
        }
        drop(saver);

        let folder = meeting_folder.to_string_lossy().to_string();
        assert!(has_audio_checkpoints(folder.clone()).await.unwrap());
        let status = recover_audio_from_checkpoints(folder, 48000).await.unwrap();
        assert_eq!(status.status, "success", "{}", status.message);
        assert_eq!(status.chunk_count, 2);

        assert_clicks_on_time(&meeting_folder.join("audio.mp4"), 60);
    }

    #[tokio::test]
    async fn test_recovery_of_legacy_aac_checkpoints() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Legacy_Meeting");
        let checkpoints_dir = meeting_folder.join(".checkpoints");
        std::fs::create_dir_all(&checkpoints_dir).unwrap();

        // Checkpoints as earlier versions wrote them: AAC in MP4.
        let audio = vec![0.25f32; 48_000 * 2];
        for i in 0..2 {
            let path = checkpoints_dir.join(format!("audio_chunk_{:03}.mp4", i));
            if super::super::encode::encode_single_audio(&audio, 48000, 2, &path).is_err() {
                return; // ffmpeg unavailable
            }
        }

        let folder = meeting_folder.to_string_lossy().to_string();
        assert!(has_audio_checkpoints(folder.clone()).await.unwrap());
        let status = recover_audio_from_checkpoints(folder, 48000).await.unwrap();
        assert_eq!(status.status, "success", "{}", status.message);
        assert_eq!(status.chunk_count, 2);
        assert!(meeting_folder.join("audio.mp4").exists());
    }

    #[test]
    fn test_checkpoints_are_ordered_by_index_and_flac_wins() {
        let names = [
            "audio_chunk_1000.flac",
            "audio_chunk_101.flac",
            "audio_chunk_099.flac",
            "audio_chunk_000.mp4",
            "concat_list.txt",
            "other_1.flac",
        ];
        let selected = select_checkpoints(names.iter().map(PathBuf::from).collect());
        let selected: Vec<_> = selected
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            selected,
            [
                "audio_chunk_099.flac",
                "audio_chunk_101.flac",
                "audio_chunk_1000.flac"
            ]
        );

        let legacy = select_checkpoints(
            ["audio_chunk_002.mp4", "audio_chunk_010.mp4"]
                .iter()
                .map(PathBuf::from)
                .collect(),
        );
        assert_eq!(legacy.len(), 2);
        assert!(select_checkpoints(vec![PathBuf::from("concat_list.txt")]).is_empty());
    }

    #[tokio::test]
    async fn test_has_audio_checkpoints_accepts_either_format() {
        let temp_dir = tempdir().unwrap();
        let checkpoints_dir = temp_dir.path().join(".checkpoints");
        std::fs::create_dir_all(&checkpoints_dir).unwrap();
        let folder = temp_dir.path().to_string_lossy().to_string();

        assert!(!has_audio_checkpoints(folder.clone()).await.unwrap());
        std::fs::write(checkpoints_dir.join("audio_chunk_000.mp4"), b"x").unwrap();
        assert!(has_audio_checkpoints(folder.clone()).await.unwrap());
        std::fs::remove_file(checkpoints_dir.join("audio_chunk_000.mp4")).unwrap();
        std::fs::write(checkpoints_dir.join("audio_chunk_000.flac"), b"x").unwrap();
        assert!(has_audio_checkpoints(folder).await.unwrap());
    }

    #[tokio::test]
    async fn test_empty_recording() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Empty_Test");
        std::fs::create_dir_all(&meeting_folder).unwrap();
        std::fs::create_dir_all(meeting_folder.join(".checkpoints")).unwrap();

        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();

        // Try to finalize without adding any chunks
        let result = saver.finalize().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No audio checkpoints"));
    }
}
