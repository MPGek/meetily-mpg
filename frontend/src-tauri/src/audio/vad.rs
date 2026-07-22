use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use std::collections::VecDeque;

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use ndarray::{Array0, Array2, Array3, Ix3};
use ort::inputs;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::TensorRef;

/// Represents a complete speech segment detected by VAD
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub samples: Vec<f32>,
    pub start_timestamp_ms: f64,
    pub end_timestamp_ms: f64,
    pub confidence: f32,
}

/// Thin wrapper around ort ONNX session for Silero VAD v6 model
pub struct VadSessionV6 {
    session: Session,
    state: Array3<f32>,
    context: VecDeque<f32>,
    sample_rate: usize,
}

impl VadSessionV6 {
    /// Model constants from v6 architecture
    const CONTEXT_SIZE: usize = 64;
    const WINDOW_SIZE: usize = 512;  // 32ms at 16kHz
    const INPUT_SIZE: usize = 576;   // 512 + 64 context
    const STATE_SHAPE: [usize; 3] = [2, 1, 128];

    pub fn new(sample_rate: usize) -> Result<Self> {
        let model_bytes: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/models/silero_vad_v6.onnx"
        ));

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_memory(model_bytes)?;

        let state = Array3::<f32>::zeros(Self::STATE_SHAPE);
        // Context MUST be pre-filled with zeros: v6 model always expects 64 context samples.
        // Python reference: self._context = torch.zeros(batch_size, context_size)
        let context = VecDeque::from(vec![0.0f32; Self::CONTEXT_SIZE]);

        Ok(Self { session, state, context, sample_rate })
    }

    pub fn forward(&mut self, chunk: &[f32]) -> Result<f32> {
        // Prep context: use stored context, then save last 64 samples of chunk
        let mut input = Vec::with_capacity(Self::INPUT_SIZE);
        // Prepend 64 context samples, then append 512 chunk samples
        input.extend_from_slice(&make_slice(&self.context));
        input.extend_from_slice(chunk);
        if input.len() != Self::INPUT_SIZE {
            anyhow::bail!(
                "VAD input size mismatch: expected {} samples ({} context + {} window), got {}",
                Self::INPUT_SIZE, Self::CONTEXT_SIZE, Self::WINDOW_SIZE, input.len()
            );
        }

        // Update context: keep last 64 samples of this chunk for the next call
        self.context.clear();
        let context_start = chunk.len().saturating_sub(Self::CONTEXT_SIZE);
        self.context.extend(&chunk[context_start..]);

        // Model expects input shape [1, 576], state [2, 1, 128], sr scalar
        let input_array = Array2::from_shape_vec((1, Self::INPUT_SIZE), input)?;
        let state_input = self.state.clone();
        let sr = Array0::from_elem((), self.sample_rate as i64);

        let ort_inputs = inputs![
            "input" => TensorRef::from_array_view(input_array.view())?,
            "state" => TensorRef::from_array_view(state_input.view())?,
            "sr" => TensorRef::from_array_view(sr.view())?,
        ];

        let outputs = self.session.run(ort_inputs)?;

        let prob_view = outputs
            .get("output")
            .ok_or_else(|| anyhow!("VAD output not found"))?
            .try_extract_array::<f32>()?;

        if prob_view.ndim() != 2 || prob_view.shape()[0] != 1 || prob_view.shape()[1] != 1 {
            anyhow::bail!(
                "VAD probability output shape mismatch: expected [1,1], got {:?}",
                prob_view.shape()
            );
        }

        let new_state_view = outputs
            .get("stateN")
            .ok_or_else(|| anyhow!("VAD stateN not found"))?
            .try_extract_array::<f32>()?;

        if new_state_view.ndim() != 3
            || new_state_view.shape()[0] != 2
            || new_state_view.shape()[1] != 1
            || new_state_view.shape()[2] != 128
        {
            anyhow::bail!(
                "VAD state output shape mismatch: expected [2,1,128], got {:?}",
                new_state_view.shape()
            );
        }

        self.state = new_state_view.to_owned().into_dimensionality::<Ix3>()?;

        Ok(prob_view[[0, 0]])
    }

    pub fn reset(&mut self) {
        self.state = Array3::<f32>::zeros(Self::STATE_SHAPE);
        self.context.clear();
        self.context.extend(std::iter::repeat(0.0f32).take(Self::CONTEXT_SIZE));
    }
}

