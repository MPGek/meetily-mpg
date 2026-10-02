use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;

use super::buffer_pool::AudioBufferPool;
use super::devices::AudioDevice;
use super::sync_ext::LockRecover;

/// Device type for audio chunks
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Microphone,
    System,
}

/// Audio chunk with metadata for processing
#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub chunk_id: u64,
    pub device_type: DeviceType,
    pub channels: u16,
}

/// Processed audio chunk (post-VAD) for recording
#[derive(Debug, Clone)]
pub struct ProcessedAudioChunk {
    pub data: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: f64,
    pub device_type: DeviceType,
    pub channels: u16,
}

/// Comprehensive error types for audio system
#[derive(Debug, Clone)]
pub enum AudioError {
    DeviceDisconnected,
    StreamFailed,
    ProcessingFailed,
    TranscriptionFailed,
    ChannelClosed,
    InitializationFailed,
    ConfigurationError,
    PermissionDenied,
    BufferOverflow,
    SampleRateUnsupported,
    /// Recording audio could not be delivered to the saver (channel closed or
    /// unavailable). Recoverable and never trips the automatic stop threshold:
    /// the loss is surfaced as a warning instead of killing the recording.
    SaveUnavailable,
}

impl AudioError {
    /// Check if error is recoverable (can attempt reconnection)
    pub fn is_recoverable(&self) -> bool {
        match self {
            // Device disconnect is now recoverable - we can attempt reconnection
            AudioError::DeviceDisconnected => true,
            AudioError::StreamFailed => true,
            AudioError::ProcessingFailed => true,
            AudioError::TranscriptionFailed => true,
            AudioError::ChannelClosed => false,
            AudioError::InitializationFailed => false,
            AudioError::ConfigurationError => false,
            AudioError::PermissionDenied => false,
            AudioError::BufferOverflow => true,
            AudioError::SampleRateUnsupported => false,
            AudioError::SaveUnavailable => true,
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> &'static str {
        match self {
            AudioError::DeviceDisconnected => "Audio device was disconnected",
            AudioError::StreamFailed => "Audio stream encountered an error",
            AudioError::ProcessingFailed => "Audio processing failed",
            AudioError::TranscriptionFailed => "Speech transcription failed",
            AudioError::ChannelClosed => "Audio channel was closed unexpectedly",
            AudioError::InitializationFailed => "Failed to initialize audio system",
            AudioError::ConfigurationError => "Audio configuration error",
            AudioError::PermissionDenied => "Microphone permission denied",
            AudioError::BufferOverflow => "Audio buffer overflow",
            AudioError::SampleRateUnsupported => "Audio sample rate not supported",
            AudioError::SaveUnavailable => "Recording audio could not be saved",
        }
    }
}

/// Recording statistics
#[derive(Debug, Default)]
pub struct RecordingStats {
    pub chunks_processed: u64,
    pub total_duration: f64,
    pub last_activity: Option<Instant>,
}

/// Unified state management for audio recording
pub struct RecordingState {
    // Core recording state
    is_recording: AtomicBool,
    is_paused: AtomicBool,

    // Audio devices
    microphone_device: Mutex<Option<Arc<AudioDevice>>>,
    system_device: Mutex<Option<Arc<AudioDevice>>>,

    // Audio pipeline. Bounded (see `set_audio_sender`): a stalled consumer
    // drops chunks instead of growing memory unboundedly.
    audio_sender: Mutex<Option<mpsc::Sender<AudioChunk>>>,

    // Memory optimization
    buffer_pool: AudioBufferPool,

    // Error handling
    error_count: AtomicU32,
    recoverable_error_count: AtomicU32,
    last_error: Mutex<Option<AudioError>>,
    error_callback: Mutex<Option<Box<dyn Fn(&AudioError) + Send + Sync>>>,

    // Statistics
    stats: Mutex<RecordingStats>,

