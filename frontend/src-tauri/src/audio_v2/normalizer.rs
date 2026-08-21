//! Audio normalization (placeholder)
//! 
//! Automatic gain is intentionally NOT applied to live capture: boosting quiet
//! or non-speech content toward a loudness target is what caused ambient noise
//! (street sound) to become as loud as speech. This normalizer is a passthrough;
//! the captured level stays at unity gain and is controlled only by the user's
//! input volume. If finished recordings should ever be loudness-matched, it must
//! be done as an offline post-processing step (e.g. EBU R128 on the complete file).

use anyhow::Result;

/// Professional audio normalizer with EBU R128 compliance
pub struct AudioNormalizer {
    #[allow(dead_code)]
    target_lufs: f64,
}

impl AudioNormalizer {
    /// Create a new audio normalizer
    pub fn new(target_lufs: f64) -> Self {
        Self {
            target_lufs,
        }
    }

    /// Normalize audio to target LUFS level
    ///
    /// Deliberately a passthrough: microphone audio is captured at unity gain.
    /// No adaptive or per-chunk gain is applied to live capture.
    pub fn normalize(&mut self, audio: &[f32]) -> Vec<f32> {
        audio.to_vec()
    }
}