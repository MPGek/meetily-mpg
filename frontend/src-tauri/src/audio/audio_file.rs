// Shared audio file discovery and playback preparation

use crate::audio::constants::AUDIO_EXTENSIONS;
use std::path::{Path, PathBuf};

/// Find audio file in meeting folder
/// Tries common names first, then scans for any file with an audio extension
pub fn find_audio_file(folder: &Path) -> Result<PathBuf, String> {
    let candidates = [
        "audio.mp4", "audio.m4a", "audio.wav", "audio.mp3",
        "audio.flac", "audio.ogg", "recording.mp4",
        "audio.mkv", "audio.webm", "audio.wma",
    ];

    for name in candidates {
        let path = folder.join(name);
        if path.exists() {
            return Ok(path);
        }
    }

    // Fallback: scan folder for any file with an audio extension
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
                    return Ok(path);
                }
            }
        }
    }

    Err(format!("No audio file found in: {}", folder.display()))
}

/// Transcode an audio file to WAV in the temp dir for webview playback.
/// Results are cached by (path, mtime) so repeat calls reuse the WAV.
pub fn prepare_audio_for_playback(file_path: &str) -> Result<String, String> {
    let src = PathBuf::from(file_path);
    if !src.is_file() {
        return Err(format!("Audio file not found: {}", file_path));
    }

    let mtime = std::fs::metadata(&src)
        .and_then(|m| m.modified())
        .map(|t| format!("{:?}", t))
        .unwrap_or_default();

    let cache_dir = std::env::temp_dir().join("meetily-playback");
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("Cannot create temp dir: {}", e))?;

    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file_path.hash(&mut hasher);
    mtime.hash(&mut hasher);
    let hash = hasher.finish();

    let out_path = cache_dir.join(format!("{:016x}.wav", hash));
    if out_path.is_file() {
        return Ok(out_path.to_string_lossy().to_string());
    }

    let ffmpeg_path = super::ffmpeg::find_ffmpeg_path()
        .ok_or_else(|| "FFmpeg not found. Cannot prepare audio for playback.".to_string())?;

    let partial_path = cache_dir.join(format!("{:016x}.wav.part", hash));
    let _ = std::fs::remove_file(&partial_path);

    let mut command = std::process::Command::new(ffmpeg_path);
    command
        .args(["-y", "-i", file_path, "-vn", "-ar", "44100"])
        .arg(&partial_path);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command
        .output()
        .map_err(|e| format!("Failed to run FFmpeg: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&partial_path);
        return Err(format!("FFmpeg transcode failed: {}", stderr.trim()));
    }

    std::fs::rename(&partial_path, &out_path)
        .map_err(|e| format!("Cannot move transcoded file: {}", e))?;

    Ok(out_path.to_string_lossy().to_string())
}
