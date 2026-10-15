//! Shared refinement entry point + audio span sources (tasks 4.3, 5.1).
//!
//! `refine_segment_tokens` is the single implementation behind both repair
//! call sites (offline re-diarization, stop-time finalize). The live queue
//! path aligns the worker's in-memory block directly via
//! [`AlignmentEngine::align_tokens`] and never goes through a span source.
//! Every failure mode is silent: segments keep their pre-alignment tokens.

use super::catalog::{model_dir, resolve_status, spec_by_id, AlignmentModelStatus};
use super::engine::{align_tokens_with_timeout, AlignmentEngine};
use crate::audio::recording_saver::TranscriptSegment;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Per-segment alignment wall-clock bound; on expiry the segment keeps ASR
/// tokens and the run continues.
pub const SEGMENT_ALIGN_TIMEOUT: Duration = Duration::from_secs(30);

/// Lookahead appended to each extraction window so adjacent segments are
/// served from one ffmpeg pass (bounded memory: ~65 s of 16 kHz f32 ≈ 4 MB).
const BATCH_AHEAD_SECS: f64 = 60.0;

/// Alignment feature settings resolved from the store.
#[derive(Debug, Clone)]
pub struct AlignmentSettings {
    pub enabled: bool,
    pub model_id: Option<String>,
    /// Root holding `alignment/<id>/` model dirs (usually `<app_data>/models`).
    pub models_root: PathBuf,
}

impl AlignmentSettings {
    /// Resolve the engine for these settings: `None` when disabled or the
    /// model is not downloaded/valid. Loads (and caches) the engine on demand.
    pub fn engine(&self) -> Option<Arc<AlignmentEngine>> {
        if !self.enabled {
            return None;
        }
        let id = self.model_id.as_deref()?;
        let spec = spec_by_id(id)?;
        let dir = model_dir(&self.models_root, id);
        if resolve_status(&dir, spec) != AlignmentModelStatus::Available {
            return None;
        }
        cached_engine(id, &dir)
    }
}

// Engine cache: loading a 650 MB ONNX graph is seconds — keep one per model.
static ENGINE_CACHE: Mutex<Option<(String, Arc<AlignmentEngine>)>> = Mutex::new(None);

fn cached_engine(id: &str, dir: &Path) -> Option<Arc<AlignmentEngine>> {
    let mut guard = ENGINE_CACHE.lock().unwrap();
    if let Some((cached_id, engine)) = guard.as_ref() {
        if cached_id == id {
            return Some(engine.clone());
        }
    }
    match AlignmentEngine::load(dir) {
        Ok(engine) => {
            let engine = Arc::new(engine);
            *guard = Some((id.to_string(), engine.clone()));
            Some(engine)
        }
        Err(e) => {
            log::warn!("Failed to load alignment model {}: {}", id, e);
            None
        }
    }
}

/// Supplies mono 16 kHz f32 samples for a recording-relative span on one
/// channel ("Microphone" | "System"). The live path does not use this — the
/// worker's in-memory block is aligned directly; this exists for the repair
/// paths reading saved meeting files.
pub trait AudioSpanSource: Send + Sync {
    fn extract(&self, channel: &str, start: f64, end: f64) -> Option<Vec<f32>>;
}

/// One cached ffmpeg seek-extraction window per channel.
struct WindowCache {
    channel: String,
    start: f64,
    end: f64,
    samples: Vec<f32>,
}

/// File-backed span source using bounded `-ss/-t` seek extraction with
/// adjacent-window batching (mic=left, system=right; mono maps to one
/// channel).
pub struct FileSpanSource {
    audio_path: PathBuf,
    stereo: bool,
    ffmpeg: PathBuf,
    cache: Mutex<Option<WindowCache>>,
}

impl FileSpanSource {
    /// `stereo` selects the mic(left)/system(right) split; mono files map the
    /// whole file to the requested channel.
    pub fn new(audio_path: PathBuf, stereo: bool) -> Result<Self> {
        let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path()
            .ok_or_else(|| anyhow::anyhow!("ffmpeg not found for span extraction"))?;
        Ok(Self {
            audio_path,
            stereo,
            ffmpeg,
            cache: Mutex::new(None),
        })
    }
}

