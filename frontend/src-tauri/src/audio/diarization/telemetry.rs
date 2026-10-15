//! Diarization telemetry: progress events to the frontend and the peak-memory
//! sampler used to report a run's footprint.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Runtime};

use super::batch::pcm::join_within;
use super::streaming::reconcile::LiveTurnRegistry;
use super::DiarizationProgress;
use crate::audio::recording_state::DeviceType;

pub(crate) fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    status: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

// ===== Memory sampler =====

pub(crate) struct MemorySampler {
    peak_bytes: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        // A generous fixed timeout: `Drop` is a safety net, not the primary
        // stop path (see `finish`), so it still waits for the sampler thread
        // rather than leaking it, but bounded instead of an untimed join.
        self.stop(Duration::from_secs(1));
    }
}

impl MemorySampler {
    pub(crate) fn start() -> Self {
        let peak_bytes = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let peak_bytes_clone = peak_bytes.clone();
        let running_clone = running.clone();

        let handle = std::thread::spawn(move || {
            let pid = match sysinfo::get_current_pid() {
                Ok(pid) => pid,
                Err(_) => return,
            };
            let mut system = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new().with_memory()),
            );
            let refresh_kind = ProcessRefreshKind::new().with_memory();
            while running_clone.load(Ordering::Relaxed) {
                let _ = system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    false,
                    refresh_kind,
                );
                if let Some(proc) = system.process(pid) {
                    let mem = proc.memory();
                    let mut current = peak_bytes_clone.load(Ordering::Relaxed);
                    while mem > current {
                        match peak_bytes_clone.compare_exchange_weak(
                            current,
                            mem,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(c) => current = c,
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            peak_bytes,
            running,
            handle: Some(handle),
        }
    }

    /// Signal the sampler thread to stop and wait up to `timeout` for it to
    /// exit. Returns whether it joined within the timeout.
    fn stop(&mut self, timeout: Duration) -> bool {
        self.running.store(false, Ordering::Relaxed);
        join_within(&mut self.handle, timeout)
    }

    /// Stop the sampler (bounded by a generous fixed timeout) and return the
    /// peak resident memory observed, in MiB. The primary, explicit stop path
    /// (`Drop` remains a safety net for the case this is not called).
    pub(crate) fn finish(mut self) -> u64 {
        self.stop(Duration::from_secs(1));
        self.peak_bytes.load(Ordering::Relaxed) / 1024 / 1024
    }
}

/// Mirrors the frontend `diarizationMode` setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiarizationMode {
    Off,
    Efficient,
    Fast,
}

impl DiarizationMode {
    pub fn parse(opt: Option<&str>) -> Self {
        match opt.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("fast") => DiarizationMode::Fast,
            Some("efficient") => DiarizationMode::Efficient,
            _ => DiarizationMode::Off,
        }
    }

    pub fn is_online(self) -> bool {
        matches!(self, DiarizationMode::Efficient | DiarizationMode::Fast)
    }
}

// ---------------------------------------------------------------------------
// Live telemetry (change: online-diarization-telemetry)
// ---------------------------------------------------------------------------

/// Diarization channel a status line describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiarChannel {
    Microphone,
    System,
}

impl DiarChannel {
    /// Source-device key used by the live turn stream.
    pub fn source_device(self) -> &'static str {
        match self {
            DiarChannel::Microphone => "Microphone",
            DiarChannel::System => "System",
        }
    }
}

/// Display state of one channel's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiarChannelState {
    /// No online diarization session is available.
    Unavailable,
    /// Mono session: there is no system channel to report on.
    Inactive,
    /// Efficient mode: clustering is deferred to recording stop.
    Deferred,
    /// Chunks are being embedded but no stable turn exists yet.
    Accumulating,
    /// Embedding and turn order are healthy.
    Healthy,
    /// Published turns are no longer ordered in time.
    Warning,
    /// Embedding stopped and the session's diarization was disabled.
    Error,
}

/// Resolve one channel's display state from its counters and session context.
/// Kept pure so every state is testable without an engine or audio.
#[allow(clippy::too_many_arguments)] // 8 params; pure resolver over independent counters, kept flat for testability
pub(crate) fn resolve_channel_state(
    available: bool,
    mode: DiarizationMode,
    channel: DiarChannel,
    has_system_device: bool,
    engine_disabled: bool,
    chunks: u64,
    turns: u64,
    ordered: bool,
) -> DiarChannelState {
    if !available {
        return DiarChannelState::Unavailable;
    }
    if channel == DiarChannel::System && !has_system_device {
        return DiarChannelState::Inactive;
    }
    if engine_disabled && chunks > 0 {
        return DiarChannelState::Error;
    }
    if mode == DiarizationMode::Efficient {
        return DiarChannelState::Deferred;
    }
    if turns > 0 {
        return if ordered {
            DiarChannelState::Healthy
        } else {
            DiarChannelState::Warning
        };
    }
    DiarChannelState::Accumulating
}