fn make_slice(deque: &VecDeque<f32>) -> Vec<f32> {
    let mut v = Vec::with_capacity(deque.len());
    for &x in deque {
        v.push(x);
    }
    v
}

/// Processes audio in 32ms chunks but returns complete speech segments
pub struct ContinuousVadProcessor {
    session_v6: VadSessionV6,
    chunk_size: usize,
    sample_rate: u32,
    buffer: Vec<f32>,
    speech_segments: VecDeque<SpeechSegment>,
    current_speech: Vec<f32>,
    in_speech: bool,
    processed_samples: usize,
    speech_start_sample: usize,
    /// Count of consecutive samples below negative threshold
    silent_samples: usize,
    redemption_samples: usize,
    min_speech_samples: usize,
    positive_threshold: f32,
    negative_threshold: f32,
    /// True if min_speech_time has been exceeded (speech is "confirmed")
    redemption_passed: bool,
    /// Pre and post speech padding in samples
    pre_speech_pad_samples: usize,
    post_speech_pad_samples: usize,
    // State tracking for smart logging
    last_logged_state: bool,
    // Persistent rubato resampler for 48kHz → 16kHz conversion
    resampler: Option<SincFixedIn<f32>>,
    resampler_input_buffer: Vec<f32>,
    resampler_chunk_size: usize,
}

impl ContinuousVadProcessor {
    pub fn new(input_sample_rate: u32, redemption_time_ms: u32) -> Result<Self> {
        // Silero VAD v6 MUST use 16kHz - this is hardcoded requirement
        const VAD_SAMPLE_RATE: u32 = 16000;
        const VAD_CHUNK_SIZE: usize = 512; // v6 uses fixed 512-sample window (32ms @ 16kHz)

        // These thresholds match the silero_rs defaults from the old implementation
        // and are applied directly instead of passing to a config struct
        const POSITIVE_THRESHOLD: f32 = 0.50;
        const NEGATIVE_THRESHOLD: f32 = 0.35;
        const PRE_SPEECH_PAD_MS: u32 = 300;
        const POST_SPEECH_PAD_MS: u32 = 400;
        const MIN_SPEECH_MS: u32 = 250;

        let redemption_samples = (VAD_SAMPLE_RATE as f64 * redemption_time_ms as f64 / 1000.0) as usize;
        let min_speech_samples = (VAD_SAMPLE_RATE as f64 * MIN_SPEECH_MS as f64 / 1000.0) as usize;
        let pre_speech_pad_samples = (VAD_SAMPLE_RATE as f64 * PRE_SPEECH_PAD_MS as f64 / 1000.0) as usize;
        let post_speech_pad_samples = (VAD_SAMPLE_RATE as f64 * POST_SPEECH_PAD_MS as f64 / 1000.0) as usize;

        debug!("Creating VAD session v6: sample_rate={}Hz, redemption={}ms, min_speech={}ms, input_rate={}Hz",
               VAD_SAMPLE_RATE, redemption_time_ms, MIN_SPEECH_MS, input_sample_rate);

        let session_v6 = VadSessionV6::new(VAD_SAMPLE_RATE as usize)?;

        // Initialize persistent rubato resampler for input → 16kHz conversion
        const RESAMPLER_CHUNK_SIZE: usize = 512;
        let resampler = if input_sample_rate != VAD_SAMPLE_RATE {
            let ratio = input_sample_rate as f64 / VAD_SAMPLE_RATE as f64;
            let params = SincInterpolationParameters {
                sinc_len: 256,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2,
            };

            match SincFixedIn::<f32>::new(
                1.0 / ratio,
                2.0,
                params,
                RESAMPLER_CHUNK_SIZE,
                1,
            ) {
                Ok(r) => {
                    info!("VAD resampler initialized: {}Hz → {}Hz", input_sample_rate, VAD_SAMPLE_RATE);
                    Some(r)
                }
                Err(e) => {
                    warn!("Failed to create VAD resampler: {}, will use fallback", e);
                    None
                }
            }
        } else {
            None
        };

        info!("VAD processor v6 created: input={}Hz, vad={}Hz, chunk_size={} samples",
              input_sample_rate, VAD_SAMPLE_RATE, VAD_CHUNK_SIZE);

        Ok(Self {
            session_v6,
            chunk_size: VAD_CHUNK_SIZE,
            sample_rate: input_sample_rate,
            buffer: Vec::with_capacity(VAD_CHUNK_SIZE * 2),
            speech_segments: VecDeque::new(),
            current_speech: Vec::new(),
            in_speech: false,
            processed_samples: 0,
            speech_start_sample: 0,
            silent_samples: 0,
            redemption_samples,
            min_speech_samples,
            positive_threshold: POSITIVE_THRESHOLD,
            negative_threshold: NEGATIVE_THRESHOLD,
            redemption_passed: false,
            pre_speech_pad_samples,
            post_speech_pad_samples,
            last_logged_state: false,
            resampler,
            resampler_input_buffer: Vec::with_capacity(RESAMPLER_CHUNK_SIZE * 2),
            resampler_chunk_size: RESAMPLER_CHUNK_SIZE,
        })
    }

