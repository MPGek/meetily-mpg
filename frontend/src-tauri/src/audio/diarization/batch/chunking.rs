//! Chunked batch diarization: split a long channel into overlapping chunks and
//! run the core over each, keeping peak memory at one chunk.

use std::borrow::Cow;

use log::info;

use super::super::core::segment::V2Core;
use super::super::core::source::{AudioSource, Window};
use super::super::core::units::StageTimings;
use super::super::{
    ClusteredEmbedding, DiarizationConfig, DiarizationSegment, PolyvoiceDiarizer,
    DIARIZATION_SAMPLE_RATE,
};

/// The batch driver's source for samples already in memory: the recording cut
/// into overlapping chunks, each resampled to the diarization rate when the
/// recording is at another one. Owns its windows so the core can take them one
/// at a time and peak memory stays at one chunk plus what is still queued.
pub(crate) struct MemoryWindows {
    chunks: std::vec::IntoIter<(f32, Vec<f32>)>,
    next_index: usize,
    sample_rate: u32,
}

impl MemoryWindows {
    pub(crate) fn new(chunks: Vec<(f32, Vec<f32>)>, sample_rate: u32) -> Self {
        Self {
            chunks: chunks.into_iter(),
            next_index: 0,
            sample_rate,
        }
    }
}

impl AudioSource for MemoryWindows {
    fn next_window(&mut self) -> Result<Option<Window<'_>>, String> {
        let Some((start_secs, samples)) = self.chunks.next() else {
            return Ok(None);
        };
        let index = self.next_index;
        self.next_index += 1;
        let window = if self.sample_rate != DIARIZATION_SAMPLE_RATE {
            crate::audio::audio_processing::resample(
                &samples,
                self.sample_rate,
                DIARIZATION_SAMPLE_RATE,
            )
            .map_err(|e| format!("Resampling failed for chunk {}: {}", index, e))?
        } else {
            samples
        };
        Ok(Some((start_secs as f64, Cow::Owned(window))))
    }
}

pub(crate) fn run_chunked_polyvoice_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
) -> Result<
    (
        Vec<DiarizationSegment>,
        Vec<ClusteredEmbedding>,
        StageTimings,
    ),
    String,
> {
    let chunk_duration = config.chunk_duration_secs();
    let chunks = channel_chunks(
        samples,
        sample_rate,
        chunk_duration,
        config.chunk_overlap_secs,
    );

    info!(
        "Chunked v2 diarization: {} chunks ({}s duration, {}s overlap)",
        chunks.len(),
        chunk_duration,
        config.chunk_overlap_secs
    );

    let mut core = V2Core::new(diarizer, config);
    core.process_source(&mut MemoryWindows::new(chunks, sample_rate))?;
    core.finish()
}

fn channel_chunks(
    samples: &[f32],
    sample_rate: u32,
    chunk_duration_secs: f32,
    overlap_secs: f32,
) -> Vec<(f32, Vec<f32>)> {
    if samples.is_empty() || chunk_duration_secs <= 0.0 {
        return Vec::new();
    }

    let chunk_samples = (chunk_duration_secs * sample_rate as f32) as usize;
    let overlap_samples = (overlap_secs * sample_rate as f32).max(0.0) as usize;
    let step = chunk_samples.saturating_sub(overlap_samples).max(1);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        let end = (start + chunk_samples).min(samples.len());
        let chunk = samples[start..end].to_vec();
        let chunk_start_seconds = start as f32 / sample_rate as f32;
        chunks.push((chunk_start_seconds, chunk));
        if end == samples.len() {
            break;
        }
        start += step;
        // Avoid generating a tiny trailing sliver; extend the last chunk instead.
        if start + step >= samples.len() && start < samples.len() {
            // Last iteration will grab [start..end].
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_splitting_preserves_overlap_and_offsets() {
        let sample_rate = 16000;
        let samples: Vec<f32> = (0..sample_rate * 30).map(|i| (i as f32).sin()).collect();
        let chunks = channel_chunks(&samples, sample_rate, 10.0, 5.0);

        assert!(!chunks.is_empty());
        // Every chunk except the last should be a full 10-second window.
        for (i, (_, chunk)) in chunks
            .iter()
            .enumerate()
            .take(chunks.len().saturating_sub(1))
        {
            assert_eq!(
                chunk.len(),
                sample_rate as usize * 10,
                "chunk {} has wrong size",
                i
            );
        }

        // Adjacent chunks should overlap by 5 seconds.
        for window in chunks.windows(2) {
            let start_a = window[0].0;
            let start_b = window[1].0;
            let diff = (start_b - start_a - 5.0).abs();
            assert!(
                diff < 0.01,
                "expected 5s overlap, got diff {}s",
                start_b - start_a
            );
        }

        // Last chunk should reach the end of the input.
        let (_, last_chunk) = chunks.last().unwrap();
        let last_start = chunks.last().unwrap().0;
        assert_eq!(
            (last_start * sample_rate as f32) as usize + last_chunk.len(),
            samples.len(),
            "last chunk must reach the end of the input"
        );
    }
}