/// The most recent stable turn on a channel, as shown in its status line.
#[derive(Debug, Clone, Serialize)]
pub struct LastTurn {
    pub speaker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// One channel's status line.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelStatusLine {
    pub channel: DiarChannel,
    pub state: DiarChannelState,
    pub chunks: u64,
    pub embed_ok: u64,
    pub embed_failed: u64,
    pub buffered: u64,
    /// Audio seconds covered by the buffered embeddings.
    pub buffered_secs: f32,
    pub turns: u64,
    pub ordered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn: Option<LastTurn>,
}

/// Per-channel counters updated from the chunk path.
#[derive(Debug, Default)]
struct ChannelStats {
    chunks: AtomicU64,
    embed_ok: AtomicU64,
    embed_failed: AtomicU64,
    buffered: AtomicU64,
    /// Audio covered by the buffered embeddings, in milliseconds: the buffer
    /// read as work done rather than a bare count.
    buffered_ms: AtomicU64,
    turns: AtomicU64,
}

/// Session-scoped online diarization stats. Counters are cheap atomics written
/// from the chunk path; the status command reads them on its own interval, so
/// no event is emitted per audio chunk.
pub struct OnlineDiarizationStats {
    mode: DiarizationMode,
    has_system_device: bool,
    available: AtomicBool,
    engine_disabled: AtomicBool,
    mic: ChannelStats,
    sys: ChannelStats,
    /// Speech blocks sent to the (unbounded) embedding channel this session.
    blocks_sent: AtomicU64,
    /// Blocks consumed (processed or dropped) by the engine path.
    blocks_processed: AtomicU64,
    /// True while the engine is working on a dequeued block.
    blocks_in_flight: AtomicBool,
}

impl OnlineDiarizationStats {
    pub fn new(mode: DiarizationMode, has_system_device: bool) -> Self {
        Self {
            mode,
            has_system_device,
            available: AtomicBool::new(false),
            engine_disabled: AtomicBool::new(false),
            mic: ChannelStats::default(),
            sys: ChannelStats::default(),
            blocks_sent: AtomicU64::new(0),
            blocks_processed: AtomicU64::new(0),
            blocks_in_flight: AtomicBool::new(false),
        }
    }