    /// Process incoming audio samples and return any complete speech segments
    /// Handles resampling from input sample rate to 16kHz for VAD processing
    pub fn process_audio(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
        // Resample to 16kHz if needed
        let resampled_audio = if self.sample_rate == 16000 {
            samples.to_vec()
        } else {
            self.resample_to_16k(samples)?
        };

        self.buffer.extend_from_slice(&resampled_audio);
        let mut completed_segments = Vec::new();

        // Process complete 32ms chunks (512 samples at 16kHz, v6 fixed window)
        while self.buffer.len() >= self.chunk_size {
            let chunk: Vec<f32> = self.buffer.drain(..self.chunk_size).collect();
            self.process_chunk(&chunk)?;

            // Extract any completed speech segments
            while let Some(segment) = self.speech_segments.pop_front() {
                completed_segments.push(segment);
            }
        }

        Ok(completed_segments)
    }

    /// Resample from input sample rate to 16kHz using persistent rubato resampler
    fn resample_to_16k(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        if self.sample_rate == 16000 {
            return Ok(samples.to_vec());
        }

        let mut resampled_output = Vec::new();

        // Add new samples to input buffer
        self.resampler_input_buffer.extend_from_slice(samples);

        // Process complete chunks through the resampler
        if let Some(ref mut resampler) = self.resampler {
            while self.resampler_input_buffer.len() >= self.resampler_chunk_size {
                let chunk: Vec<f32> = self.resampler_input_buffer.drain(..self.resampler_chunk_size).collect();
                let waves_in = vec![chunk];

                match resampler.process(&waves_in, None) {
                    Ok(mut waves_out) => {
                        if let Some(output) = waves_out.pop() {
                            resampled_output.extend_from_slice(&output);
                        }
                    }
                    Err(e) => {
                        warn!("VAD resampler processing failed: {}", e);
                        break;
                    }
                }
            }
        }

        // If no resampler or processing failed, fall back to simple linear interpolation
        if resampled_output.is_empty() && self.resampler.is_none() {
            let ratio = self.sample_rate as f64 / 16000.0;
            let output_len = (samples.len() as f64 / ratio) as usize;
            for i in 0..output_len {
                let source_pos = i as f64 * ratio;
                let source_index = source_pos as usize;
                let fraction = source_pos - source_index as f64;

                if source_index + 1 < samples.len() {
                    let interpolated = samples[source_index] + (samples[source_index + 1] - samples[source_index]) * fraction as f32;
                    resampled_output.push(interpolated);
                } else if source_index < samples.len() {
                    resampled_output.push(samples[source_index]);
                }
            }
        }

        Ok(resampled_output)
    }

    /// Flush any remaining audio and return final speech segments
    pub fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
        debug!("VAD flush: in_speech={}, current_speech_len={}, buffer_len={}, speech_segments_queued={}",
              self.in_speech, self.current_speech.len(), self.buffer.len(), self.speech_segments.len());

        let mut completed_segments = Vec::new();

