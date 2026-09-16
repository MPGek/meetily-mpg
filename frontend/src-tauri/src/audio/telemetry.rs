// audio/telemetry.rs
//
// Live recording telemetry (change: online-diarization-telemetry).
//
// Two things are published here by the subsystems that already own them, and
// read back by the read-only status command:
//   - the fills of buffers that gate a pipeline operation, together with the
//     thresholds that fire them, so "how close is this to firing" is
//     answerable instead of showing a bare count;
//   - the activity of every model kind in use (voice activity, speech
//     recognition, word alignment, diarization).
//
// Publishers write atomics, or register an Arc they already hold. Nothing here
// emits per-chunk events.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use super::recording_state::DeviceType;
use super::word_alignment::queue::AlignmentQueue;

/// Bundled voice-activity model identity.
pub const VAD_MODEL_IDENTITY: &str = "silero_vad_v6";

/// Voice-activity window length in milliseconds (512 samples at 16 kHz).
const VAD_FRAME_MS: f64 = 32.0;

// ---------------------------------------------------------------------------
// Gated buffers
// ---------------------------------------------------------------------------

/// A buffer that must fill before an operation fires.
#[derive(Debug, Default)]
pub struct GatedBuffer {
    fill: AtomicU64,
    threshold: AtomicU64,
}

/// A buffer's fill relative to the threshold that fires the operation it gates.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct BufferFill {
    pub fill: u64,
    pub threshold: u64,
    /// `fill / threshold`; at or above 1.0 once the operation has fired.
    pub fraction: f32,
    pub fired: bool,
}

impl GatedBuffer {
    pub fn set_threshold(&self, threshold: u64) {
        self.threshold.store(threshold, Ordering::Relaxed);
    }

    pub fn set_fill(&self, fill: u64) {
        self.fill.store(fill, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> BufferFill {
        let fill = self.fill.load(Ordering::Relaxed);
        let threshold = self.threshold.load(Ordering::Relaxed);
        BufferFill {
            fill,
            threshold,
            fraction: if threshold == 0 {
                0.0
            } else {
                fill as f32 / threshold as f32
            },
            fired: threshold > 0 && fill >= threshold,
        }
    }
}

/// Merged speech waiting to be sent for recognition.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct PendingState {
    pub segments: u64,
    pub buffered_ms: u64,
    /// Silence gap to the pending tail that triggers the flush.
    pub gap_trigger_ms: u64,
    /// Accumulated-duration cap that triggers the flush.
    pub cap_trigger_ms: u64,
}

/// Level below which a channel is treated as silent for the level meter.
const LEVEL_FLOOR_DB: f32 = -60.0;

/// A channel's processed-audio level, with the age of the sample it came from.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct AudioLevel {
    /// RMS of the last processed chunk (linear amplitude).
    pub rms: f32,
    /// Peak absolute amplitude of the last processed chunk.
    pub peak: f32,
    /// Milliseconds since that chunk was measured.
    pub age_ms: u64,
}

/// The level measured from one chunk of mono audio: `(rms, peak)`.
pub fn measure_level(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_squares = 0.0f64;
    let mut peak = 0.0f32;
    for sample in samples {
        let magnitude = sample.abs();
        if magnitude > peak {
            peak = magnitude;
        }
        sum_squares += (*sample as f64) * (*sample as f64);
    }
    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    (rms, peak)
}

/// Map a linear RMS amplitude onto a 0..1 position on a decibel meter.
pub fn level_fraction(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    if db <= LEVEL_FLOOR_DB {
        return 0.0;
    }
    ((db - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB).clamp(0.0, 1.0)
}

/// Milliseconds since the Unix epoch (level freshness, not a wall clock).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// One channel's buffer fills, voice-activity counters and input level.
#[derive(Debug, Default)]
pub struct ChannelPipelineStats {
    /// Samples queued for the voice-activity dispatcher.
    pub vad_dispatch: GatedBuffer,
    /// Voice-activity frames evaluated.
    vad_frames: AtomicU64,
    /// Whether the detector is currently inside speech on this channel.
    vad_speaking: AtomicBool,
    pending_segments: AtomicU64,
    pending_ms: AtomicU64,
    pending_gap_ms: AtomicU64,
    pending_cap_ms: AtomicU64,
    /// Samples queued for the recording mix window.
    pub mix: GatedBuffer,
    /// Linear RMS of the last processed chunk (f32 bits).
    level_rms: std::sync::atomic::AtomicU32,
    /// Peak amplitude of the last processed chunk (f32 bits).
    level_peak: std::sync::atomic::AtomicU32,
    /// When that chunk was measured (epoch ms).
    level_at_ms: AtomicU64,
}

/// One channel's published buffer fills, voice-activity activity and level.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ChannelPipelineFill {
    pub vad_dispatch: BufferFill,
    pub vad_frames: u64,
    pub vad_speaking: bool,
    pub pending: PendingState,
    pub mix: BufferFill,
    /// Input level, with how old it is.
    pub level: AudioLevel,
}

