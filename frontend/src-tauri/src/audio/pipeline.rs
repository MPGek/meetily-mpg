use super::batch_processor::AudioMetricsBatcher;
use crate::batch_audio_metric;
use anyhow::Result;
use log::{debug, error, info, warn};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::audio_processing::{audio_to_mono, HighPassFilter, NoiseSuppressionProcessor};
use super::devices::AudioDevice;
use super::recording_state::{AudioChunk, AudioError, DeviceType, RecordingState};
use super::vad::{merge_segments, ContinuousVadProcessor, SpeechSegment, VadConfig};

/// Ring buffer for synchronized audio mixing
/// Accumulates samples from mic and system streams until we have aligned windows
struct AudioMixerRingBuffer {
    mic_buffer: VecDeque<f32>,
    system_buffer: VecDeque<f32>,
    window_size_samples: usize, // Fixed mixing window (e.g., 50ms)
    max_buffer_size: usize,     // Safety limit (e.g., 100ms)
}

impl AudioMixerRingBuffer {
    fn new(sample_rate: u32) -> Self {
        // Use 50ms windows for mixing
        let window_ms = 600.0;
        let window_size_samples = (sample_rate as f32 * window_ms / 1000.0) as usize;

        // CRITICAL FIX: Increase max buffer to 400ms for system audio stability
        // System audio (especially Core Audio on macOS) can have significant jitter
        // due to sample-by-sample streaming → batching → channel transmission
        // Accounts for: RNNoise buffering + Core Audio jitter + processing delays
        let max_buffer_size = window_size_samples * 8; // 400ms (was 200ms)

        info!(
            "🔊 Ring buffer initialized: window={}ms ({} samples), max={}ms ({} samples)",
            window_ms,
            window_size_samples,
            window_ms * 8.0,
            max_buffer_size
        );

        Self {
            mic_buffer: VecDeque::with_capacity(max_buffer_size),
            system_buffer: VecDeque::with_capacity(max_buffer_size),
            window_size_samples,
            max_buffer_size,
        }
    }

    fn add_samples(&mut self, device_type: DeviceType, samples: Vec<f32>) {
        static SAMPLE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = SAMPLE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count % 200 == 0 {
            debug!(
                "📊 Ring buffer status: mic={} samples, sys={} samples (max={})",
                self.mic_buffer.len(),
                self.system_buffer.len(),
                self.max_buffer_size
            );
        }

        // Data is now mono — bulk extend directly into the correct buffer
        match device_type {
            DeviceType::Microphone => self.mic_buffer.extend(samples),
            DeviceType::System => self.system_buffer.extend(samples),
        }

        // CRITICAL FIX: Add warnings before dropping samples
        // This helps diagnose timing issues in production
        if self.mic_buffer.len() > self.max_buffer_size {
            warn!(
                "⚠️ Microphone buffer overflow: {} > {} samples, dropping oldest {} samples",
                self.mic_buffer.len(),
                self.max_buffer_size,
                self.mic_buffer.len() - self.max_buffer_size
            );
        }
        if self.system_buffer.len() > self.max_buffer_size {
            error!("🔴 SYSTEM AUDIO BUFFER OVERFLOW: {} > {} samples, dropping {} samples - THIS CAUSES DISTORTION!",
                  self.system_buffer.len(), self.max_buffer_size,
                  self.system_buffer.len() - self.max_buffer_size);
        }

        // Safety: prevent buffer overflow (keep only last 200ms)
        while self.mic_buffer.len() > self.max_buffer_size {
            self.mic_buffer.pop_front();
        }
        while self.system_buffer.len() > self.max_buffer_size {
            self.system_buffer.pop_front();
        }
    }

    fn can_mix(&self) -> bool {
        self.mic_buffer.len() >= self.window_size_samples
            || self.system_buffer.len() >= self.window_size_samples
    }

    fn extract_window(&mut self) -> Option<(Vec<f32>, Vec<f32>)> {
        if !self.can_mix() {
            return None;
        }

        // Extract mic window with zero-padding for incomplete buffers
        // Zero-padding (silence) is preferred over last-sample-hold to prevent artifacts

        // Extract mic window (or pad with zeros if insufficient data)
        let mic_window = if self.mic_buffer.len() >= self.window_size_samples {
            // Enough mic data - drain window
            self.mic_buffer.drain(0..self.window_size_samples).collect()
        } else if !self.mic_buffer.is_empty() {
            // Some mic data but not enough - consume all + pad with zeros
            let available: Vec<f32> = self.mic_buffer.drain(..).collect();
            let mut padded = Vec::with_capacity(self.window_size_samples);
            padded.extend_from_slice(&available);

            // Use zero-padding (silence) to prevent repetition artifacts
            // Zero-padding is inaudible at 48kHz sample rate
            padded.resize(self.window_size_samples, 0.0);

            padded
        } else {
            // No mic data - return silence
            vec![0.0; self.window_size_samples]
        };

        // Extract system window (or pad with zeros if insufficient data)
        let sys_window = if self.system_buffer.len() >= self.window_size_samples {
            // Enough system data - drain window
            self.system_buffer
                .drain(0..self.window_size_samples)
                .collect()
        } else if !self.system_buffer.is_empty() {
            // Some system data but not enough - consume all + pad with zeros
            let available: Vec<f32> = self.system_buffer.drain(..).collect();
            let mut padded = Vec::with_capacity(self.window_size_samples);
            padded.extend_from_slice(&available);

            // Use zero-padding (silence) to prevent repetition artifacts
            // Zero-padding is inaudible at 48kHz sample rate
            padded.resize(self.window_size_samples, 0.0);

            padded
        } else {
            // No system data - return silence
            vec![0.0; self.window_size_samples]
        };

        Some((mic_window, sys_window))
    }
}

/// Interleave mic and system audio into stereo: [mic₀, sys₀, mic₁, sys₁, ...]
fn interleave_stereo(mic: &[f32], sys: &[f32]) -> Vec<f32> {
    let max_len = mic.len().max(sys.len());
    let mut stereo = Vec::with_capacity(max_len * 2);
    for i in 0..max_len {
        stereo.push(mic.get(i).copied().unwrap_or(0.0));
        stereo.push(sys.get(i).copied().unwrap_or(0.0));
    }
    stereo
}

/// Simplified audio capture without broadcast channels
#[derive(Clone)]
pub struct AudioCapture {
    device: Arc<AudioDevice>,
    state: Arc<RecordingState>,
    sample_rate: u32, // Original device sample rate
    channels: u16,
    chunk_counter: Arc<std::sync::atomic::AtomicU64>,
    device_type: DeviceType,
    recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    needs_resampling: bool, // Flag if resampling is required
    // CRITICAL FIX: Persistent resampler to preserve energy across chunks
    resampler: Arc<std::sync::Mutex<Option<SincFixedIn<f32>>>>,
    // Buffering for variable-size chunks → fixed-size resampler input
    resampler_input_buffer: Arc<std::sync::Mutex<Vec<f32>>>,
    resampler_chunk_size: usize, // Fixed chunk size for resampler (512 samples)
    // Audio enhancement processors (microphone only)
    noise_suppressor: Arc<std::sync::Mutex<Option<NoiseSuppressionProcessor>>>,
    high_pass_filter: Arc<std::sync::Mutex<Option<HighPassFilter>>>,
    // Note: Using global recording timestamp for synchronization
}