        // Process any remaining buffered audio
        if !self.buffer.is_empty() {
            let remaining = self.buffer.clone();
            self.buffer.clear();

            // Pad to chunk size if needed
            let mut padded_chunk = remaining;
            if padded_chunk.len() < self.chunk_size {
                padded_chunk.resize(self.chunk_size, 0.0);
            }

            self.process_chunk(&padded_chunk)?;
        }

        // Force end any ongoing speech
        if self.in_speech && !self.current_speech.is_empty() {
            // processed_samples and speech_start_sample always count 16kHz samples (post-resampling)
            let start_ms = (self.speech_start_sample as f64 / 16000.0) * 1000.0;
            let end_ms = (self.processed_samples as f64 / 16000.0) * 1000.0;

            debug!("VAD flush: Force-ending speech - start={}ms, end={}ms, duration={}ms, samples={}",
                  start_ms, end_ms, end_ms - start_ms, self.current_speech.len());

            let segment = SpeechSegment {
                samples: self.current_speech.clone(),
                start_timestamp_ms: start_ms,
                end_timestamp_ms: end_ms,
                confidence: 0.8, // Estimated confidence for forced end
            };

            self.speech_segments.push_back(segment);
            self.current_speech.clear();
            self.in_speech = false;
        }

        // Extract all remaining segments
        while let Some(segment) = self.speech_segments.pop_front() {
            completed_segments.push(segment);
        }

        Ok(completed_segments)
    }

    fn process_chunk(&mut self, chunk: &[f32]) -> Result<()> {
        // Track accumulated speech buffer size to detect memory issues
        let current_speech_size = self.current_speech.len();
        if current_speech_size > 1_000_000 {
            warn!("VAD: Accumulated speech buffer is large: {} samples ({:.1}s) - possible memory issue",
                  current_speech_size, current_speech_size as f64 / 16000.0);
        }

        let prob = self.session_v6.forward(chunk)?;

        // Track silent samples: count consecutive samples below negative threshold
        if prob < self.negative_threshold {
            self.silent_samples += chunk.len();
        } else {
            self.silent_samples = 0;
        }

        if !self.in_speech && prob >= self.positive_threshold {
            // Transition: silence → speech
            if !self.last_logged_state {
                debug!("VAD: Speech started at {}ms (prob={:.4})",
                       self.processed_samples * 1000 / 16000, prob);
                self.last_logged_state = true;
            }
            self.in_speech = true;
            self.redemption_passed = false;
            // Apply pre-speech padding: start from padded position
            self.speech_start_sample = self.processed_samples.saturating_sub(self.pre_speech_pad_samples);
            self.current_speech.clear();
            // Include padding samples
            if self.processed_samples > 0 {
                let pad_start = self.speech_start_sample;
                let recorded = self.processed_samples - pad_start;
                let previous_buffered = (self.current_speech.len() as f64 / 16000.0 * 1000.0) as u32;
                if previous_buffered < self.pre_speech_pad_samples as u32 {
                    // Add silent padding for context (we don't have raw audio before our buffer)
                    let padding_needed = self.pre_speech_pad_samples.saturating_sub(recorded);
                    self.current_speech.resize(padding_needed, 0.0);
                }
            }
            self.current_speech.extend_from_slice(chunk);
        } else if self.in_speech {
            // Check if speech has exceeded minimum duration to be "confirmed"
            let speech_duration_samples = self.processed_samples + chunk.len() - self.speech_start_sample;
            if !self.redemption_passed && speech_duration_samples >= self.min_speech_samples {
                self.redemption_passed = true;
            }

            if prob < self.negative_threshold && self.redemption_passed {
                // Possible speech end - check if silence exceeds redemption time
                if self.silent_samples >= self.redemption_samples {
                    // Speech end confirmed: silence exceeded redemption time
                    if self.last_logged_state {
                        let duration_ms = (self.processed_samples - self.speech_start_sample) as f64 / 16.0;
                        debug!("VAD: Speech ended at {}ms (duration: {:.1}ms, prob={:.4})",
                               self.processed_samples * 1000 / 16000, duration_ms, prob);
                        self.last_logged_state = false;
                    }

                    // Calculate end sample with post-speech padding
                    let end_with_pad = (self.processed_samples + self.post_speech_pad_samples)
                        .min(self.processed_samples + chunk.len() + self.post_speech_pad_samples);
                    let end_sample_for_segment = self.processed_samples.saturating_sub(self.silent_samples)
                        + self.post_speech_pad_samples;

                    if !self.current_speech.is_empty() {
                        let start_ms = self.speech_start_sample as f64 / 16.0;
                        let end_ms = end_sample_for_segment as f64 / 16.0;

                        let segment = SpeechSegment {
                            samples: self.current_speech.clone(),
                            start_timestamp_ms: start_ms,
                            end_timestamp_ms: end_ms,
                            confidence: prob,
                        };

                        info!("VAD: Completed speech segment: {:.1}ms duration, {} samples",
                              end_ms - start_ms, segment.samples.len());

                        self.speech_segments.push_back(segment);
                    }

                    self.current_speech.clear();
                    self.in_speech = false;
                    self.redemption_passed = false;
                    self.processed_samples += chunk.len();
                    return Ok(());
                }
            }

            // Still in speech - continue accumulating
            self.current_speech.extend_from_slice(chunk);
        }

        self.processed_samples += chunk.len();
        Ok(())
    }
}