impl ChannelPipelineStats {
    pub fn set_vad_frames(&self, frames: u64) {
        self.vad_frames.store(frames, Ordering::Relaxed);
    }

    pub fn set_vad_speaking(&self, speaking: bool) {
        self.vad_speaking.store(speaking, Ordering::Relaxed);
    }

    pub fn set_pending(&self, segments: u64, buffered_ms: u64) {
        self.pending_segments.store(segments, Ordering::Relaxed);
        self.pending_ms.store(buffered_ms, Ordering::Relaxed);
    }

    pub fn set_pending_triggers(&self, gap_ms: u64, cap_ms: u64) {
        self.pending_gap_ms.store(gap_ms, Ordering::Relaxed);
        self.pending_cap_ms.store(cap_ms, Ordering::Relaxed);
    }

    /// Record the level of one processed chunk. Cheap: a single pass over the
    /// samples the pipeline already holds.
    pub fn set_level(&self, samples: &[f32]) {
        let (rms, peak) = measure_level(samples);
        self.level_rms.store(rms.to_bits(), Ordering::Relaxed);
        self.level_peak.store(peak.to_bits(), Ordering::Relaxed);
        self.level_at_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// The last measured level, with its age. A channel that has never carried
    /// audio reports an empty level rather than a stale one.
    pub fn level(&self) -> AudioLevel {
        let measured_at = self.level_at_ms.load(Ordering::Relaxed);
        if measured_at == 0 {
            return AudioLevel::default();
        }
        AudioLevel {
            rms: f32::from_bits(self.level_rms.load(Ordering::Relaxed)),
            peak: f32::from_bits(self.level_peak.load(Ordering::Relaxed)),
            age_ms: now_ms().saturating_sub(measured_at),
        }
    }

    pub fn snapshot(&self) -> ChannelPipelineFill {
        ChannelPipelineFill {
            vad_dispatch: self.vad_dispatch.snapshot(),
            vad_frames: self.vad_frames.load(Ordering::Relaxed),
            vad_speaking: self.vad_speaking.load(Ordering::Relaxed),
            pending: PendingState {
                segments: self.pending_segments.load(Ordering::Relaxed),
                buffered_ms: self.pending_ms.load(Ordering::Relaxed),
                gap_trigger_ms: self.pending_gap_ms.load(Ordering::Relaxed),
                cap_trigger_ms: self.pending_cap_ms.load(Ordering::Relaxed),
            },
            mix: self.mix.snapshot(),
            level: self.level(),
        }
    }
}

/// Pipeline telemetry for one recording session.
pub struct PipelineTelemetry {
    sample_rate: u32,
    mic: ChannelPipelineStats,
    sys: ChannelPipelineStats,
}

impl PipelineTelemetry {
    fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            mic: ChannelPipelineStats::default(),
            sys: ChannelPipelineStats::default(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channel(&self, device: &DeviceType) -> &ChannelPipelineStats {
        match device {
            DeviceType::Microphone => &self.mic,
            DeviceType::System => &self.sys,
        }
    }

    /// Voice-activity frames for a processed duration, in the 16 kHz domain.
    pub fn frames_from_processed_ms(processed_ms: f64) -> u64 {
        (processed_ms / VAD_FRAME_MS).max(0.0) as u64
    }

    pub fn snapshots(&self) -> (ChannelPipelineFill, ChannelPipelineFill) {
        (self.mic.snapshot(), self.sys.snapshot())
    }
}

static PIPELINE: Mutex<Option<Arc<PipelineTelemetry>>> = Mutex::new(None);

/// Install pipeline telemetry for a new session (the pipeline owns this).
pub fn install_pipeline(sample_rate: u32) -> Arc<PipelineTelemetry> {
    let telemetry = Arc::new(PipelineTelemetry::new(sample_rate));
    if let Ok(mut slot) = PIPELINE.lock() {
        *slot = Some(telemetry.clone());
    }
    telemetry
}

pub fn pipeline() -> Option<Arc<PipelineTelemetry>> {
    PIPELINE.lock().ok().and_then(|slot| slot.clone())
}

pub fn clear_pipeline() {
    if let Ok(mut slot) = PIPELINE.lock() {
        *slot = None;
    }
}

// ---------------------------------------------------------------------------
// Speech recognition activity
// ---------------------------------------------------------------------------

static ASR_QUEUED: Mutex<Option<Arc<AtomicU64>>> = Mutex::new(None);
static ASR_COMPLETED: Mutex<Option<Arc<AtomicU64>>> = Mutex::new(None);
static ASR_ENGINE: Mutex<Option<String>> = Mutex::new(None);
static ASR_MODEL: Mutex<Option<String>> = Mutex::new(None);
static ASR_LAST_TEXT: Mutex<Option<String>> = Mutex::new(None);

/// Longest fragment of the most recent recognition kept for the status block.
const ASR_LAST_TEXT_CHARS: usize = 48;

/// Speech-recognition activity for the status block.
#[derive(Debug, Clone, Serialize)]
pub struct AsrActivity {
    /// Engine kind (`Whisper`, `Parakeet`, provider name); None when idle.
    pub engine: Option<String>,
    /// Model identity the engine loaded; None when nothing is loaded.
    pub model: Option<String>,
    pub loaded: bool,
    pub queued: u64,
    pub completed: u64,
    /// Segments queued but not yet recognised.
    pub pending: u64,
    /// Most recent recognised text, truncated.
    pub last_text: Option<String>,
}

/// Register the transcription worker's counters and engine identity. Called by
/// the worker, which owns them.
pub fn install_asr(
    queued: Arc<AtomicU64>,
    completed: Arc<AtomicU64>,
    engine: Option<String>,
    model: Option<String>,
) {
    if let Ok(mut slot) = ASR_QUEUED.lock() {
        *slot = Some(queued);
    }
    if let Ok(mut slot) = ASR_COMPLETED.lock() {
        *slot = Some(completed);
    }
    if let Ok(mut slot) = ASR_ENGINE.lock() {
        *slot = engine;
    }
    if let Ok(mut slot) = ASR_MODEL.lock() {
        *slot = model;
    }
}

/// Record the most recent recognition so the status block can show that the
/// recogniser is producing output. Cheap: one short string, not per chunk.
pub fn note_asr_result(text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(mut slot) = ASR_LAST_TEXT.lock() {
        *slot = Some(trimmed.chars().take(ASR_LAST_TEXT_CHARS).collect());
    }
}

pub fn asr_activity() -> AsrActivity {
    let counter = |slot: &Mutex<Option<Arc<AtomicU64>>>| {
        slot.lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|c| c.load(Ordering::Relaxed)))
            .unwrap_or(0)
    };
    let queued = counter(&ASR_QUEUED);
    let completed = counter(&ASR_COMPLETED);
    let engine = ASR_ENGINE.lock().ok().and_then(|slot| slot.clone());
    let model = ASR_MODEL.lock().ok().and_then(|slot| slot.clone());
    let last_text = ASR_LAST_TEXT.lock().ok().and_then(|slot| slot.clone());