    /// A block was enqueued to the diarization stage (called from the
    /// pipeline's send path, before the engine has dequeued it).
    pub fn record_block_enqueued(&self) {
        self.blocks_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// A dequeued block has been consumed by the engine path (processed,
    /// dropped by a too-short check, or skipped in the error state): the
    /// pending gauge falls by one, clamped at zero.
    pub fn record_block_consumed(&self) {
        self.blocks_processed.fetch_add(1, Ordering::Relaxed);
        self.blocks_in_flight.store(false, Ordering::SeqCst);
    }

    /// The engine path started working on a dequeued block.
    pub fn record_block_in_flight(&self) {
        self.blocks_in_flight.store(true, Ordering::SeqCst);
    }

    /// Blocks sent to the diarization stage this session (total).
    pub fn blocks_sent_total(&self) -> u64 {
        self.blocks_sent.load(Ordering::Relaxed)
    }

    /// Blocks consumed by the diarization stage this session (total).
    pub fn blocks_completed_total(&self) -> u64 {
        self.blocks_processed.load(Ordering::Relaxed)
    }

    /// Blocks sent but not yet consumed (the pending gauge).
    pub fn blocks_pending_now(&self) -> u64 {
        let sent = self.blocks_sent.load(Ordering::Relaxed);
        let processed = self.blocks_processed.load(Ordering::Relaxed);
        sent.saturating_sub(processed)
    }

    /// Whether the engine is currently working on a consumed block.
    pub fn blocks_in_flight_now(&self) -> bool {
        self.blocks_in_flight.load(Ordering::SeqCst)
    }

    pub fn mode(&self) -> DiarizationMode {
        self.mode
    }

    /// Mark the processor as constructed. Until then the lines report
    /// unavailable rather than showing zeros as live values.
    pub fn mark_available(&self) {
        self.available.store(true, Ordering::SeqCst);
    }

    /// Whether the processor was constructed for this session.
    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    fn channel(&self, channel: DiarChannel) -> &ChannelStats {
        match channel {
            DiarChannel::Microphone => &self.mic,
            DiarChannel::System => &self.sys,
        }
    }

    pub(crate) fn record_chunk(&self, channel: DiarChannel) {
        self.channel(channel)
            .chunks
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_embed_ok(&self, channel: DiarChannel) {
        self.channel(channel)
            .embed_ok
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_embed_failed(&self, channel: DiarChannel) {
        self.channel(channel)
            .embed_failed
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_buffered(&self, channel: DiarChannel, audio_ms: u64) {
        let stats = self.channel(channel);
        stats.buffered.fetch_add(1, Ordering::Relaxed);
        stats.buffered_ms.fetch_add(audio_ms, Ordering::Relaxed);
    }

    pub(crate) fn record_turn(&self, channel: DiarChannel) {
        self.channel(channel).turns.fetch_add(1, Ordering::Relaxed);
    }

    /// The session's diarization was disabled (embedding or pipeline failure).
    pub(crate) fn disable(&self) {
        self.engine_disabled.store(true, Ordering::SeqCst);
    }

    /// One channel's status line. Turn count, turn order and the last turn come
    /// from the shared live turn registry, so there is a single source of truth
    /// for what the live stream published.
    pub fn line(&self, channel: DiarChannel, registry: &LiveTurnRegistry) -> ChannelStatusLine {
        let stats = self.channel(channel);
        let chunks = stats.chunks.load(Ordering::Relaxed);
        let turns = stats.turns.load(Ordering::Relaxed);
        let source_device = channel.source_device();
        let ordered = registry.is_ordered(source_device);
        let last_turn = registry
            .turns(source_device)
            .last()
            .map(|turn| LastTurn {
                speaker: turn.speaker.clone(),
                display_name: turn.display_name.clone(),
                matched_by: turn.matched_by.clone(),
                score: turn.match_score,
            });

        ChannelStatusLine {
            channel,
            state: resolve_channel_state(
                self.available.load(Ordering::SeqCst),
                self.mode,
                channel,
                self.has_system_device,
                self.engine_disabled.load(Ordering::SeqCst),
                chunks,
                turns,
                ordered,
            ),
            chunks,
            embed_ok: stats.embed_ok.load(Ordering::Relaxed),
            embed_failed: stats.embed_failed.load(Ordering::Relaxed),
            buffered: stats.buffered.load(Ordering::Relaxed),
            buffered_secs: stats.buffered_ms.load(Ordering::Relaxed) as f32 / 1000.0,
            turns,
            ordered,
            last_turn,
        }
    }
}

/// Read-only snapshot backing the two live status lines.
#[derive(Debug, Clone, Serialize)]
pub struct OnlineDiarizationStatus {
    /// True while an online diarization session is running.
    pub active: bool,
    pub mode: DiarizationMode,
    pub available: bool,
    pub model_tag: String,
    pub embedding_dim: usize,
    pub recognition_threshold: f32,
    /// Prototype and session-binding counts; `None` when no store is loaded.
    pub prototypes: Option<usize>,
    pub bindings: Option<usize>,
    /// Speech blocks sent to the diarization stage but not yet consumed.
    pub pending_blocks: u64,
    /// Blocks sent (total) / consumed (total) this session.
    pub blocks_sent: u64,
    pub blocks_processed: u64,
    /// True while the engine works on a dequeued block.
    pub blocks_in_flight: bool,
    pub mic: ChannelStatusLine,
    pub sys: ChannelStatusLine,
}

// Session-scoped stats for the live status lines. Installed at recording start
// and cleared at stop, so a stopped session's counters are never presented as
// live values.
static ONLINE_DIARIZATION_STATS: Mutex<Option<Arc<OnlineDiarizationStats>>> = Mutex::new(None);

/// Install a fresh stats holder for a new session, clearing any previous one.
pub fn begin_stats(mode: DiarizationMode, has_system_device: bool) -> Arc<OnlineDiarizationStats> {
    let stats = Arc::new(OnlineDiarizationStats::new(mode, has_system_device));
    if let Ok(mut slot) = ONLINE_DIARIZATION_STATS.lock() {
        *slot = Some(stats.clone());
    }
    stats
}

/// The running session's stats, if any.
pub fn current_stats() -> Option<Arc<OnlineDiarizationStats>> {
    ONLINE_DIARIZATION_STATS.lock().ok().and_then(|s| s.clone())
}

/// Drop the session's stats (recording stopped).
pub fn clear_stats() {
    if let Ok(mut slot) = ONLINE_DIARIZATION_STATS.lock() {
        *slot = None;
    }
}

/// Count one speech block enqueued to the running diarization session (called
/// from the pipeline's send path). No-op without a session.
pub fn record_block_enqueued() {
    if let Some(stats) = current_stats() {
        stats.record_block_enqueued();
    }
}

/// Map a pipeline device type to its diarization channel.
pub(crate) fn diar_channel(device: &DeviceType) -> DiarChannel {
    match device {
        DeviceType::System => DiarChannel::System,
        DeviceType::Microphone => DiarChannel::Microphone,
    }
}

/// Apply a stats update when telemetry is attached, otherwise do nothing.
/// Takes the handle by reference (not through the processor) so the chunk path
/// can update counters while the engine is still borrowed mutably.
pub(crate) fn record_stats<F: FnOnce(&OnlineDiarizationStats)>(
    stats: &Option<Arc<OnlineDiarizationStats>>,
    update: F,
) {
    if let Some(stats) = stats.as_deref() {
        update(stats);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::time::Instant;

    use super::super::batch::pcm::drain_with_stop;
    use super::*;

    /// A `Read` that never reaches EOF and sleeps a bit on every call before
    /// returning a few bytes — simulating a pipe with no data ready yet, so a
    /// loop reading it would otherwise never stop on its own. Exercises the
    /// same generic seam (`drain_with_stop<R: Read>`) `PcmStream`'s real
    /// stderr-reader thread is built on; `PcmStream` itself cannot be
    /// constructed in a unit test without a real OS child process (it owns a
    /// `std::process::Child`/`ChildStdout`).
    struct TricklingReader {
        delay: Duration,
    }

    impl Read for TricklingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(self.delay);
            let n = buf.len().min(4);
            for b in buf.iter_mut().take(n) {
                *b = 0;
            }
            Ok(n)
        }
    }

    #[test]
    fn drain_with_stop_exits_a_would_otherwise_block_forever_reader_within_the_timeout() {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let _ = drain_with_stop(
                TricklingReader {
                    delay: Duration::from_millis(10),
                },
                &stop_clone,
            );
        });
        let mut handle_opt = Some(handle);

        // Give the reader thread a couple of iterations before asking it to
        // stop, then prove `join_within` (the same helper `PcmStream::stop`
        // and `MemorySampler::stop` use) observes the stop within its bound.
        std::thread::sleep(Duration::from_millis(30));
        stop.store(true, Ordering::Relaxed);

        let joined = join_within(&mut handle_opt, Duration::from_millis(500));
        assert!(joined, "reader thread did not stop within the timeout");
    }

    #[test]
    fn memory_sampler_stop_joins_promptly() {
        let mut sampler = MemorySampler::start();
        let start = Instant::now();
        let joined = sampler.stop(Duration::from_secs(2));
        let elapsed = start.elapsed();
        assert!(joined, "MemorySampler did not stop within the timeout");
        // The sampler polls every 500ms; a prompt stop should not need to
        // wait for anywhere near that, let alone several iterations of it.
        assert!(
            elapsed < Duration::from_millis(500),
            "stop() took {:?}, expected a prompt join",
            elapsed
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn state(
        available: bool,
        mode: DiarizationMode,
        channel: DiarChannel,
        has_system: bool,
        disabled: bool,
        chunks: u64,
        turns: u64,
        ordered: bool,
    ) -> DiarChannelState {
        resolve_channel_state(
            available, mode, channel, has_system, disabled, chunks, turns, ordered,
        )
    }

    #[test]
    fn telemetry_counters_stay_isolated_per_channel() {
        let stats = OnlineDiarizationStats::new(DiarizationMode::Fast, true);
        stats.mark_available();
        stats.record_chunk(DiarChannel::Microphone);
        stats.record_buffered(DiarChannel::Microphone, 2400);

        let registry = LiveTurnRegistry::new();
        let mic = stats.line(DiarChannel::Microphone, &registry);
        let sys = stats.line(DiarChannel::System, &registry);

        assert_eq!(mic.chunks, 1);
        assert_eq!(sys.chunks, 0);
        assert_eq!(mic.embed_ok, 0);
        assert_eq!(sys.embed_ok, 0);
        // Buffered audio is derived per channel, not shared.
        assert_eq!(mic.buffered_secs, 2.4);
        assert_eq!(sys.buffered_secs, 0.0);
    }

    #[test]
    fn telemetry_each_counter_advances_once_per_event() {
        let stats = OnlineDiarizationStats::new(DiarizationMode::Fast, true);
        stats.mark_available();
        stats.record_chunk(DiarChannel::Microphone);
        stats.record_embed_ok(DiarChannel::Microphone);
        stats.record_embed_failed(DiarChannel::Microphone);
        stats.record_buffered(DiarChannel::Microphone, 2400);
        stats.record_turn(DiarChannel::Microphone);

        let line = stats.line(DiarChannel::Microphone, &LiveTurnRegistry::new());
        assert_eq!(line.chunks, 1);
        assert_eq!(line.embed_ok, 1);
        assert_eq!(line.embed_failed, 1);
        assert_eq!(line.buffered, 1);
        assert_eq!(line.buffered_secs, 2.4);
        assert_eq!(line.turns, 1);
    }

    #[test]
    fn telemetry_line_reports_registry_last_turn() {
        use crate::audio::live_diarization_reconcile::LiveTurn;

        let stats = OnlineDiarizationStats::new(DiarizationMode::Fast, true);
        stats.mark_available();
        stats.record_turn(DiarChannel::Microphone);

        let registry = LiveTurnRegistry::new();
        registry.publish(LiveTurn {
            start_time: 1.0,
            end_time: 3.0,
            speaker: "MIC_SPEAKER_01".to_string(),
            source_device: "Microphone".to_string(),
            display_name: Some("Alice".to_string()),
            matched_by: Some("auto".to_string()),
            match_score: Some(0.74),
        });

        let line = stats.line(DiarChannel::Microphone, &registry);
        let last = line.last_turn.expect("last turn recorded");
        assert_eq!(last.speaker, "MIC_SPEAKER_01");
        assert_eq!(last.display_name.as_deref(), Some("Alice"));
        assert_eq!(last.matched_by.as_deref(), Some("auto"));
        assert_eq!(last.score, Some(0.74));
        assert!(line.ordered);
        assert_eq!(line.state, DiarChannelState::Healthy);

        // The system channel is untouched by a microphone turn.
        let sys = stats.line(DiarChannel::System, &registry);
        assert!(sys.last_turn.is_none());
        assert_eq!(sys.turns, 0);
    }

    #[test]
    fn telemetry_new_session_starts_from_zero() {
        let _guard = crate::audio::telemetry::TELEMETRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let first = begin_stats(DiarizationMode::Fast, true);
        first.mark_available();
        first.record_chunk(DiarChannel::Microphone);
        assert_eq!(
            current_stats()
                .unwrap()
                .line(DiarChannel::Microphone, &LiveTurnRegistry::new())
                .chunks,
            1
        );

        let second = begin_stats(DiarizationMode::Fast, true);
        second.mark_available();
        assert_eq!(
            current_stats()
                .unwrap()
                .line(DiarChannel::Microphone, &LiveTurnRegistry::new())
                .chunks,
            0
        );

        clear_stats();
        assert!(current_stats().is_none());
    }

    #[test]
    fn telemetry_state_unavailable_without_session() {
        assert_eq!(
            state(
                false,
                DiarizationMode::Fast,
                DiarChannel::Microphone,
                true,
                false,
                0,
                0,
                true
            ),
            DiarChannelState::Unavailable
        );
    }

    #[test]
    fn telemetry_state_mono_system_is_inactive_not_error() {
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::System,
                false,
                true,
                5,
                0,
                true
            ),
            DiarChannelState::Inactive
        );
    }

    #[test]
    fn telemetry_state_efficient_defers_without_warning() {
        assert_eq!(
            state(
                true,
                DiarizationMode::Efficient,
                DiarChannel::Microphone,
                true,
                false,
                7,
                0,
                true
            ),
            DiarChannelState::Deferred
        );
    }

    #[test]
    fn telemetry_state_no_speech_is_not_a_warning() {
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::Microphone,
                true,
                false,
                0,
                0,
                true
            ),
            DiarChannelState::Accumulating
        );
    }

    #[test]
    fn telemetry_state_follows_turn_order() {
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::Microphone,
                true,
                false,
                4,
                3,
                true
            ),
            DiarChannelState::Healthy
        );
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::Microphone,
                true,
                false,
                4,
                3,
                false
            ),
            DiarChannelState::Warning
        );
    }

    #[test]
    fn telemetry_state_error_only_for_channel_that_fed() {
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::Microphone,
                true,
                true,
                4,
                0,
                true
            ),
            DiarChannelState::Error
        );
        // Engine disabled but this channel never fed: still just waiting.
        assert_eq!(
            state(
                true,
                DiarizationMode::Fast,
                DiarChannel::System,
                true,
                true,
                0,
                0,
                true
            ),
            DiarChannelState::Accumulating
        );
    }
}