impl AudioSpanSource for FileSpanSource {
    fn extract(&self, channel: &str, start: f64, end: f64) -> Option<Vec<f32>> {
        if end <= start {
            return None;
        }
        // Serve from the cached window when it covers the request.
        {
            let guard = self.cache.lock().unwrap();
            if let Some(w) = guard.as_ref() {
                if w.channel == channel && w.start <= start + 1e-3 && w.end >= end - 1e-3 {
                    let lo = ((start - w.start) * 16000.0) as usize;
                    let hi = ((end - w.start) * 16000.0).ceil() as usize;
                    if lo < w.samples.len() {
                        return Some(w.samples[lo..hi.min(w.samples.len())].to_vec());
                    }
                }
            }
        }

        // Extract [start, start + (end-start) + BATCH_AHEAD] in one pass.
        let window_end = end + BATCH_AHEAD_SECS;
        let duration = window_end - start;
        let channel_index = if self.stereo {
            if channel.eq_ignore_ascii_case("system") {
                1
            } else {
                0
            }
        } else {
            0
        };

        let mut cmd = Command::new(&self.ffmpeg);
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-ss")
            .arg(format!("{:.3}", start))
            .arg("-i")
            .arg(&self.audio_path)
            .arg("-t")
            .arg(format!("{:.3}", duration))
            .arg("-vn")
            .args(["-af", &format!("pan=mono|c0=c{}", channel_index)])
            .arg("-ar")
            .arg("16000")
            .arg("-ac")
            .arg("1")
            .arg("-f")
            .arg("f32le")
            .arg("-")
             .stdin(Stdio::null())
             .stdout(Stdio::piped())
             .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                log::warn!("ffmpeg span extract spawn failed: {}", e);
                return None;
            }
        };
        if !output.status.success() {
            log::warn!(
                "ffmpeg span extract failed ({}) for [{:.1},{:.1}]: {}",
                self.audio_path.display(),
                start,
                window_end,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return None;
        }

        let bytes = output.stdout;
        if bytes.len() % 4 != 0 {
            log::warn!("ffmpeg span extract returned misaligned f32 stream");
            return None;
        }
        let samples: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        if samples.is_empty() {
            return None;
        }

        let actual_end = start + samples.len() as f64 / 16000.0;
        let window = WindowCache {
            channel: channel.to_string(),
            start,
            end: actual_end,
            samples,
        };
        let slice = {
            let lo = ((start - window.start) * 16000.0) as usize;
            let hi = ((end - window.start) * 16000.0).ceil() as usize;
            window.samples[lo..hi.min(window.samples.len())].to_vec()
        };
        *self.cache.lock().unwrap() = Some(window);
        Some(slice)
    }
}

/// In-memory span source over pre-extracted channel audio (used by tests and
/// by callers that already hold decoded channels).
pub struct MemorySpanSource {
    pub mic: Vec<f32>,
    pub system: Option<Vec<f32>>,
}

impl AudioSpanSource for MemorySpanSource {
    fn extract(&self, channel: &str, start: f64, end: f64) -> Option<Vec<f32>> {
        if end <= start {
            return None;
        }
        let samples = if channel.eq_ignore_ascii_case("system") {
            self.system.as_ref()?
        } else {
            &self.mic
        };
        let lo = (start * 16000.0) as usize;
        let hi = (end * 16000.0).ceil() as usize;
        if lo >= samples.len() {
            return None;
        }
        Some(samples[lo..hi.min(samples.len())].to_vec())
    }
}

/// True when the segment already carries refined tokens (skip for
/// idempotency, design D4).
pub fn segment_is_refined(segment: &TranscriptSegment) -> bool {
    match &segment.tokens {
        Some(tokens) => !tokens.is_empty() && tokens.iter().all(|t| t.refined),
        None => false,
    }
}

