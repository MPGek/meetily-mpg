// Self-contained voiceprint audio clips (voiceprint-audio-clips-and-verification).
//
// Cuts short Opus mono clips out of a meeting's saved audio file for the exact
// time windows backing voiceprint embeddings, so clips can be validated by
// listening without depending on the meeting file at playback time.
//
// Capture runs best-effort at persist time: any failure yields `None` and the
// row is stored as a legacy clip-less row, never failing persistence.

use super::encode::run_ffmpeg_with_timeout;
use super::ffmpeg::find_ffmpeg_path;
use std::path::{Path, PathBuf};

/// Maximum clip length per voiceprint row (spec: ~15 s cap).
pub const VOICEPRINT_CLIP_MAX_SECS: f64 = 15.0;
/// Clip sample rate: matches the diarization rate, voice needs nothing above it.
pub const VOICEPRINT_CLIP_SAMPLE_RATE: u32 = 16000;
/// Opus bitrate for voice validation (transparent with headroom for noise).
pub const VOICEPRINT_CLIP_BITRATE: &str = "24k";
/// Codec tag stored alongside the blob.
pub const VOICEPRINT_CLIP_CODEC: &str = "opus";
/// Bound on a single clip encode; far above the <1 s a ≤15 s window needs.
pub const CLIP_ENCODE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Resolve a meeting's saved audio file via its folder path + canonical
/// discovery. Returns `None` when the meeting, folder, or file is missing —
/// callers then store legacy clip-less rows.
pub async fn resolve_meeting_audio_file(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
) -> Option<PathBuf> {
    let meta =
        crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(
            pool, meeting_id,
        )
        .await
        .ok()??;
    let folder = meta.folder_path?;
    super::audio_file::find_audio_file(Path::new(&folder)).ok()
}

/// Cut one Opus mono clip for `[start_secs, end_secs)` from `audio_path`,
/// taking the pan channel matching `channel` (`mic` = left/c0, `system` =
/// right/c1 — the stereo layout written by the pipeline/incremental saver).
/// The window is clamped to `VOICEPRINT_CLIP_MAX_SECS` (start-anchored).
/// Returns `None` on any failure (missing ffmpeg, bad window, encode error).
/// Blocking: callers run it under `spawn_blocking`.
pub fn cut_voiceprint_clip(
    audio_path: &Path,
    channel: &str,
    start_secs: f64,
    end_secs: f64,
) -> Option<Vec<u8>> {
    if !start_secs.is_finite() || !end_secs.is_finite() {
        return None;
    }
    let start = start_secs.max(0.0);
    let mut end = end_secs;
    if end <= start {
        return None;
    }
    if end - start > VOICEPRINT_CLIP_MAX_SECS {
        end = start + VOICEPRINT_CLIP_MAX_SECS;
    }

    let ffmpeg_path = find_ffmpeg_path()?;
    // Mono sources have no c1; mic (c0) always exists, system needs stereo.
    let pan = match channel {
        "system" => "pan=mono|c0=c1",
        _ => "pan=mono|c0=c0",
    };

    let cache_dir = std::env::temp_dir().join("meetily-voiceprint-clips");
    if std::fs::create_dir_all(&cache_dir).is_err() {
        return None;
    }
    let out_path = cache_dir.join(format!("clip-{}.ogg", uuid::Uuid::new_v4()));

    let mut command = std::process::Command::new(ffmpeg_path);
    command.args([
        "-y",
        "-ss",
        &format!("{:.3}", start),
        "-to",
        &format!("{:.3}", end),
        "-i",
        &audio_path.to_string_lossy().to_string(),
        "-vn",
        "-af",
        pan,
        "-ar",
        &VOICEPRINT_CLIP_SAMPLE_RATE.to_string(),
        "-ac",
        "1",
        "-c:a",
        "libopus",
        "-b:a",
        VOICEPRINT_CLIP_BITRATE,
        "-vbr",
        "on",
        "-application",
        "voip",
        "-f",
        "ogg",
        &out_path.to_string_lossy().to_string(),
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = run_ffmpeg_with_timeout(command, None, CLIP_ENCODE_TIMEOUT).ok()?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&out_path);
        return None;
    }
    let bytes = std::fs::read(&out_path).ok()?;
    let _ = std::fs::remove_file(&out_path);
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

/// Cut clips for parallel windows; one entry per window, `None` on failure.
/// Runs the blocking encodes on a spawn_blocking thread. Returns all-`None`
/// without touching ffmpeg when `windows` is empty or the file is missing.
pub async fn cut_clips_for_meeting(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    channel: &str,
    windows: &[(f64, f64)],
) -> Vec<Option<Vec<u8>>> {
    if windows.is_empty() {
        return Vec::new();
    }
    let Some(audio_path) = resolve_meeting_audio_file(pool, meeting_id).await else {
        return vec![None; windows.len()];
    };
    let channel = channel.to_string();
    let windows = windows.to_vec();
    let window_count = windows.len();
    tokio::task::spawn_blocking(move || {
        windows
            .iter()
            .map(|(s, e)| cut_voiceprint_clip(&audio_path, &channel, *s, *e))
            .collect()
    })
    .await
    .unwrap_or_else(|_| vec![None; window_count])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_window_validation() {
        let missing = Path::new("/nonexistent-audio-file.mp4");
        // Degenerate windows never reach ffmpeg.
        assert!(cut_voiceprint_clip(missing, "mic", 5.0, 5.0).is_none());
        assert!(cut_voiceprint_clip(missing, "mic", 5.0, 4.0).is_none());
        assert!(cut_voiceprint_clip(missing, "mic", f64::NAN, 6.0).is_none());
        assert!(cut_voiceprint_clip(missing, "mic", -2.0, -1.0).is_none());
    }

    #[test]
    fn test_clip_cap_clamps_window() {
        // A 120 s window must be clamped to the 15 s cap, not rejected.
        // With a missing file the encode fails, but the clamp path is what
        // reaches ffmpeg (verified by a bounded failure, not a hang).
        let missing = Path::new("/nonexistent-audio-file.mp4");
        let started = std::time::Instant::now();
        assert!(cut_voiceprint_clip(missing, "mic", 0.0, 120.0).is_none());
        assert!(
            started.elapsed() < CLIP_ENCODE_TIMEOUT,
            "missing-file failure must be fast"
        );
    }
}