    AsrActivity {
        loaded: model.is_some(),
        engine,
        model,
        queued,
        completed,
        pending: queued.saturating_sub(completed),
        last_text,
    }
}

pub fn clear_asr() {
    if let Ok(mut slot) = ASR_QUEUED.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = ASR_COMPLETED.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = ASR_ENGINE.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = ASR_MODEL.lock() {
        *slot = None;
    }
    if let Ok(mut slot) = ASR_LAST_TEXT.lock() {
        *slot = None;
    }
}

// ---------------------------------------------------------------------------
// Word alignment activity
// ---------------------------------------------------------------------------

static ALIGNMENT_QUEUE: Mutex<Option<Arc<AlignmentQueue>>> = Mutex::new(None);
static ALIGNMENT_ENGINE_LOADED: AtomicBool = AtomicBool::new(false);
static ALIGNMENT_REFINED: AtomicU64 = AtomicU64::new(0);

/// Word-alignment activity for the status block.
#[derive(Debug, Clone, Serialize)]
pub struct AlignmentActivity {
    /// Whether word-level alignment is enabled in settings.
    pub enabled: bool,
    pub model_id: Option<String>,
    /// Whether the alignment engine has been loaded off disk.
    pub loaded: bool,
    pub queued_jobs: usize,
    pub queue_bytes: usize,
    pub dropped: usize,
    pub refined: u64,
}