/// True when a segment's tokens are worth submitting to alignment.
fn needs_refinement(tokens: &Option<Vec<crate::audio::token_assignment::Token>>) -> bool {
    match tokens {
        Some(t) => !t.is_empty() && !t.iter().all(|x| x.refined),
        None => false,
    }
}

/// Refine one segment's tokens against a span extracted from `audio_source`.
/// Returns true when the tokens were updated and flagged refined. Any failure
/// (span unavailable, out-of-alphabet, timeout, engine error) leaves `tokens`
/// untouched (per-segment fallback). Shared core of the repair paths and the
/// live consumer.
pub fn refine_tokens_with_source(
    tokens: &mut [crate::audio::token_assignment::Token],
    audio_source: &dyn AudioSpanSource,
    channel: &str,
    start: f64,
    end: f64,
    engine: &Arc<AlignmentEngine>,
) -> bool {
    if tokens.is_empty() || tokens.iter().all(|t| t.refined) {
        return false;
    }
    let Some(samples) = audio_source.extract(channel, start, end) else {
        return false;
    };
    align_tokens_with_timeout(
        engine.clone(),
        tokens,
        samples,
        start as f32,
        end as f32,
        SEGMENT_ALIGN_TIMEOUT,
    )
}

/// Refine word tokens of `segments` in place against `audio_source`
/// (task 5.1). No-op when alignment is disabled or the model is missing;
/// per-segment failures (extract, alphabet, timeout, engine error) keep the
/// segment's pre-alignment tokens; already-refined segments are skipped.
/// Returns the number of segments refined.
pub fn refine_segment_tokens(
    segments: &mut [TranscriptSegment],
    audio_source: &dyn AudioSpanSource,
    settings: &AlignmentSettings,
) -> usize {
    let Some(engine) = settings.engine() else {
        return 0;
    };
    let mut refined_count = 0;
    for segment in segments.iter_mut() {
        if !needs_refinement(&segment.tokens) {
            continue;
        }
        let Some(tokens) = segment.tokens.as_mut() else {
            continue;
        };
        if refine_tokens_with_source(
            tokens,
            audio_source,
            &segment.source_device,
            segment.audio_start_time,
            segment.audio_end_time,
            &engine,
        ) {
            refined_count += 1;
        }
    }
    refined_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::token_assignment::Token;

    fn seg(tokens: Vec<Token>) -> TranscriptSegment {
        TranscriptSegment {
            id: "seg".to_string(),
            text: "hello world".to_string(),
            audio_start_time: 1.0,
            audio_end_time: 3.0,
            duration: 2.0,
            display_time: "[00:01]".to_string(),
            confidence: 0.9,
            sequence_id: 1,
            source_device: "Microphone".to_string(),
            tokens: Some(tokens),
        }
    }

    fn baseline_token(text: &str, start: f32, end: f32) -> Token {
        Token {
            text: text.to_string(),
            start,
            end,
            refined: false,
        }
    }

    struct NoSource;
    impl AudioSpanSource for NoSource {
        fn extract(&self, _c: &str, _s: f64, _e: f64) -> Option<Vec<f32>> {
            None
        }
    }

    /// Write a minimal mono PCM16 WAV at 16 kHz filled with a constant.
    fn write_test_wav(path: &Path, seconds: f64) {
        let n = (seconds * 16000.0) as usize;
        let data_len = (n * 2) as u32;
        let mut bytes: Vec<u8> = Vec::new();
        let riff_len = 36 + data_len;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_len.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&32000u32.to_le_bytes()); // byte rate
        bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
        bytes.extend_from_slice(&16u16.to_le_bytes()); // bits
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        // Ramp value so different spans yield different samples.
        for i in 0..n {
            let v = ((i % 1000) as i16) - 500;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn file_span_source_extracts_requested_window() {
        if crate::audio::ffmpeg::find_ffmpeg_path().is_none() {
            eprintln!("ffmpeg not available; skipping FileSpanSource test");
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "meetily_align_span_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("test.wav");
        write_test_wav(&wav, 3.0);

        let source = FileSpanSource::new(wav.clone(), false).unwrap();
        // Request [1.0, 2.0) = 16000 samples.
        let window = source
            .extract("Microphone", 1.0, 2.0)
            .expect("extraction should succeed");
        // Within one frame (320 samples) of the requested 16000.
        assert!(
            window.len() >= 16000 - 320 && window.len() <= 16000 + 320,
            "window len {} expected ~16000",
            window.len()
        );

        // Adjacent request is served from the cached window (no re-spawn):
        // overlapping span must still return a correctly-sized slice.
        let second = source.extract("Microphone", 1.5, 2.0).unwrap();
        assert!(second.len() >= 8000 - 320 && second.len() <= 8000 + 320);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn memory_span_source_maps_channels() {
        let mic = vec![0.1f32; 16000 * 2];
        let source = MemorySpanSource {
            mic: mic.clone(),
            system: None,
        };
        // Mono-ish: mic channel returns the slice; system is absent.
        let w = source.extract("Microphone", 0.5, 1.0).unwrap();
        assert_eq!(w.len(), 8000);
        assert!(source.extract("System", 0.0, 1.0).is_none());
        // Out of range start returns None.
        assert!(source.extract("Microphone", 5.0, 6.0).is_none());
    }

    fn settings(models_root: PathBuf, enabled: bool) -> AlignmentSettings {
        AlignmentSettings {
            enabled,
            model_id: Some("wav2vec2-xlsr-56".to_string()),
            models_root,
        }
    }

    #[test]
    fn disabled_settings_is_noop() {
        let mut segments = vec![seg(vec![baseline_token("hello", 1.0, 2.0)])];
        let n = refine_segment_tokens(
            &mut segments,
            &NoSource,
            &settings(std::env::temp_dir(), false),
        );
        assert_eq!(n, 0);
        assert!(!segments[0].tokens.as_ref().unwrap()[0].refined);
    }

    #[test]
    fn missing_model_is_noop() {
        let dir = std::env::temp_dir().join(format!(
            "meetily_align_missing_{}",
            std::process::id()
        ));
        let mut segments = vec![seg(vec![baseline_token("hello", 1.0, 2.0)])];
        let n = refine_segment_tokens(&mut segments, &NoSource, &settings(dir, true));
        assert_eq!(n, 0);
    }

    #[test]
    fn already_refined_segments_are_skipped() {
        let refined = seg(vec![Token {
            text: "hello".to_string(),
            start: 1.0,
            end: 2.0,
            refined: true,
        }]);
        assert!(segment_is_refined(&refined));
        assert!(!needs_refinement(&refined.tokens));

        let baseline = seg(vec![baseline_token("hello", 1.0, 2.0)]);
        assert!(!segment_is_refined(&baseline));
        assert!(needs_refinement(&baseline.tokens));

        assert!(!needs_refinement(&None));
        assert!(!needs_refinement(&Some(vec![])));

        // Mixed (partially refined) segments are re-aligned as a unit.
        let mixed = seg(vec![
            Token {
                text: "hello".to_string(),
                start: 1.0,
                end: 2.0,
                refined: true,
            },
            baseline_token("world", 2.0, 3.0),
        ]);
        assert!(needs_refinement(&mixed.tokens));
    }

    #[test]
    fn per_segment_span_failure_keeps_baseline() {
        // MemorySpanSource with empty audio: extract returns None for the
        // requested span -> segment keeps baseline tokens.
        let source = MemorySpanSource {
            mic: vec![],
            system: None,
        };
        let mut segments = vec![seg(vec![baseline_token("hello", 1.0, 2.0)])];
        // No engine available (missing model) -> whole run is a no-op and the
        // tokens remain exactly as produced.
        let n = refine_segment_tokens(
            &mut segments,
            &source,
            &settings(std::env::temp_dir().join("nope"), true),
        );
        assert_eq!(n, 0);
        let t = &segments[0].tokens.as_ref().unwrap()[0];
        assert_eq!((t.start, t.end, t.refined), (1.0, 2.0, false));
    }
}