    // Recording start time for accurate timestamps
    recording_start: Mutex<Option<Instant>>,
    // Pause time tracking
    pause_start: Mutex<Option<Instant>>,
    total_pause_duration: Mutex<std::time::Duration>,
}

impl RecordingState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            buffer_pool: AudioBufferPool::new(16, 48000), // Pool of 16 buffers with 48kHz samples capacity
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            error_callback: Mutex::new(None),
            stats: Mutex::new(RecordingStats::default()),
            recording_start: Mutex::new(None),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        })
    }

    // Recording control
    pub fn start_recording(&self) -> Result<()> {
        self.is_recording.store(true, Ordering::SeqCst);
        *self.recording_start.lock_or_recover() = Some(Instant::now());
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);
        *self.last_error.lock_or_recover() = None;
        Ok(())
    }

    pub fn stop_recording(&self) {
        self.is_recording.store(false, Ordering::SeqCst);
        self.is_paused.store(false, Ordering::SeqCst);
        // Clear pause tracking when stopping
        *self.pause_start.lock_or_recover() = None;
        // CRITICAL: Clear audio sender to close the pipeline channel
        // This ensures the pipeline loop exits properly after processing all chunks
        *self.audio_sender.lock_or_recover() = None;
        // CRITICAL: Clear device references to release microphone/speaker
        // Without this, Arc<AudioDevice> references persist and keep the mic active
        *self.microphone_device.lock_or_recover() = None;
        *self.system_device.lock_or_recover() = None;
        log::info!("Recording stopped, device references cleared");
    }

    pub fn pause_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot pause when not recording"));
        }
        if self.is_paused() {
            return Err(anyhow::anyhow!("Recording is already paused"));
        }

        self.is_paused.store(true, Ordering::SeqCst);
        *self.pause_start.lock_or_recover() = Some(Instant::now());
        log::info!("Recording paused");
        Ok(())
    }

    pub fn resume_recording(&self) -> Result<()> {
        if !self.is_recording() {
            return Err(anyhow::anyhow!("Cannot resume when not recording"));
        }
        if !self.is_paused() {
            return Err(anyhow::anyhow!("Recording is not paused"));
        }

        // Calculate pause duration and add to total
        if let Some(pause_start) = self.pause_start.lock_or_recover().take() {
            let pause_duration = pause_start.elapsed();
            *self.total_pause_duration.lock_or_recover() += pause_duration;
            log::info!(
                "Recording resumed after pause of {:.2}s",
                pause_duration.as_secs_f64()
            );
        }

        self.is_paused.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.is_recording() && !self.is_paused()
    }

    // Device management
    pub fn set_microphone_device(&self, device: Arc<AudioDevice>) {
        *self.microphone_device.lock_or_recover() = Some(device);
    }

    pub fn set_system_device(&self, device: Arc<AudioDevice>) {
        *self.system_device.lock_or_recover() = Some(device);
    }

    pub fn get_microphone_device(&self) -> Option<Arc<AudioDevice>> {
        self.microphone_device.lock_or_recover().clone()
    }

    pub fn get_system_device(&self) -> Option<Arc<AudioDevice>> {
        self.system_device.lock_or_recover().clone()
    }

    // Audio pipeline management
    pub fn set_audio_sender(&self, sender: mpsc::Sender<AudioChunk>) {
        *self.audio_sender.lock_or_recover() = Some(sender);
    }

    pub fn send_audio_chunk(&self, chunk: AudioChunk) -> Result<()> {
        // Don't send audio chunks when paused
        if self.is_paused() {
            return Ok(()); // Silently discard chunks while paused
        }

        if let Some(sender) = self.audio_sender.lock_or_recover().as_ref() {
            match sender.try_send(chunk) {
                Ok(()) => {
                    // Update statistics
                    let mut stats = self.stats.lock_or_recover();
                    stats.chunks_processed += 1;
                    stats.last_activity = Some(Instant::now());
                    Ok(())
                }
                Err(mpsc::error::TrySendError::Full(chunk)) => {
                    // Bounded channel at capacity: drop and log rather than
                    // grow memory unboundedly while a consumer is stalled
                    // (same accepted policy as a paused recording above).
                    log::warn!(
                        "audio_sender channel full (128 capacity); dropping chunk {}",
                        chunk.chunk_id
                    );
                    Ok(())
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    Err(anyhow::anyhow!("Failed to send audio chunk"))
                }
            }
        } else {
            // Return an error when no sender is available (pipeline not ready)
            Err(anyhow::anyhow!(
                "Audio pipeline not ready - no sender available"
            ))
        }
    }

    /// Send the mic discontinuity marker to the pipeline (mic-disconnect-
    /// recovery D7): an empty Microphone chunk with the reserved id
    /// `MIC_DISCONTINUITY_CHUNK_ID`. Unlike audio, it is sent while paused too,
    /// so a switch during a pause still closes the old mic's speech. Returns
    /// whether the pipeline received it.
    pub fn mark_mic_discontinuity(&self) -> bool {
        let marker = AudioChunk {
            data: Vec::new(),
            sample_rate: 48000,
            timestamp: self.get_recording_duration().unwrap_or(0.0),
            chunk_id: super::pipeline::MIC_DISCONTINUITY_CHUNK_ID,
            device_type: DeviceType::Microphone,
            channels: 1,
        };
        match self.audio_sender.lock_or_recover().as_ref() {
            Some(sender) => match sender.try_send(marker) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("[HOT_SWAP] mic discontinuity marker not delivered: {}", e);
                    false
                }
            },
            None => false,
        }
    }

    // Error handling
    pub fn set_error_callback<F>(&self, callback: F)
    where
        F: Fn(&AudioError) + Send + Sync + 'static,
    {
        *self.error_callback.lock_or_recover() = Some(Box::new(callback));
    }

    pub fn report_error(&self, error: AudioError) {
        let count = self.error_count.fetch_add(1, Ordering::SeqCst) + 1;

        // A stalled saver channel is surfaced but must never trip the automatic
        // stop-recording thresholds (recoverable_count / total count).
        let is_save_unavailable = matches!(error, AudioError::SaveUnavailable);

        // Track recoverable vs non-recoverable errors separately
        if error.is_recoverable() {
            let recoverable_count = self.recoverable_error_count.fetch_add(1, Ordering::SeqCst) + 1;
            log::warn!(
                "Recoverable audio error ({}): {:?}",
                recoverable_count,
                error
            );

            // Allow more recoverable errors before stopping
            if recoverable_count >= 10 && !is_save_unavailable {
                log::error!(
                    "Too many recoverable errors ({}), stopping recording",
                    recoverable_count
                );
                self.stop_recording();
            }
        } else {
            log::error!("Non-recoverable audio error: {:?}", error);
            // Stop immediately for non-recoverable errors
            self.stop_recording();
        }

        *self.last_error.lock_or_recover() = Some(error.clone());

        // Call error callback if set
        if let Some(callback) = self.error_callback.lock_or_recover().as_ref() {
            callback(&error);
        }

        // Fallback: stop recording after too many total errors
        if count >= 15 && !is_save_unavailable {
            log::error!(
                "Too many total audio errors ({}), stopping recording",
                count
            );
            self.stop_recording();
        }
    }

    pub fn get_error_count(&self) -> u32 {
        self.error_count.load(Ordering::SeqCst)
    }

    pub fn get_recoverable_error_count(&self) -> u32 {
        self.recoverable_error_count.load(Ordering::SeqCst)
    }

    pub fn get_last_error(&self) -> Option<AudioError> {
        self.last_error.lock_or_recover().clone()
    }

    pub fn has_fatal_error(&self) -> bool {
        if let Some(error) = &*self.last_error.lock_or_recover() {
            !error.is_recoverable() && self.error_count.load(Ordering::SeqCst) > 0
        } else {
            false
        }
    }

    // Statistics
    pub fn get_stats(&self) -> RecordingStats {
        self.stats.lock_or_recover().clone()
    }

    pub fn get_recording_duration(&self) -> Option<f64> {
        self.recording_start
            .lock()
            .unwrap()
            .map(|start| start.elapsed().as_secs_f64())
    }

    pub fn get_active_recording_duration(&self) -> Option<f64> {
        self.recording_start.lock_or_recover().map(|start| {
            let total_duration = start.elapsed().as_secs_f64();
            let pause_duration = self.get_total_pause_duration();
            let current_pause = if self.is_paused() {
                self.pause_start
                    .lock()
                    .unwrap()
                    .map(|p| p.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
            } else {
                0.0
            };
            total_duration - pause_duration - current_pause
        })
    }

    pub fn get_total_pause_duration(&self) -> f64 {
        self.total_pause_duration.lock_or_recover().as_secs_f64()
    }

    pub fn get_current_pause_duration(&self) -> Option<f64> {
        if self.is_paused() {
            self.pause_start
                .lock()
                .unwrap()
                .map(|start| start.elapsed().as_secs_f64())
        } else {
            None
        }
    }

    // Memory management
    pub fn get_buffer_pool(&self) -> AudioBufferPool {
        self.buffer_pool.clone()
    }

    // Cleanup
    pub fn cleanup(&self) {
        self.stop_recording();
        *self.microphone_device.lock_or_recover() = None;
        *self.system_device.lock_or_recover() = None;
        *self.audio_sender.lock_or_recover() = None;
        *self.last_error.lock_or_recover() = None;
        *self.error_callback.lock_or_recover() = None;
        *self.stats.lock_or_recover() = RecordingStats::default();
        *self.recording_start.lock_or_recover() = None;
        *self.pause_start.lock_or_recover() = None;
        *self.total_pause_duration.lock_or_recover() = std::time::Duration::ZERO;
        self.error_count.store(0, Ordering::SeqCst);
        self.recoverable_error_count.store(0, Ordering::SeqCst);

        // Clear buffer pool to free memory
        self.buffer_pool.clear();
    }
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            is_recording: AtomicBool::new(false),
            is_paused: AtomicBool::new(false),
            microphone_device: Mutex::new(None),
            system_device: Mutex::new(None),
            audio_sender: Mutex::new(None),
            buffer_pool: AudioBufferPool::new(16, 48000), // Pool of 16 buffers with 48kHz samples capacity
            error_count: AtomicU32::new(0),
            recoverable_error_count: AtomicU32::new(0),
            last_error: Mutex::new(None),
            error_callback: Mutex::new(None),
            stats: Mutex::new(RecordingStats::default()),
            recording_start: Mutex::new(None),
            pause_start: Mutex::new(None),
            total_pause_duration: Mutex::new(std::time::Duration::ZERO),
        }
    }
}