/// Register the alignment queue, owned by the transcription worker.
pub fn install_alignment_queue(queue: Arc<AlignmentQueue>) {
    if let Ok(mut slot) = ALIGNMENT_QUEUE.lock() {
        *slot = Some(queue);
    }
}

pub fn set_alignment_engine_loaded(loaded: bool) {
    ALIGNMENT_ENGINE_LOADED.store(loaded, Ordering::Relaxed);
}

pub fn note_alignment_refined() {
    ALIGNMENT_REFINED.fetch_add(1, Ordering::Relaxed);
}

pub fn alignment_activity() -> AlignmentActivity {
    let settings = super::word_alignment::settings::current();
    let queue = ALIGNMENT_QUEUE.lock().ok().and_then(|slot| slot.clone());
    let (queued_jobs, queue_bytes, dropped) = match queue.as_ref() {
        Some(queue) => (queue.len(), queue.bytes(), queue.dropped()),
        None => (0, 0, 0),
    };

    AlignmentActivity {
        enabled: settings.enabled,
        model_id: settings.model_id,
        loaded: ALIGNMENT_ENGINE_LOADED.load(Ordering::Relaxed),
        queued_jobs,
        queue_bytes,
        dropped,
        refined: ALIGNMENT_REFINED.load(Ordering::Relaxed),
    }
}