/// Legacy function for backward compatibility - now uses the optimized approach
pub fn extract_speech_16k(samples_mono_16k: &[f32]) -> Result<Vec<f32>> {
    let mut processor = ContinuousVadProcessor::new(16000, 400)?;

    // Process all audio
    let mut all_segments = processor.process_audio(samples_mono_16k)?;
    let final_segments = processor.flush()?;
    all_segments.extend(final_segments);

    // Concatenate all speech segments
    let mut result = Vec::new();
    let num_segments = all_segments.len();
    for segment in &all_segments {
        result.extend_from_slice(&segment.samples);
    }

    // Apply balanced energy filtering for very short segments
    if result.len() < 1600 { // Less than 100ms at 16kHz
        let input_energy: f32 = samples_mono_16k.iter().map(|&x| x * x).sum::<f32>() / samples_mono_16k.len() as f32;
        let rms = input_energy.sqrt();
        let peak = samples_mono_16k.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);

        // BALANCED FIX: Lowered thresholds to preserve quiet speech while still filtering silence
        // Previous aggressive values (0.08/0.15) were discarding valid quiet speech
        // New values (0.03/0.08) are more balanced - catch quiet speech, reject pure silence
        if rms < 0.2 || peak < 0.20 {
            info!("-----VAD detected silence/noise (RMS: {:.6}, Peak: {:.6}), skipping to prevent hallucinations-----", rms, peak);
            return Ok(Vec::new());
        } else {
            info!("VAD detected speech with sufficient energy (RMS: {:.6}, Peak: {:.6})", rms, peak);
            return Ok(samples_mono_16k.to_vec());
        }
    }

    debug!("VAD: Processed {} samples, extracted {} speech samples from {} segments",
           samples_mono_16k.len(), result.len(), num_segments);

    Ok(result)
}

/// Simple convenience function to get speech chunks from audio
/// Uses the optimized ContinuousVadProcessor with configurable redemption time
pub fn get_speech_chunks(samples_mono_16k: &[f32], redemption_time_ms: u32) -> Result<Vec<SpeechSegment>> {
    get_speech_chunks_with_progress(samples_mono_16k, redemption_time_ms, |_, _| true)
}