// Thread-safe cloning for RecordingStats
impl Clone for RecordingStats {
    fn clone(&self) -> Self {
        Self {
            chunks_processed: self.chunks_processed,
            total_duration: self.total_duration,
            last_activity: self.last_activity,
        }
    }
}

#[cfg(test)]
mod bounded_channel_tests {
    use super::*;

    fn sample_chunk(chunk_id: u64) -> AudioChunk {
        AudioChunk {
            data: vec![],
            sample_rate: 16000,
            timestamp: 0.0,
            chunk_id,
            device_type: DeviceType::Microphone,
            channels: 1,
        }
    }

    /// Proves the drop-and-log policy's capacity-1 case documented in the
    /// design (audio-carrying channels): a full channel rejects a further
    /// `try_send` with `Full` rather than blocking or growing, and draining
    /// one item frees capacity for the next `try_send` to succeed. Uses
    /// `AudioChunk` (the element type of all four bounded audio channels
    /// this change introduces) with a minimal capacity-1 fixture rather than
    /// one specific production channel.
    #[tokio::test]
    async fn bounded_channel_rejects_when_full_and_accepts_again_after_drain() {
        let (sender, mut receiver) = mpsc::channel::<AudioChunk>(1);

        sender.try_send(sample_chunk(1)).expect("first send fills capacity 1");

        match sender.try_send(sample_chunk(2)) {
            Err(mpsc::error::TrySendError::Full(chunk)) => assert_eq!(chunk.chunk_id, 2),
            other => panic!("expected Full, got {:?}", other),
        }

        let received = receiver.recv().await.expect("channel not closed");
        assert_eq!(received.chunk_id, 1);

        sender
            .try_send(sample_chunk(3))
            .expect("capacity freed after drain");
    }

    #[tokio::test]
    async fn mic_discontinuity_marker_is_sent_while_paused_but_audio_is_not() {
        let state = RecordingState::new();
        let (sender, mut receiver) = mpsc::channel::<AudioChunk>(4);
        state.set_audio_sender(sender);
        state.start_recording().unwrap();
        state.pause_recording().unwrap();

        state.send_audio_chunk(sample_chunk(1)).unwrap();
        assert!(state.mark_mic_discontinuity());

        let received = receiver.recv().await.expect("marker delivered");
        assert_eq!(received.chunk_id, super::super::pipeline::MIC_DISCONTINUITY_CHUNK_ID);
        assert_eq!(received.device_type, DeviceType::Microphone);
        assert!(received.data.is_empty());
        assert!(
            receiver.try_recv().is_err(),
            "the audio chunk sent while paused must be dropped"
        );
    }

    #[test]
    fn mic_discontinuity_marker_reports_no_pipeline() {
        let state = RecordingState::new();
        assert!(!state.mark_mic_discontinuity());
    }
}