impl AudioCapture {
    pub fn new(
        device: Arc<AudioDevice>,
        state: Arc<RecordingState>,
        sample_rate: u32,
        channels: u16,
        device_type: DeviceType,
        recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    ) -> Self {
        // CRITICAL FIX: Detect if resampling is needed
        // Pipeline expects 48kHz, but Bluetooth devices often report 8kHz, 16kHz, or 44.1kHz
        const TARGET_SAMPLE_RATE: u32 = 48000;
        let needs_resampling = sample_rate != TARGET_SAMPLE_RATE;

        // Detect device kind (Bluetooth vs Wired) for adaptive processing
        // Use reasonable defaults for buffer size (512 samples is typical)
        let device_kind =
            super::device_detection::InputDeviceKind::detect(&device.name, 512, sample_rate);

        if needs_resampling {
            warn!("⚠️ SAMPLE RATE MISMATCH DETECTED ⚠️");
            warn!(
                "🔄 [{:?}] Audio device '{}' ({:?}) reports {} Hz (pipeline expects {} Hz)",
                device_type, device.name, device_kind, sample_rate, TARGET_SAMPLE_RATE
            );
            warn!(
                "🔄 Automatic resampling will be applied: {} Hz → {} Hz",
                sample_rate, TARGET_SAMPLE_RATE
            );

            // Log which resampling strategy will be used
            let ratio = TARGET_SAMPLE_RATE as f64 / sample_rate as f64;
            let strategy = if ratio >= 2.0 {
                "High-quality upsampling (sinc_len=512, Cubic interpolation)"
            } else if ratio >= 1.5 {
                "Moderate upsampling (sinc_len=384, Cubic)"
            } else if ratio > 1.0 {
                "Small upsampling (sinc_len=256, Linear)"
            } else if ratio <= 0.5 {
                "Anti-aliased downsampling (sinc_len=512, Cubic)"
            } else {
                "Moderate downsampling (sinc_len=384, Linear)"
            };
            info!("   Resampling strategy: {}", strategy);
        } else {
            info!(
                "✅ [{:?}] Audio device '{}' ({:?}) uses {} Hz (matches pipeline)",
                device_type, device.name, device_kind, sample_rate
            );
        }

        // Initialize audio enhancement processors for MICROPHONE ONLY
        // System audio doesn't need enhancement (already clean)
        let (noise_suppressor, high_pass_filter) = if matches!(device_type, DeviceType::Microphone)
        {
            // Initialize noise suppression (RNNoise) at 48kHz - CONDITIONAL based on flag
            let ns = if super::ffmpeg_mixer::RNNOISE_APPLY_ENABLED {
                match NoiseSuppressionProcessor::new(TARGET_SAMPLE_RATE) {
                    Ok(processor) => {
                        info!("✅ RNNoise noise suppression ENABLED for microphone '{}' (10-15 dB reduction)", device.name);
                        Some(processor)
                    }
                    Err(e) => {
                        warn!("⚠️ Failed to create noise suppressor: {}, continuing without noise suppression", e);
                        None
                    }
                }
            } else {
                info!("ℹ️ RNNoise noise suppression DISABLED for microphone '{}' (flag: RNNOISE_APPLY_ENABLED=false)", device.name);
                info!("   Whisper handles noise well internally - RNNoise is optional");
                None
            };

            // Initialize high-pass filter (removes rumble below 80 Hz)
            let hpf = {
                let filter = HighPassFilter::new(TARGET_SAMPLE_RATE, 80.0);
                info!(
                    "✅ High-pass filter initialized for microphone '{}' (cutoff: 80 Hz)",
                    device.name
                );
                Some(filter)
            };

            (ns, hpf)
        } else {
            // System audio: no enhancement needed
            info!(
                "ℹ️ System audio '{}' captured raw (no enhancement)",
                device.name
            );
            (None, None)
        };

        // CRITICAL FIX: Initialize persistent resampler to preserve energy across chunks
        // Creating a new resampler per chunk causes energy amplification and incorrect output sizes
        // Use fixed chunk size of 512 samples with buffering for variable-size input
        const RESAMPLER_CHUNK_SIZE: usize = 512;

        let resampler = if needs_resampling {
            let ratio = TARGET_SAMPLE_RATE as f64 / sample_rate as f64;

            // Adaptive parameters based on sample rate ratio (same logic as resample_audio)
            let (sinc_len, interpolation_type, oversampling) = if ratio >= 2.0 {
                (512, SincInterpolationType::Cubic, 512)
            } else if ratio >= 1.5 {
                (384, SincInterpolationType::Cubic, 384)
            } else if ratio > 1.0 {
                (256, SincInterpolationType::Linear, 256)
            } else if ratio <= 0.5 {
                (512, SincInterpolationType::Cubic, 512)
            } else {
                (384, SincInterpolationType::Linear, 384)
            };

            let params = SincInterpolationParameters {
                sinc_len,
                f_cutoff: 0.95,
                interpolation: interpolation_type,
                oversampling_factor: oversampling,
                window: WindowFunction::BlackmanHarris2,
            };

            match SincFixedIn::<f32>::new(
                ratio,
                2.0, // Maximum relative deviation
                params,
                RESAMPLER_CHUNK_SIZE,
                1, // Mono
            ) {
                Ok(resampler) => {
                    info!(
                        "✅ Persistent resampler initialized for '{}' ({}Hz → {}Hz, chunk_size={})",
                        device.name, sample_rate, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE
                    );
                    info!("   Buffering enabled for variable-size chunks (e.g., 320, 512, 1024, etc.)");
                    Some(resampler)
                }
                Err(e) => {
                    warn!(
                        "⚠️ Failed to create persistent resampler: {}, will use fallback",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        Self {
            device,
            state,
            sample_rate,
            channels,
            chunk_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            device_type,
            recording_sender,
            needs_resampling,
            resampler: Arc::new(std::sync::Mutex::new(resampler)),
            resampler_input_buffer: Arc::new(std::sync::Mutex::new(Vec::with_capacity(
                RESAMPLER_CHUNK_SIZE * 2,
            ))),
            resampler_chunk_size: RESAMPLER_CHUNK_SIZE,
            noise_suppressor: Arc::new(std::sync::Mutex::new(noise_suppressor)),
            high_pass_filter: Arc::new(std::sync::Mutex::new(high_pass_filter)),
            // Using global recording time for sync
        }
    }

    /// Process audio data directly from callback
    pub fn process_audio_data(&self, data: &[f32]) {
        // Check if still recording
        if !self.state.is_recording() {
            return;
        }

        // Convert to mono if needed
        let mut mono_data = if self.channels > 1 {
            audio_to_mono(data, self.channels)
        } else {
            data.to_vec()
        };

        // CRITICAL FIX: Resample to 48kHz if device uses different sample rate
        // This fixes Bluetooth devices (like Sony WH-1000XM4) that report 16kHz or 44.1kHz
        // Without this, audio is sped up 3x and VAD fails
        //
        // IMPORTANT: Uses PERSISTENT resampler with BUFFERING to preserve energy across chunks
        // Creating a new resampler per chunk causes energy amplification (173.5% RMS)
        // Buffering handles variable chunk sizes (320, 512, 1024, etc.) by accumulating to fixed 512-sample chunks
        const TARGET_SAMPLE_RATE: u32 = 48000;
        if self.needs_resampling {
            let before_len = mono_data.len();
            let before_rms = if !mono_data.is_empty() {
                (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt()
            } else {
                0.0
            };

            // Use persistent resampler with buffering to handle variable chunk sizes
            let mut resampled_output = Vec::new();
            let mut used_persistent_resampler = false;

            if let Ok(mut buffer_lock) = self.resampler_input_buffer.lock() {
                // Add new samples to buffer
                buffer_lock.extend_from_slice(&mono_data);

                // Process complete chunks through the resampler
                if let Ok(mut resampler_lock) = self.resampler.lock() {
                    if let Some(ref mut resampler) = *resampler_lock {
                        used_persistent_resampler = true;

                        // Process as many complete chunks as we have
                        while buffer_lock.len() >= self.resampler_chunk_size {
                            // Extract exactly chunk_size samples
                            let chunk: Vec<f32> =
                                buffer_lock.drain(0..self.resampler_chunk_size).collect();

                            // Rubato expects input as Vec<Vec<f32>> (one Vec per channel)
                            let waves_in = vec![chunk];

                            match resampler.process(&waves_in, None) {
                                Ok(mut waves_out) => {
                                    if let Some(output) = waves_out.pop() {
                                        resampled_output.extend_from_slice(&output);
                                    }
                                }
                                Err(e) => {
                                    warn!("⚠️ Persistent resampler processing failed: {}", e);
                                    used_persistent_resampler = false;
                                    break;
                                }
                            }
                        }
                        // Remaining samples in buffer will be processed in next iteration
                    }
                }
            }

            // CRITICAL: Only update mono_data if we got output from persistent resampler
            // If buffer is accumulating (< 512 samples), skip this chunk - data is safely buffered
            // and will be processed in next iteration with proper resampling
            let has_resampled_output = !resampled_output.is_empty();

            if has_resampled_output {
                mono_data = resampled_output;
            } else if !used_persistent_resampler {
                // Only fallback if persistent resampler is not available at all
                mono_data = super::audio_processing::resample_audio(
                    &mono_data,
                    self.sample_rate,
                    TARGET_SAMPLE_RATE,
                );
            } else {
                // Buffering: samples are accumulating in buffer, waiting for 512-sample chunk
                // Don't send partial/unprocessed data - return early
                // Audio is NOT lost - it's in the buffer and will be processed next iteration
                return;
            }

            // Log resampling only occasionally to avoid spam
            let chunk_id = self.chunk_counter.load(std::sync::atomic::Ordering::SeqCst);
            if chunk_id % 100 == 0 && has_resampled_output {
                let after_len = mono_data.len();
                let after_rms = if !mono_data.is_empty() {
                    (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt()
                } else {
                    0.0
                };
                let ratio = TARGET_SAMPLE_RATE as f64 / self.sample_rate as f64;
                let rms_preservation = if before_rms > 0.0 {
                    (after_rms / before_rms) * 100.0
                } else {
                    100.0
                };

                let buffer_size = if let Ok(buf) = self.resampler_input_buffer.lock() {
                    buf.len()
                } else {
                    0
                };

                info!(
                    "🔄 [{:?}] Persistent buffered resampler: {}Hz → {}Hz (ratio: {:.2}x)",
                    self.device_type, self.sample_rate, TARGET_SAMPLE_RATE, ratio
                );
                info!(
                    "   Chunk {}: {} → {} samples, RMS preservation: {:.1}%, buffer: {}",
                    chunk_id, before_len, after_len, rms_preservation, buffer_size
                );
            }
        }

        // AUDIO ENHANCEMENT PIPELINE (Microphone Only)
        // Processing order is critical: high-pass → noise suppression
        // No automatic gain is applied: the mic signal stays at unity gain so
        // quiet/non-speech content is never amplified toward a loudness target
        if matches!(self.device_type, DeviceType::Microphone) {
            // STEP 1: Apply high-pass filter to remove low-frequency rumble (< 80 Hz)
            if let Ok(mut hpf_lock) = self.high_pass_filter.lock() {
                if let Some(ref mut filter) = *hpf_lock {
                    mono_data = filter.process(&mono_data);
                }
            }

            // STEP 2: Apply RNNoise noise suppression (10-15 dB reduction) - CONDITIONAL
            if super::ffmpeg_mixer::RNNOISE_APPLY_ENABLED {
                if let Ok(mut ns_lock) = self.noise_suppressor.lock() {
                    if let Some(ref mut suppressor) = *ns_lock {
                        let before_len = mono_data.len();
                        mono_data = suppressor.process(&mono_data);
                        let after_len = mono_data.len();

                        // CRITICAL MONITORING: Track buffer health
                        let chunk_id = self.chunk_counter.load(std::sync::atomic::Ordering::SeqCst);
                        if chunk_id % 100 == 0 {
                            let buffered = suppressor.buffered_samples();
                            let length_delta = (before_len as i32 - after_len as i32).abs();

                            debug!("🔇 Noise suppression health: in={}, out={}, delta={}, buffered={}, RMS={:.4}",
                                   before_len, after_len, length_delta, buffered,
                                   if !mono_data.is_empty() {
                                       (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt()
                                   } else { 0.0 });

                            // WARN if accumulating samples (potential latency buildup)
                            if buffered > 1000 {
                                warn!("⚠️ RNNoise accumulating samples: {} buffered (potential latency issue!)",
                                      buffered);
                            }

                            // WARN if significant length mismatch
                            if length_delta > 50 {
                                warn!(
                                    "⚠️ RNNoise length mismatch: input={} output={} (delta={})",
                                    before_len, after_len, length_delta
                                );
                            }
                        }
                    }
                }
            }

            // STEP 3: No automatic gain is applied. The mic signal is intentionally
            // kept at unity gain so that quiet stretches and ambient noise are never
            // boosted toward a fixed loudness target during live capture.
        }

        // Create audio chunk with stream-specific timestamp (get ID first for logging)
        let chunk_id = self
            .chunk_counter
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // RAW AUDIO: No gain applied here - will be applied AFTER mixing
        // This prevents amplifying system audio bleed-through in the microphone

        // DIAGNOSTIC: Log audio levels for debugging (especially mic issues)
        // if chunk_id % 100 == 0 && !mono_data.is_empty() {
        //     let raw_rms = (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt();
        //     let raw_peak = mono_data.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);

        //         info!("🎙️ [{:?}] Chunk {} - Raw: RMS={:.6}, Peak={:.6}",
        //               self.device_type, chunk_id, raw_rms, raw_peak);

        //     // Warn if microphone is completely silent
        //     if matches!(self.device_type, DeviceType::Microphone) && raw_rms == 0.0 && raw_peak == 0.0 {
        //         warn!("⚠️ Microphone producing ZERO audio - check permissions or hardware!");
        //     }
        // }
        // else if chunk_id % 100 == 0 && matches!(self.device_type, DeviceType::System) {
        //     let raw_rms = (mono_data.iter().map(|&x| x * x).sum::<f32>() / mono_data.len() as f32).sqrt();
        //     let raw_peak = mono_data.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
        //     info!("🔊 [{:?}] Chunk {} - Raw: RMS={:.6}, Peak={:.6}",
        //       self.device_type, chunk_id, raw_rms, raw_peak);

        //     // Warn if system audio is completely silent
        //     if raw_rms == 0.0 && raw_peak == 0.0 {
        //         warn!("⚠️ System audio producing ZERO audio - check permissions or hardware!");
        //     }
        // }

        // Use global recording timestamp for proper synchronization
        let timestamp = self.state.get_recording_duration().unwrap_or(0.0);

        // RAW AUDIO CHUNK: No gain applied - will be mixed and gained downstream
        // Use 48kHz if we resampled, otherwise use original rate
        // Interleave into stereo: mic on left channel, system on right channel
        let audio_chunk = AudioChunk {
            data: mono_data,
            sample_rate: if self.needs_resampling {
                48000
            } else {
                self.sample_rate
            },
            timestamp,
            chunk_id,
            device_type: self.device_type.clone(),
            channels: 1,
        };

        // NOTE: Raw audio is NOT sent to recording saver to prevent echo
        // Only the mixed audio (from AudioPipeline) is saved to file (see pipeline.rs:726-736)
        // This ensures we only record once: mic + system properly mixed
        // Individual raw streams go only to the transcription pipeline below

        // Send to processing pipeline for transcription
        if let Err(e) = self.state.send_audio_chunk(audio_chunk) {
            // Check if this is the "pipeline not ready" error
            if e.to_string().contains("Audio pipeline not ready") {
                // This is expected during initialization, just log it as debug
                debug!("Audio pipeline not ready yet, skipping chunk {}", chunk_id);
                return;
            }

            warn!("Failed to send audio chunk: {}", e);
            // More specific error handling based on failure reason
            let error = if e.to_string().contains("channel closed") {
                AudioError::ChannelClosed
            } else if e.to_string().contains("full") {
                AudioError::BufferOverflow
            } else {
                AudioError::ProcessingFailed
            };
            self.state.report_error(error);
        } else {
            debug!("Sent audio chunk {} ({} samples)", chunk_id, data.len());
        }
    }

    /// Handle stream errors with enhanced disconnect detection
    pub fn handle_stream_error(&self, error: cpal::StreamError) {
        error!("Audio stream error for {}: {}", self.device.name, error);

        let error_str = error.to_string().to_lowercase();

        // Enhanced error detection for device disconnection
        let audio_error = if error_str.contains("device is no longer available")
            || error_str.contains("device not found")
            || error_str.contains("device disconnected")
            || error_str.contains("no such device")
            || error_str.contains("device unavailable")
            || error_str.contains("device removed")
        {
            warn!("🔌 Device disconnect detected for: {}", self.device.name);
            AudioError::DeviceDisconnected
        } else if error_str.contains("permission") || error_str.contains("access denied") {
            AudioError::PermissionDenied
        } else if error_str.contains("channel closed") {
            AudioError::ChannelClosed
        } else if error_str.contains("stream") && error_str.contains("failed") {
            AudioError::StreamFailed
        } else {
            warn!("Unknown audio error: {}", error);
            AudioError::StreamFailed
        };

        self.state.report_error(audio_error);
    }
}

/// VAD-driven audio processing pipeline
/// Uses Voice Activity Detection to segment speech in real-time and send only speech to Whisper
pub struct AudioPipeline {
    receiver: mpsc::UnboundedReceiver<AudioChunk>,
    transcription_sender: mpsc::UnboundedSender<AudioChunk>,
    embedding_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    state: Arc<RecordingState>,
    vad_processor_mic: ContinuousVadProcessor,
    vad_processor_sys: ContinuousVadProcessor,
    sample_rate: u32,
    chunk_id_counter: u64,
    // Performance optimization: reduce logging frequency
    last_summary_time: std::time::Instant,
    processed_chunks: u64,
    // Smart batching for audio metrics
    metrics_batcher: Option<AudioMetricsBatcher>,
    // Ring buffer for synchronized audio interleaving
    ring_buffer: AudioMixerRingBuffer,
    // Live telemetry: published buffer fills and voice-activity activity
    // (online-diarization-telemetry). None only if telemetry is not installed.
    telemetry: Option<Arc<super::telemetry::PipelineTelemetry>>,
    // Recording sender for stereo interleaved audio
    recording_sender_for_mixed: Option<mpsc::UnboundedSender<AudioChunk>>,
    // Throttle: only surface the first saver-delivery failure per run so a dead
    // channel is reported without spamming the recording error surface.
    recording_save_failure_reported: bool,
    // Per-source mono accumulation buffers for window-batched VAD dispatch
    vad_buffer_mic: Vec<f32>,
    vad_buffer_sys: Vec<f32>,
    vad_dispatch_threshold_samples: usize,
    // Segment accumulation buffers for merge-before-transcribe
    vad_pending_mic: Vec<SpeechSegment>,
    vad_pending_sys: Vec<SpeechSegment>,
    live_vad_config: VadConfig,
    // Real-time anchoring for live transcription timestamps. VAD segments are
    // timestamped in the per-source sample domain; a stream that starts late or
    // delivers nothing during silent periods (e.g. WASAPI loopback) shifts every
    // later segment into the past. These anchors map the VAD counter domain (ms)
    // to recording-relative seconds using the chunk capture timestamps, so
    // transcription timestamps stay aligned with the audio file.
    vad_mic_buffer_real_base: Option<f64>,
    vad_sys_buffer_real_base: Option<f64>,
    vad_mic_anchors: Vec<(f64, f64)>,
    vad_sys_anchors: Vec<(f64, f64)>,
}

/// Remap VAD sample-domain segment timestamps (per-source counter ms) to
/// recording-relative seconds using per-dispatch real-time anchors.
/// `anchors` maps VAD counter ms -> real recording seconds at the start of each
/// dispatched buffer; empty anchors leave the segment unchanged.
fn remap_segment_times_to_real(anchors: &[(f64, f64)], segments: &mut [SpeechSegment]) {
    for seg in segments.iter_mut() {
        // Find the latest anchor whose counter is at or before the segment start.
        let idx = anchors.partition_point(|(counter_ms, _)| *counter_ms <= seg.start_timestamp_ms);
        if idx > 0 {
            let (counter_ms, real_sec) = anchors[idx - 1];
            let shift_ms = (real_sec - counter_ms / 1000.0) * 1000.0;
            seg.start_timestamp_ms = (seg.start_timestamp_ms + shift_ms).max(0.0);
            seg.end_timestamp_ms = (seg.end_timestamp_ms + shift_ms).max(0.0);
        }
    }
}

impl AudioPipeline {
    pub fn new(
        receiver: mpsc::UnboundedReceiver<AudioChunk>,
        transcription_sender: mpsc::UnboundedSender<AudioChunk>,
        embedding_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
        state: Arc<RecordingState>,
        target_chunk_duration_ms: u32,
        sample_rate: u32,
        mic_device_name: String,
        mic_device_kind: super::device_detection::InputDeviceKind,
        system_device_name: String,
        system_device_kind: super::device_detection::InputDeviceKind,
    ) -> Self {
        // Log device characteristics for adaptive buffering
        info!("🎛️ AudioPipeline initializing with device characteristics:");
        info!(
            "   Mic: '{}' ({:?}) - Buffer: {:?}",
            mic_device_name,
            mic_device_kind,
            mic_device_kind.buffer_timeout()
        );
        info!(
            "   System: '{}' ({:?}) - Buffer: {:?}",
            system_device_name,
            system_device_kind,
            system_device_kind.buffer_timeout()
        );

        // Device kind information can be used for adaptive buffering in the future
        // For now, we log it for monitoring and potential optimization
        let _ = (
            mic_device_name,
            mic_device_kind,
            system_device_name,
            system_device_kind,
        );

        // Create VAD processors with balanced redemption time for speech accumulation
        // The VAD processor handles 48kHz->16kHz resampling internally
        // Two independent instances: one for mic, one for system audio
        let vad_config = super::vad::VadConfig::live();

        let vad_processor_mic = match ContinuousVadProcessor::new(sample_rate, vad_config.clone()) {
            Ok(processor) => {
                info!("VAD Mic processor created");
                processor
            }
            Err(e) => {
                error!("Failed to create VAD processor for mic: {}", e);
                panic!("VAD processor creation failed: {}", e);
            }
        };

        let vad_config_clone = vad_config.clone();
        let vad_processor_sys = match ContinuousVadProcessor::new(sample_rate, vad_config) {
            Ok(processor) => {
                info!("VAD Sys processor created");
                processor
            }
            Err(e) => {
                error!("Failed to create VAD processor for system audio: {}", e);
                panic!("VAD processor creation failed: {}", e);
            }
        };

        // Initialize ring buffer for synchronized audio interleaving
        let ring_buffer = AudioMixerRingBuffer::new(sample_rate);

        // Window-batched VAD dispatch: accumulate samples until threshold
        // 200ms at 48kHz = 9600 samples (reduces VAD call rate from ~100/sec to ~5/sec)
        let vad_dispatch_threshold_samples = (sample_rate as f32 * 0.2) as usize;

        // Note: target_chunk_duration_ms is ignored - VAD controls segmentation now
        let _ = target_chunk_duration_ms;

        // Live telemetry for this session. Thresholds come from the same
        // constants that drive the operations they gate, so the reported fill
        // always matches what actually fires.
        let telemetry = super::telemetry::install_pipeline(sample_rate);
        for channel in [
            telemetry.channel(&DeviceType::Microphone),
            telemetry.channel(&DeviceType::System),
        ] {
            channel
                .vad_dispatch
                .set_threshold(vad_dispatch_threshold_samples as u64);
            channel
                .mix
                .set_threshold(ring_buffer.window_size_samples as u64);
            channel.set_pending_triggers(500, 25_000);
        }

        Self {
            receiver,
            transcription_sender,
            embedding_sender,
            state,
            vad_processor_mic,
            vad_processor_sys,
            sample_rate,
            chunk_id_counter: 0,
            // Performance optimization: reduce logging frequency
            last_summary_time: std::time::Instant::now(),
            processed_chunks: 0,
            // Initialize metrics batcher for smart batching
            metrics_batcher: Some(AudioMetricsBatcher::new()),
            // Initialize ring buffer
            ring_buffer,
            telemetry: Some(telemetry),
            recording_sender_for_mixed: None, // Will be set by manager
            recording_save_failure_reported: false,
            // Initialize VAD accumulation buffers
            vad_buffer_mic: Vec::with_capacity(vad_dispatch_threshold_samples * 2),
            vad_buffer_sys: Vec::with_capacity(vad_dispatch_threshold_samples * 2),
            vad_dispatch_threshold_samples,
            vad_pending_mic: Vec::new(),
            vad_pending_sys: Vec::new(),
            live_vad_config: vad_config_clone,
            vad_mic_buffer_real_base: None,
            vad_sys_buffer_real_base: None,
            vad_mic_anchors: Vec::new(),
            vad_sys_anchors: Vec::new(),
        }
    }

    /// Publish one channel's live telemetry: the fills of the buffers that gate
    /// this channel's operations, and its voice-activity activity. Called once
    /// per audio chunk; touches only atomics.
    fn publish_channel_telemetry(&self, device_type: &DeviceType) {
        let Some(telemetry) = self.telemetry.as_deref() else {
            return;
        };
        let channel = telemetry.channel(device_type);

        let (dispatch_fill, vad, pending, mix_fill) = match device_type {
            DeviceType::Microphone => (
                self.vad_buffer_mic.len(),
                &self.vad_processor_mic,
                &self.vad_pending_mic,
                self.ring_buffer.mic_buffer.len(),
            ),
            DeviceType::System => (
                self.vad_buffer_sys.len(),
                &self.vad_processor_sys,
                &self.vad_pending_sys,
                self.ring_buffer.system_buffer.len(),
            ),
        };

        channel.vad_dispatch.set_fill(dispatch_fill as u64);
        channel.set_vad_frames(
            super::telemetry::PipelineTelemetry::frames_from_processed_ms(vad.processed_ms()),
        );
        channel.set_vad_speaking(vad.is_in_speech());
        channel.set_pending(
            pending.len() as u64,
            pending
                .iter()
                .map(|segment| {
                    (segment.end_timestamp_ms - segment.start_timestamp_ms).max(0.0) as u64
                })
                .sum(),
        );
        channel.mix.set_fill(mix_fill as u64);
    }

    /// Merge accumulated VAD segments and dispatch to transcription.
    /// Segments with gap < 500ms are merged into coherent chunks.
    fn flush_pending_segments(&mut self, device_type: DeviceType) {
        let pending = match device_type {
            DeviceType::Microphone => &mut self.vad_pending_mic,
            DeviceType::System => &mut self.vad_pending_sys,
        };
        if pending.is_empty() {
            return;
        }

        let merged = merge_segments(pending, 500.0, 25 * 16000);
        for segment in merged {
            if segment.samples.len() < self.live_vad_config.min_segment_samples {
                continue;
            }
            info!(
                "📤 Sending merged segment [{:?}]: {:.0}ms, {} samples",
                device_type,
                segment.end_timestamp_ms - segment.start_timestamp_ms,
                segment.samples.len()
            );

            let transcription_chunk = AudioChunk {
                data: segment.samples,
                sample_rate: 16000,
                timestamp: segment.start_timestamp_ms / 1000.0,
                chunk_id: self.chunk_id_counter,
                device_type: device_type.clone(),
                channels: 1,
            };

            if let Err(e) = self.transcription_sender.send(transcription_chunk.clone()) {
                warn!("Failed to send merged segment: {}", e);
            } else {
                self.chunk_id_counter += 1;
                if let Some(ref embedding_sender) = self.embedding_sender {
                    // Live status: count the block in the diarization queue
                    // even if the send later fails.
                    crate::audio::online_diarization::record_block_enqueued();
                    if let Err(e) = embedding_sender.send(transcription_chunk) {
                        debug!("Failed to send segment to embedding channel: {}", e);
                    }
                }
            }
        }
        pending.clear();
    }

    /// Run the VAD-driven audio processing pipeline
    pub async fn run(mut self) -> Result<()> {
        info!("VAD-driven audio pipeline started - segments sent in real-time based on speech detection");

        // CRITICAL FIX: Continue processing until channel is closed, not based on recording state
        // This ensures ALL chunks are processed during shutdown, fixing premature meeting completion
        // Previous bug: Loop checked `while self.state.is_recording()` which caused early exit when
        // stop_recording() was called, losing flush signals and remaining chunks in the pipeline
        loop {
            // Receive audio chunks with timeout
            match tokio::time::timeout(
                std::time::Duration::from_millis(50), // Shorter timeout for responsiveness
                self.receiver.recv(),
            )
            .await
            {
                Ok(Some(chunk)) => {
                    // PERFORMANCE: Check for flush signal (special chunk with ID >= u64::MAX - 10)
                    // Multiple flush signals may be sent to ensure processing
                    if chunk.chunk_id >= u64::MAX - 10 {
                        info!(
                            "📥 Received FLUSH signal #{} - flushing VAD processor",
                            u64::MAX - chunk.chunk_id
                        );
                        self.flush_remaining_audio()?;
                        // Continue processing to handle any remaining chunks
                        continue;
                    }

                    // PERFORMANCE OPTIMIZATION: Eliminate per-chunk logging overhead
                    // Logging in hot paths causes severe performance degradation
                    self.processed_chunks += 1;

                    // Smart batching: collect metrics instead of logging every chunk
                    if let Some(ref batcher) = self.metrics_batcher {
                        let avg_level = chunk.data.iter().map(|&x| x.abs()).sum::<f32>()
                            / chunk.data.len() as f32;
                        let duration_ms =
                            chunk.data.len() as f64 / chunk.sample_rate as f64 * 1000.0;

                        batch_audio_metric!(
                            Some(batcher),
                            chunk.chunk_id,
                            chunk.data.len(),
                            duration_ms,
                            avg_level
                        );
                    }

                    // CRITICAL: Log summary only every 200 chunks OR every 60 seconds (99.5% reduction)
                    // This eliminates I/O overhead in the audio processing hot path
                    // Use performance-optimized debug macro that compiles to nothing in release builds
                    if self.processed_chunks % 200 == 0
                        || self.last_summary_time.elapsed().as_secs() >= 60
                    {
                        perf_debug!(
                            "Pipeline processed {} chunks, current chunk: {} ({} samples)",
                            self.processed_chunks,
                            chunk.chunk_id,
                            chunk.data.len()
                        );
                        self.last_summary_time = std::time::Instant::now();
                    }

                    // STEP 1: Accumulate mono audio into per-source VAD buffer
                    let (vad_buffer, real_base) = match chunk.device_type {
                        DeviceType::Microphone => {
                            (&mut self.vad_buffer_mic, &mut self.vad_mic_buffer_real_base)
                        }
                        DeviceType::System => {
                            (&mut self.vad_buffer_sys, &mut self.vad_sys_buffer_real_base)
                        }
                    };
                    if vad_buffer.is_empty() {
                        // The oldest sample of a fresh buffer anchors the source's
                        // VAD counter timeline to real recording time.
                        *real_base = Some(chunk.timestamp);
                    }
                    vad_buffer.extend_from_slice(&chunk.data);

                    // STEP 2: Dispatch to VAD only when buffer reaches threshold (window-batched)
                    let threshold = self.vad_dispatch_threshold_samples;
                    let should_dispatch = match chunk.device_type {
                        DeviceType::Microphone => self.vad_buffer_mic.len() >= threshold,
                        DeviceType::System => self.vad_buffer_sys.len() >= threshold,
                    };

                    if should_dispatch {
                        let (vad, buffer) = match chunk.device_type {
                            DeviceType::Microphone => {
                                (&mut self.vad_processor_mic, &mut self.vad_buffer_mic)
                            }
                            DeviceType::System => {
                                (&mut self.vad_processor_sys, &mut self.vad_buffer_sys)
                            }
                        };

                        // Take accumulated samples and clear buffer
                        let counter_base_ms = vad.processed_ms();
                        let accumulated: Vec<f32> = std::mem::take(buffer);

                        // Record the real-time anchor for this dispatch batch
                        {
                            let (base_ref, anchors) = match chunk.device_type {
                                DeviceType::Microphone => (
                                    &mut self.vad_mic_buffer_real_base,
                                    &mut self.vad_mic_anchors,
                                ),
                                DeviceType::System => (
                                    &mut self.vad_sys_buffer_real_base,
                                    &mut self.vad_sys_anchors,
                                ),
                            };
                            if let Some(real_base) = base_ref.take() {
                                anchors.push((counter_base_ms, real_base));
                            }
                        }

                        match vad.process_audio(&accumulated) {
                            Ok(mut speech_segments) => {
                                // Convert sample-domain times to recording-relative
                                // seconds so transcription stays aligned to the audio.
                                let anchors = match chunk.device_type {
                                    DeviceType::Microphone => &self.vad_mic_anchors,
                                    DeviceType::System => &self.vad_sys_anchors,
                                };
                                remap_segment_times_to_real(anchors, &mut speech_segments);

                                let pending_ref = match chunk.device_type {
                                    DeviceType::Microphone => &mut self.vad_pending_mic,
                                    DeviceType::System => &mut self.vad_pending_sys,
                                };

                                // Check if any new segment is distant from the accumulated tail
                                let should_flush = speech_segments
                                    .first()
                                    .and_then(|first| {
                                        pending_ref.last().map(|last| {
                                            (first.start_timestamp_ms - last.end_timestamp_ms)
                                                >= 500.0
                                        })
                                    })
                                    .unwrap_or(false);

                                if should_flush {
                                    drop(pending_ref); // release borrow before calling flush
                                    self.flush_pending_segments(chunk.device_type.clone());
                                }

                                // Re-borrow after flush to extend
                                let pending_ref = match chunk.device_type {
                                    DeviceType::Microphone => &mut self.vad_pending_mic,
                                    DeviceType::System => &mut self.vad_pending_sys,
                                };
                                pending_ref.extend(speech_segments);

                                // Flush if accumulated duration would exceed 25s when merged
                                let total_duration_ms: f64 = pending_ref
                                    .iter()
                                    .map(|s| s.end_timestamp_ms - s.start_timestamp_ms)
                                    .sum();
                                if total_duration_ms > 25_000.0 {
                                    drop(pending_ref);
                                    self.flush_pending_segments(chunk.device_type.clone());
                                }
                            }
                            Err(e) => {
                                warn!("⚠️ VAD error: {}", e);
                            }
                        }
                    }

                    // STEP 1.5: live input level for the status lines. Measured
                    // on the mono samples this channel is actually using, before
                    // they move into the recording ring buffer.
                    if let Some(telemetry) = self.telemetry.as_deref() {
                        telemetry
                            .channel(&chunk.device_type)
                            .set_level(&chunk.data);
                    }

                    // STEP 2: Add mono audio to ring buffer for recording (move, no clone)
                    self.ring_buffer
                        .add_samples(chunk.device_type.clone(), chunk.data);

                    // STEP 2.5: publish this channel's live telemetry (fills and
                    // voice-activity activity) for the status lines.
                    self.publish_channel_telemetry(&chunk.device_type);

                    // STEP 3: Interleave stereo from ring buffer for recording
                    while self.ring_buffer.can_mix() {
                        if let Some((mic_window, sys_window)) = self.ring_buffer.extract_window() {
                            if let Some(ref sender) = self.recording_sender_for_mixed {
                                let stereo = interleave_stereo(&mic_window, &sys_window);
                                let recording_chunk = AudioChunk {
                                    data: stereo,
                                    sample_rate: self.sample_rate,
                                    timestamp: chunk.timestamp,
                                    chunk_id: self.chunk_id_counter,
                                    device_type: DeviceType::Microphone,
                                    channels: 2,
                                };
                                if sender.send(recording_chunk).is_err() {
                                    // The saver channel is closed/unavailable.
                                    // Log and surface (throttled) instead of
                                    // silently discarding the recording chunk.
                                    warn!("Failed to deliver recording chunk to saver (channel closed/unavailable) - audio may be lost");
                                    if !self.recording_save_failure_reported {
                                        self.recording_save_failure_reported = true;
                                        self.state.report_error(AudioError::SaveUnavailable);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(None) => {
                    info!(
                        "Audio pipeline: sender closed after processing {} chunks",
                        self.processed_chunks
                    );
                    break;
                }
                Err(_) => {
                    // Timeout - just continue, VAD handles all segmentation
                    continue;
                }
            }
        }

        // Flush any remaining VAD segments
        self.flush_remaining_audio()?;

        info!("VAD-driven audio pipeline ended");
        Ok(())
    }

    fn flush_remaining_audio(&mut self) -> Result<()> {
        info!(
            "Flushing remaining audio from pipeline (processed {} chunks)",
            self.processed_chunks
        );

        // Flush remaining accumulated VAD buffers first
        for (vad, buffer, device_type) in [
            (
                &mut self.vad_processor_mic,
                &mut self.vad_buffer_mic,
                DeviceType::Microphone,
            ),
            (
                &mut self.vad_processor_sys,
                &mut self.vad_buffer_sys,
                DeviceType::System,
            ),
        ] {
            if !buffer.is_empty() {
                let counter_base_ms = vad.processed_ms();
                let accumulated: Vec<f32> = std::mem::take(buffer);
                info!(
                    "Flushing VAD buffer [{:?}]: {} samples",
                    device_type,
                    accumulated.len()
                );

                // Record the real-time anchor for this final dispatch batch
                {
                    let (base_ref, anchors) = match device_type {
                        DeviceType::Microphone => (
                            &mut self.vad_mic_buffer_real_base,
                            &mut self.vad_mic_anchors,
                        ),
                        DeviceType::System => (
                            &mut self.vad_sys_buffer_real_base,
                            &mut self.vad_sys_anchors,
                        ),
                    };
                    if let Some(real_base) = base_ref.take() {
                        anchors.push((counter_base_ms, real_base));
                    }
                }

                if let Ok(mut speech_segments) = vad.process_audio(&accumulated) {
                    if !speech_segments.is_empty() {
                        let anchors = match device_type {
                            DeviceType::Microphone => &self.vad_mic_anchors,
                            DeviceType::System => &self.vad_sys_anchors,
                        };
                        remap_segment_times_to_real(anchors, &mut speech_segments);
                        let pending = match device_type {
                            DeviceType::Microphone => &mut self.vad_pending_mic,
                            DeviceType::System => &mut self.vad_pending_sys,
                        };
                        pending.extend(speech_segments);
                    }
                }
            }
        }

        // Flush both VAD processors independently (forces end of any ongoing speech)
        for (vad, device_type) in [
            (&mut self.vad_processor_mic, DeviceType::Microphone),
            (&mut self.vad_processor_sys, DeviceType::System),
        ] {
            match vad.flush() {
                Ok(mut final_segments) => {
                    if !final_segments.is_empty() {
                        let anchors = match device_type {
                            DeviceType::Microphone => &self.vad_mic_anchors,
                            DeviceType::System => &self.vad_sys_anchors,
                        };
                        remap_segment_times_to_real(anchors, &mut final_segments);
                        let pending = match device_type {
                            DeviceType::Microphone => &mut self.vad_pending_mic,
                            DeviceType::System => &mut self.vad_pending_sys,
                        };
                        pending.extend(final_segments);
                    }
                }
                Err(e) => {
                    warn!("Failed to flush VAD processor [{:?}]: {}", device_type, e);
                }
            }
        }

        // Merge and dispatch all accumulated segments
        self.flush_pending_segments(DeviceType::Microphone);
        self.flush_pending_segments(DeviceType::System);

        Ok(())
    }
}

/// Simple audio pipeline manager
pub struct AudioPipelineManager {
    pipeline_handle: Option<JoinHandle<Result<()>>>,
    audio_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
    embedding_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
}

impl AudioPipelineManager {
    pub fn new() -> Self {
        Self {
            pipeline_handle: None,
            audio_sender: None,
            embedding_sender: None,
        }
    }

    /// Start the audio pipeline with device information for adaptive buffering
    pub fn start(
        &mut self,
        state: Arc<RecordingState>,
        transcription_sender: mpsc::UnboundedSender<AudioChunk>,
        embedding_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
        target_chunk_duration_ms: u32,
        sample_rate: u32,
        recording_sender: Option<mpsc::UnboundedSender<AudioChunk>>,
        mic_device_name: String,
        mic_device_kind: super::device_detection::InputDeviceKind,
        system_device_name: String,
        system_device_kind: super::device_detection::InputDeviceKind,
    ) -> Result<()> {
        // Log device information for adaptive buffering
        info!("🎙️ Starting pipeline with device info:");
        info!(
            "   Microphone: '{}' ({:?})",
            mic_device_name, mic_device_kind
        );
        info!(
            "   System Audio: '{}' ({:?})",
            system_device_name, system_device_kind
        );

        // Create audio processing channel
        let (audio_sender, audio_receiver) = mpsc::unbounded_channel::<AudioChunk>();

        // Set sender in state for audio captures to use
        state.set_audio_sender(audio_sender.clone());

        // Create and start pipeline with device information for adaptive mixing
        let mut pipeline = AudioPipeline::new(
            audio_receiver,
            transcription_sender,
            embedding_sender.clone(),
            state.clone(),
            target_chunk_duration_ms,
            sample_rate,
            mic_device_name,
            mic_device_kind,
            system_device_name,
            system_device_kind,
        );

        // CRITICAL FIX: Connect recording sender to receive pre-mixed audio
        // This ensures both mic AND system audio are captured in recordings
        pipeline.recording_sender_for_mixed = recording_sender;

        let handle = tokio::spawn(async move { pipeline.run().await });

        self.pipeline_handle = Some(handle);
        self.audio_sender = Some(audio_sender);
        self.embedding_sender = embedding_sender;

        info!("Audio pipeline manager started with mixed audio recording");
        Ok(())
    }

    /// Stop the audio pipeline
    pub async fn stop(&mut self) -> Result<()> {
        // Drop the sender to close the pipeline
        self.audio_sender = None;

        // Wait for pipeline to finish
        if let Some(handle) = self.pipeline_handle.take() {
            match handle.await {
                Ok(result) => result,
                Err(e) => {
                    error!("Pipeline task failed: {}", e);
                    Ok(())
                }
            }
        } else {
            Ok(())
        }
    }

    /// Drop the embedding channel so the online diarization consumer can finish
    pub fn clear_embedding_sender(&mut self) {
        self.embedding_sender = None;
    }

    /// Force immediate flush of accumulated audio and stop pipeline
    /// PERFORMANCE CRITICAL: Eliminates 30+ second shutdown delays
    pub async fn force_flush_and_stop(&mut self) -> Result<()> {
        info!("🚀 Force flushing pipeline - processing ALL accumulated audio immediately");

        // If we have a sender, send a special flush signal first
        if let Some(sender) = &self.audio_sender {
            // Create a special flush chunk to trigger immediate processing
            let flush_chunk = AudioChunk {
                data: vec![], // Empty data signals flush
                sample_rate: 16000,
                timestamp: 0.0,
                chunk_id: u64::MAX, // Special ID to indicate flush
                device_type: super::recording_state::DeviceType::Microphone,
                channels: 1,
            };

            if let Err(e) = sender.send(flush_chunk) {
                warn!("Failed to send flush signal: {}", e);
            } else {
                info!("📤 Sent flush signal to pipeline");

                // PERFORMANCE OPTIMIZATION: Reduced wait time from 50ms to 20ms
                // Pipeline should process flush signal very quickly
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

                // Send multiple flush signals to ensure the pipeline catches it
                // This aggressive approach eliminates shutdown delay issues
                for i in 0..3 {
                    let additional_flush = AudioChunk {
                        channels: 1,
                        data: vec![],
                        sample_rate: 16000,
                        timestamp: 0.0,
                        chunk_id: u64::MAX - (i as u64),
                        device_type: super::recording_state::DeviceType::Microphone,
                    };
                    let _ = sender.send(additional_flush);
                }

                info!("📤 Sent additional flush signals for reliability");
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        // Now stop normally
        self.stop().await
    }
}

impl Default for AudioPipelineManager {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remap_segment_times_to_real_constant_late_start() {
        // A stream that began delivering 730s after the recording started:
        // VAD counter 0 <-> real 730s, counter 200ms <-> real 750s (a gap).
        let anchors = vec![(0.0, 730.0), (200.0, 750.0)];
        let mut segments = vec![
            SpeechSegment {
                samples: vec![0.0; 1600],
                start_timestamp_ms: 300.0,
                end_timestamp_ms: 1400.0,
                confidence: 0.9,
            },
            // A segment that started in the previous anchor span.
            SpeechSegment {
                samples: vec![0.0; 800],
                start_timestamp_ms: 50.0,
                end_timestamp_ms: 900.0,
                confidence: 0.9,
            },
        ];
        remap_segment_times_to_real(&anchors, &mut segments);

        // 300ms of the second anchor batch: real = 750 + 0.1 = 750.1s
        assert!((segments[0].start_timestamp_ms - 750_100.0).abs() < 0.001);
        assert!((segments[0].end_timestamp_ms - 751_200.0).abs() < 0.001);
        // 50ms falls in the first anchor: real = 730 + 0.05 = 730.05s
        assert!((segments[1].start_timestamp_ms - 730_050.0).abs() < 0.001);
        assert!((segments[1].end_timestamp_ms - 730_900.0).abs() < 0.001);
    }

    #[test]
    fn test_remap_segment_times_to_real_no_anchors_is_noop() {
        let anchors: Vec<(f64, f64)> = Vec::new();
        let mut segments = vec![SpeechSegment {
            samples: vec![0.0; 1600],
            start_timestamp_ms: 1000.0,
            end_timestamp_ms: 2000.0,
            confidence: 0.9,
        }];
        remap_segment_times_to_real(&anchors, &mut segments);
        assert_eq!(segments[0].start_timestamp_ms, 1000.0);
        assert_eq!(segments[0].end_timestamp_ms, 2000.0);
    }

    #[test]
    fn test_remap_segment_times_to_real_clamps_negative() {
        // A pathological backward shift (anchor below the stream timeline) must
        // be clamped at 0, never negative.
        let anchors = vec![(0.0, -1000.0)];
        let mut segments = vec![SpeechSegment {
            samples: vec![0.0; 1600],
            start_timestamp_ms: 100.0,
            end_timestamp_ms: 500.0,
            confidence: 0.9,
        }];
        remap_segment_times_to_real(&anchors, &mut segments);
        assert_eq!(segments[0].start_timestamp_ms, 0.0);
        assert_eq!(segments[0].end_timestamp_ms, 0.0);
    }
}