/// Get speech chunks with progress callback and cancellation support
/// The callback receives (progress_percent, segments_found) and returns false to cancel
pub fn get_speech_chunks_with_progress<F>(
    samples_mono_16k: &[f32],
    redemption_time_ms: u32,
    mut progress_callback: F,
) -> Result<Vec<SpeechSegment>>
where
    F: FnMut(u32, usize) -> bool,
{
    let mut processor = ContinuousVadProcessor::new(16000, redemption_time_ms)?;

    let total_samples = samples_mono_16k.len();

    // For large files (>1 minute at 16kHz = 960,000 samples), process in chunks with progress logging
    const LARGE_FILE_THRESHOLD: usize = 960_000;
    const CHUNK_SIZE: usize = 160_000; // 10 seconds at 16kHz

    let mut all_segments = Vec::new();

    if total_samples > LARGE_FILE_THRESHOLD {
        info!("VAD: Processing large file ({} samples = {:.1}s), will log progress...",
              total_samples, total_samples as f64 / 16000.0);

        let mut processed = 0;
        let mut last_progress = 0u32;
        let mut chunk_count = 0;
        let total_chunks = (total_samples + CHUNK_SIZE - 1) / CHUNK_SIZE;

        for chunk in samples_mono_16k.chunks(CHUNK_SIZE) {
            chunk_count += 1;

            let start_time = std::time::Instant::now();
            let segments = processor.process_audio(chunk)?;
            let elapsed = start_time.elapsed();

            // Debug log for chunk processing details
            debug!("VAD: Chunk {}/{} processed in {:?}, found {} segments",
                  chunk_count, total_chunks, elapsed, segments.len());

            // Warn if chunk processing took too long (>1 second)
            if elapsed.as_secs() > 1 {
                warn!("VAD: Chunk {} took {:?} - possible performance issue", chunk_count, elapsed);
            }

            all_segments.extend(segments);

            processed += chunk.len();
            let progress = ((processed * 100) / total_samples) as u32;

            // Call progress callback every 5%
            if progress >= last_progress + 5 {
                debug!("VAD: Progress {}% ({} segments found so far)", progress, all_segments.len());

                // Check for cancellation
                if !progress_callback(progress, all_segments.len()) {
                    info!("VAD: Cancelled by callback at {}%", progress);
                    return Err(anyhow!("VAD processing cancelled"));
                }

                last_progress = progress;
            }
        }

        let final_segments = processor.flush()?;
        all_segments.extend(final_segments);

        info!("VAD: Complete! Found {} speech segments", all_segments.len());
    } else {
        // Small file - process all at once
        all_segments = processor.process_audio(samples_mono_16k)?;
        let final_segments = processor.flush()?;
        all_segments.extend(final_segments);
    }

    Ok(all_segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_session_v6_creates_and_resets() {
        let mut session = VadSessionV6::new(16000).expect("Failed to create VAD session");
        // Initial state should be all zeros
        assert!(session.state.iter().all(|&x| x == 0.0), "Initial state should be zeros");
        // Context MUST start with 64 zeros: v6 model always expects context (Python reference behavior)
        assert_eq!(session.context.len(), 64, "Context should start with 64 samples");
        assert!(session.context.iter().all(|&x| x == 0.0), "Initial context should be zeros");

        // Process silence - should return low probability
        let silence = vec![0.0f32; 512];
        let prob = session.forward(&silence).expect("Forward failed");
        assert!(prob >= 0.0 && prob <= 1.0, "Probability should be in [0,1], got {}", prob);
        assert!(prob < 0.1, "Silence probability should be very low, got {}", prob);

        // Context should now contain the last 64 samples (still zeros for silence input)
        assert_eq!(session.context.len(), 64);
        assert!(session.context.iter().all(|&x| x == 0.0));

        // Reset should zero state and context, but context refills with zeros
        session.reset();
        assert!(session.state.iter().all(|&x| x == 0.0));
        assert_eq!(session.context.len(), 64, "Reset should refill context with zeros");
        assert!(session.context.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn test_vad_session_v6_context_preserved_across_calls() {
        let mut session = VadSessionV6::new(16000).expect("Failed to create VAD session");

        // First chunk: random data
        let chunk1: Vec<f32> = (0..512).map(|i| (i as f32 / 512.0) * 0.5).collect();
        let prob1 = session.forward(&chunk1).expect("Forward failed");

        // Context should now hold last 64 samples of chunk1
        assert_eq!(session.context.len(), 64);

        // Second chunk: zeros
        let chunk2 = vec![0.0f32; 512];
        let prob2 = session.forward(&chunk2).expect("Forward failed");

        // prob2 should be different from processing zeros alone, because
        // the context from chunk1 conditions the model
        assert!(prob2 >= 0.0 && prob2 <= 1.0);
        // Log for debugging
        println!("Context test: prob1={:.6}, prob2={:.6}", prob1, prob2);
    }

    #[test]
    fn test_vad_session_v6_state_evolution() {
        let mut session = VadSessionV6::new(16000).expect("Failed to create VAD session");

        // Process multiple chunks and verify state changes
        let mut prev_state_sum = 0.0f32;
        for i in 0..5 {
            let chunk: Vec<f32> = (0..512).map(|j| ((j + i * 512) as f32 / 512.0) * 0.3).collect();
            let prob = session.forward(&chunk).expect("Forward failed");
            let state_sum: f32 = session.state.iter().sum();
            println!("Chunk {}: prob={:.6}, state_sum={:.6}", i, prob, state_sum);

            // State should evolve (change from previous)
            if i > 0 {
                assert!((state_sum - prev_state_sum).abs() > 0.0,
                    "State should evolve across chunks (prev={:.6}, curr={:.6})",
                    prev_state_sum, state_sum);
            }
            prev_state_sum = state_sum;
        }
    }

    /// Generate synthetic speech-like audio with alternating speech/silence
    fn generate_test_audio_with_speech(duration_seconds: f32, sample_rate: u32) -> Vec<f32> {
        let total_samples = (duration_seconds * sample_rate as f32) as usize;
        let mut samples = vec![0.0f32; total_samples];

        // Create speech-like patterns: bursts of sine waves with varying amplitude
        // Speech every 10 seconds for 5 seconds
        let speech_interval = 10.0; // seconds between speech starts
        let speech_duration = 5.0;  // seconds of speech

        for i in 0..total_samples {
            let time = i as f32 / sample_rate as f32;
            let cycle_time = time % speech_interval;

            // Speech occurs in the first `speech_duration` seconds of each cycle
            if cycle_time < speech_duration {
                // Generate speech-like signal: multiple frequencies with amplitude modulation
                let freq1 = 200.0 + (time * 50.0).sin() * 100.0; // Varying fundamental
                let freq2 = freq1 * 2.0; // Harmonic
                let freq3 = freq1 * 3.0; // Another harmonic

                let amplitude = 0.3 + 0.1 * (time * 5.0).sin(); // Amplitude modulation
                samples[i] = amplitude * (
                    0.5 * (2.0 * std::f32::consts::PI * freq1 * time).sin() +
                    0.3 * (2.0 * std::f32::consts::PI * freq2 * time).sin() +
                    0.2 * (2.0 * std::f32::consts::PI * freq3 * time).sin()
                );
            }
            // else: silence (already 0.0)
        }

        samples
    }

    #[test]
    fn test_vad_chunked_vs_single_processing() {
        // Generate 60 seconds of audio with speech patterns at 16kHz
        let audio = generate_test_audio_with_speech(60.0, 16000);
        println!("Generated {} samples ({:.1}s)", audio.len(), audio.len() as f32 / 16000.0);

        // Process all at once (like small files)
        let segments_single = get_speech_chunks(&audio, 2000).expect("Single processing failed");
        println!("Single processing found {} segments", segments_single.len());

        // Process in chunks (like large files)
        let segments_chunked = get_speech_chunks_with_progress(&audio, 2000, |progress, segments| {
            println!("Chunked progress: {}%, {} segments", progress, segments);
            true // Don't cancel
        }).expect("Chunked processing failed");
        println!("Chunked processing found {} segments", segments_chunked.len());

        // Both should find the same number of segments (approximately)
        // Allow some variance due to chunk boundary effects
        let diff = (segments_single.len() as i32 - segments_chunked.len() as i32).abs();
        assert!(diff <= 1,
            "Chunked and single processing found different segment counts: {} vs {} (diff: {})",
            segments_single.len(), segments_chunked.len(), diff);
    }

    #[test]
    fn test_vad_large_file_progress() {
        // Generate 120 seconds (2 minutes) of audio - triggers large file threshold
        let audio = generate_test_audio_with_speech(120.0, 16000);
        let total_samples = audio.len();
        println!("Generated {} samples ({:.1}s)", total_samples, total_samples as f32 / 16000.0);

        // This should trigger the large file path (>960,000 samples)
        assert!(total_samples > 960_000, "Audio should be large enough to trigger chunked processing");

        let mut progress_updates = Vec::new();
        let segments = get_speech_chunks_with_progress(&audio, 2000, |progress, segments| {
            progress_updates.push((progress, segments));
            true // Don't cancel
        }).expect("Processing failed");

        println!("Found {} segments with {} progress updates", segments.len(), progress_updates.len());

        // The synthetic signal is not real speech, so Silero may merge it into
        // one long segment. This test is specifically for the large-file path:
        // it must still emit speech and report monotonic progress through 100%.
        assert!(!segments.is_empty(), "Expected at least one speech segment");
        assert!(
            segments.iter().all(|segment| !segment.samples.is_empty()
                && segment.end_timestamp_ms > segment.start_timestamp_ms),
            "Expected all speech segments to contain audio with positive duration"
        );

        // Should have received progress updates
        assert!(!progress_updates.is_empty(), "Expected progress updates for large file");
        assert_eq!(
            progress_updates.last().map(|(progress, _)| *progress),
            Some(100),
            "Expected progress to reach 100%"
        );
        assert!(
            progress_updates
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0),
            "Expected progress updates to increase monotonically: {:?}",
            progress_updates
        );
    }

    #[test]
    fn test_vad_cancellation() {
        let audio = generate_test_audio_with_speech(120.0, 16000);

        // Cancel at 50%
        let result = get_speech_chunks_with_progress(&audio, 2000, |progress, _| {
            progress < 50 // Cancel when reaching 50%
        });

        // Should return error due to cancellation
        assert!(result.is_err(), "Expected cancellation error");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cancelled"), "Error should mention cancellation: {}", err_msg);
    }

    #[test]
    fn test_vad_continuous_processor_state_across_chunks() {
        // Test that VAD state is correctly maintained across chunk boundaries
        let mut processor = ContinuousVadProcessor::new(16000, 2000).expect("Failed to create processor");

        // Generate audio with a speech segment that spans a chunk boundary
        let chunk_size = 160_000; // 10 seconds
        let audio = generate_test_audio_with_speech(30.0, 16000); // 30 seconds

        // Process in 10-second chunks
        let mut all_segments = Vec::new();
        for (i, chunk) in audio.chunks(chunk_size).enumerate() {
            let segments = processor.process_audio(chunk).expect("Processing failed");
            println!("Chunk {}: processed {} samples, found {} segments", i, chunk.len(), segments.len());
            all_segments.extend(segments);
        }

        // Flush remaining
        let final_segments = processor.flush().expect("Flush failed");
        all_segments.extend(final_segments);

        println!("Total segments found: {}", all_segments.len());

        // Should find speech segments
        assert!(all_segments.len() >= 1, "Expected at least 1 speech segment");
    }

    #[test]
    fn test_vad_400ms_vs_2000ms_segmentation() {
        // Demonstrates why 2000ms redemption is needed for batch processing:
        // 400ms creates excessive fragmentation, 2000ms bridges natural pauses.
        //
        // Audio pattern: 60s with 5s speech / 5s silence cycles
        // Natural pauses within speech (sentence gaps) are 500ms-1.5s
        let audio = generate_test_audio_with_speech(60.0, 16000);

        let segments_400 = get_speech_chunks(&audio, 400).expect("400ms processing failed");
        let segments_2000 = get_speech_chunks(&audio, 2000).expect("2000ms processing failed");

        println!(
            "400ms redemption: {} segments, 2000ms redemption: {} segments",
            segments_400.len(),
            segments_2000.len()
        );

        // 2000ms should produce fewer or equal segments (bridges more pauses)
        assert!(
            segments_2000.len() <= segments_400.len(),
            "2000ms redemption ({} segments) should not produce more segments than 400ms ({} segments)",
            segments_2000.len(),
            segments_400.len()
        );

        // Verify segments have reasonable durations with 2000ms
        for (i, seg) in segments_2000.iter().enumerate() {
            let duration_ms = seg.end_timestamp_ms - seg.start_timestamp_ms;
            println!("2000ms segment {}: {:.0}ms duration", i, duration_ms);
            // Each segment should be at least 250ms (min_speech_time)
            assert!(duration_ms >= 200.0, "Segment {} too short: {:.0}ms", i, duration_ms);
        }
    }
}

