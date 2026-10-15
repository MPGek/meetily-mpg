//! The live driver's audio source (05b D4): the VAD-filtered chunks a
//! recording delivers, presented to the shared core as windows.
//!
//! Test-only on purpose. The live path keeps the polyvoice streaming pipeline
//! for its incremental turns, because the shared core segments and embeds but
//! clusters once, at the end, and so has no incremental labels to show while a
//! recording runs. This source exists to state and to test the seam: given the
//! windows a live session would deliver, the core does the same work it does
//! for a saved recording. Nothing in a production build consumes it, so it is
//! not compiled into one.

use std::borrow::Cow;
use std::collections::VecDeque;

use super::super::core::source::{AudioSource, Window};
use super::super::DIARIZATION_SAMPLE_RATE;
use crate::audio::recording_state::AudioChunk;

pub(crate) struct VadChunks {
    chunks: VecDeque<AudioChunk>,
}

impl VadChunks {
    pub(crate) fn new(chunks: impl IntoIterator<Item = AudioChunk>) -> Self {
        Self {
            chunks: chunks.into_iter().collect(),
        }
    }
}

impl AudioSource for VadChunks {
    fn next_window(&mut self) -> Result<Option<Window<'_>>, String> {
        let Some(chunk) = self.chunks.pop_front() else {
            return Ok(None);
        };
        // The live pipeline resamples before the diarizer sees a chunk; a chunk
        // at another rate here is a wiring mistake, not audio to reinterpret.
        if chunk.sample_rate != DIARIZATION_SAMPLE_RATE {
            return Err(format!(
                "live chunk {} is {} Hz; the core reads {} Hz windows",
                chunk.chunk_id, chunk.sample_rate, DIARIZATION_SAMPLE_RATE
            ));
        }
        Ok(Some((chunk.timestamp, Cow::Owned(chunk.data))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recording_state::DeviceType;

    fn chunk(id: u64, timestamp: f64, sample_rate: u32, len: usize) -> AudioChunk {
        AudioChunk {
            data: vec![0.0; len],
            sample_rate,
            timestamp,
            chunk_id: id,
            device_type: DeviceType::Microphone,
            channels: 1,
        }
    }

    #[test]
    fn windows_come_out_in_order_with_their_own_start() {
        let mut source = VadChunks::new(vec![
            chunk(0, 1.5, 16_000, 8),
            chunk(1, 9.25, 16_000, 4),
        ]);
        let (start, window) = source.next_window().unwrap().unwrap();
        assert_eq!((start, window.len()), (1.5, 8));
        let (start, window) = source.next_window().unwrap().unwrap();
        assert_eq!((start, window.len()), (9.25, 4));
        assert!(source.next_window().unwrap().is_none());
    }

    #[test]
    fn a_chunk_at_another_rate_is_an_error_not_silently_reinterpreted() {
        let mut source = VadChunks::new(vec![chunk(7, 0.0, 48_000, 8)]);
        let err = source.next_window().unwrap_err();
        assert!(err.contains("chunk 7") && err.contains("48000 Hz"), "{err}");
    }
}