pub fn clear_alignment() {
    if let Ok(mut slot) = ALIGNMENT_QUEUE.lock() {
        *slot = None;
    }
    ALIGNMENT_ENGINE_LOADED.store(false, Ordering::Relaxed);
    ALIGNMENT_REFINED.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Voice-activity activity
// ---------------------------------------------------------------------------

/// Voice-activity activity for the status block.
#[derive(Debug, Clone, Serialize)]
pub struct VadActivity {
    pub identity: String,
    pub loaded: bool,
    pub mic_frames: u64,
    pub mic_speaking: bool,
    pub sys_frames: u64,
    pub sys_speaking: bool,
}

pub fn vad_activity() -> VadActivity {
    match pipeline() {
        Some(telemetry) => {
            let (mic, sys) = telemetry.snapshots();
            VadActivity {
                identity: VAD_MODEL_IDENTITY.to_string(),
                // The detector sessions live with the pipeline, so a running
                // pipeline means the model is loaded.
                loaded: true,
                mic_frames: mic.vad_frames,
                mic_speaking: mic.vad_speaking,
                sys_frames: sys.vad_frames,
                sys_speaking: sys.vad_speaking,
            }
        }
        None => VadActivity {
            identity: VAD_MODEL_IDENTITY.to_string(),
            loaded: false,
            mic_frames: 0,
            mic_speaking: false,
            sys_frames: 0,
            sys_speaking: false,
        },
    }
}

/// Serializes tests that read or write the process-global telemetry. Shared by
/// the telemetry tests, the online diarization stats tests, and the status
/// command test.
#[cfg(test)]
pub(crate) static TELEMETRY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gated_buffer_reports_fraction_and_fired() {
        let buffer = GatedBuffer::default();
        buffer.set_threshold(9600);

        buffer.set_fill(0);
        let empty = buffer.snapshot();
        assert_eq!(empty.fraction, 0.0);
        assert!(!empty.fired);

        buffer.set_fill(4800);
        let half = buffer.snapshot();
        assert!((half.fraction - 0.5).abs() < 0.001);
        assert!(!half.fired);

        buffer.set_fill(9600);
        assert!(buffer.snapshot().fired);

        // Past the threshold the fraction keeps growing (not clamped): the
        // operation already fired.
        buffer.set_fill(19200);
        let over = buffer.snapshot();
        assert!(over.fraction > 1.0);
        assert!(over.fired);
    }

    #[test]
    fn gated_buffer_without_threshold_reports_no_fraction() {
        let buffer = GatedBuffer::default();
        buffer.set_fill(100);
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.fraction, 0.0);
        assert!(!snapshot.fired);
    }

    #[test]
    fn gated_fills_are_tracked_per_channel() {
        let telemetry = PipelineTelemetry::new(48000);
        let mic = telemetry.channel(&DeviceType::Microphone);
        mic.vad_dispatch.set_threshold(9600);
        mic.vad_dispatch.set_fill(9600);
        mic.mix.set_threshold(28800);

        let (mic_fill, sys_fill) = telemetry.snapshots();
        assert!(mic_fill.vad_dispatch.fired);
        assert_eq!(mic_fill.mix.threshold, 28800);

        assert_eq!(sys_fill.vad_dispatch.fill, 0);
        assert!(!sys_fill.vad_dispatch.fired);
        assert_eq!(sys_fill.mix.threshold, 0);
    }

    #[test]
    fn pending_reports_both_flush_triggers_and_its_fill() {
        let telemetry = PipelineTelemetry::new(48000);
        let mic = telemetry.channel(&DeviceType::Microphone);
        mic.set_pending_triggers(500, 25_000);
        mic.set_pending(2, 3400);

        let (mic_fill, _) = telemetry.snapshots();
        assert_eq!(mic_fill.pending.segments, 2);
        assert_eq!(mic_fill.pending.buffered_ms, 3400);
        assert_eq!(mic_fill.pending.gap_trigger_ms, 500);
        assert_eq!(mic_fill.pending.cap_trigger_ms, 25_000);

        // At the cap the flush fires.
        mic.set_pending(1, 25_000);
        let (at_cap, _) = telemetry.snapshots();
        assert!(at_cap.pending.buffered_ms >= at_cap.pending.cap_trigger_ms);
    }

    #[test]
    fn level_measures_rms_and_peak_of_a_chunk() {
        let (rms, peak) = measure_level(&[0.5, -0.5, 0.5, -0.5]);
        assert!((rms - 0.5).abs() < 0.001);
        assert!((peak - 0.5).abs() < 0.001);

        let (silent_rms, silent_peak) = measure_level(&[0.0; 128]);
        assert_eq!(silent_rms, 0.0);
        assert_eq!(silent_peak, 0.0);

        assert_eq!(measure_level(&[]), (0.0, 0.0));
    }

    #[test]
    fn level_fraction_is_a_decibel_scale() {
        // Full scale is the top of the meter.
        assert!((level_fraction(1.0) - 1.0).abs() < 0.001);
        // Practical silence and below map to empty, not to a small bar.
        assert_eq!(level_fraction(0.0), 0.0);
        assert_eq!(level_fraction(-0.1), 0.0);
        assert_eq!(level_fraction(0.0005), 0.0);
        // Quiet speech still moves the meter (the point of the dB mapping).
        assert!(level_fraction(0.01) > 0.3);
        assert!(level_fraction(0.01) < level_fraction(0.1));
        assert_eq!(level_fraction(f32::NAN), 0.0);
    }

    #[test]
    fn levels_are_tracked_per_channel_and_age_without_audio() {
        let telemetry = PipelineTelemetry::new(48000);
        let mic = telemetry.channel(&DeviceType::Microphone);
        mic.set_level(&[0.5; 128]);

        let (mic_fill, sys_fill) = telemetry.snapshots();
        assert!(mic_fill.level.rms > 0.0);
        assert!(mic_fill.level.age_ms < 1_000);
        // The silent channel reports an empty level, not the other channel's.
        assert_eq!(sys_fill.level.rms, 0.0);
        assert_eq!(sys_fill.level.peak, 0.0);
    }

    #[test]
    fn vad_frames_come_from_processed_ms() {
        assert_eq!(PipelineTelemetry::frames_from_processed_ms(0.0), 0);
        assert_eq!(PipelineTelemetry::frames_from_processed_ms(32.0), 1);
        assert_eq!(PipelineTelemetry::frames_from_processed_ms(320.0), 10);
        assert_eq!(PipelineTelemetry::frames_from_processed_ms(-1.0), 0);
    }

    #[test]
    fn vad_activity_is_unloaded_without_a_pipeline() {
        let _guard = TELEMETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_pipeline();

        let activity = vad_activity();
        assert_eq!(activity.identity, VAD_MODEL_IDENTITY);
        assert!(!activity.loaded);
        assert_eq!(activity.mic_frames, 0);
    }

    #[test]
    fn vad_activity_distinguishes_speaking_from_silent_channel() {
        let _guard = TELEMETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let telemetry = install_pipeline(48000);
        telemetry.channel(&DeviceType::Microphone).set_vad_frames(40);
        telemetry
            .channel(&DeviceType::Microphone)
            .set_vad_speaking(true);
        telemetry.channel(&DeviceType::System).set_vad_frames(12);
        telemetry
            .channel(&DeviceType::System)
            .set_vad_speaking(false);

        let activity = vad_activity();
        assert!(activity.loaded);
        assert_eq!(activity.mic_frames, 40);
        assert!(activity.mic_speaking);
        assert_eq!(activity.sys_frames, 12);
        assert!(!activity.sys_speaking);

        clear_pipeline();
    }

    #[test]
    fn asr_activity_is_unloaded_and_empty_without_a_worker() {
        let _guard = TELEMETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_asr();

        let activity = asr_activity();
        assert!(!activity.loaded);
        assert_eq!(activity.engine, None);
        assert_eq!(activity.pending, 0);

        // A registered engine reports its queue depth as pending work.
        let queued = Arc::new(AtomicU64::new(7));
        let completed = Arc::new(AtomicU64::new(3));
        install_asr(
            queued,
            completed,
            Some("Whisper".to_string()),
            Some("large-v3-turbo".to_string()),
        );

        let running = asr_activity();
        assert!(running.loaded);
        assert_eq!(running.engine.as_deref(), Some("Whisper"));
        assert_eq!(running.model.as_deref(), Some("large-v3-turbo"));
        assert_eq!(running.queued, 7);
        assert_eq!(running.completed, 3);
        assert_eq!(running.pending, 4);

        // An engine whose model is not loaded is reported as not loaded, and
        // the most recent recognition is truncated to a short fragment.
        note_asr_result("hello world, this is a deliberately long recognised sentence");
        let latest = asr_activity();
        let text = latest.last_text.expect("last text recorded");
        assert_eq!(text.chars().count(), ASR_LAST_TEXT_CHARS);

        note_asr_result("   ");
        assert_eq!(
            asr_activity().last_text.expect("kept previous text"),
            text
        );

        install_asr(
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
            Some("Whisper".to_string()),
            None,
        );
        assert!(!asr_activity().loaded);

        clear_asr();
    }

    #[test]
    fn alignment_activity_reports_disabled_and_unloaded_distinctly() {
        let _guard = TELEMETRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_alignment();
        super::super::word_alignment::settings::set_settings(true, None);

        // Enabled but no engine loaded and nothing queued yet: not a failure.
        let idle = alignment_activity();
        assert!(idle.enabled);
        assert!(!idle.loaded);
        assert_eq!(idle.queued_jobs, 0);
        assert_eq!(idle.refined, 0);

        // Disabled by settings: reported as disabled, still not a failure.
        super::super::word_alignment::settings::set_settings(false, None);
        let disabled = alignment_activity();
        assert!(!disabled.enabled);

        // Refinements are counted as activity.
        super::super::word_alignment::settings::set_settings(true, None);
        set_alignment_engine_loaded(true);
        note_alignment_refined();
        note_alignment_refined();
        let working = alignment_activity();
        assert!(working.loaded);
        assert_eq!(working.refined, 2);

        clear_alignment();
        let cleared = alignment_activity();
        assert!(!cleared.loaded);
        assert_eq!(cleared.refined, 0);
    }
}
