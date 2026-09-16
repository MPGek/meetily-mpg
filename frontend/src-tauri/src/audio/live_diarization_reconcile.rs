//! Live word-level diarization reconcile (live-word-level-diarization).
//!
//! Display-only live token→speaker attribution for Fast-mode recording. A
//! finalized transcript block carrying word tokens (refined by live CTC
//! alignment when available, otherwise the transcription engine's own
//! timestamps) is attributed per token against the live stable speaker turns
//! published by the online diarizer. A block spanning more than one validated
//! speaker run is emitted as split display sub-rows through the
//! `live-transcript-blocks` event.
//!
//! Nothing here touches persistence: `SHARED_SEGMENTS`, incremental transcript
//! files, and the database keep the original block. Stop-time finalize remains
//! the authoritative splitter (spec `online-speaker-diarization`).
//!
//! Because a stable turn can arrive after its block finalizes, blocks that are
//! not yet decidable are held in a bounded provisional set and re-attributed
//! when new turns arrive (watermark rule: decidable once a same-channel turn
//! starts at or after the block's end). Overflow drops the oldest held block,
//! keeping its last emitted rendering.
//!
//! Fast mode only: Efficient/Off never publish turns, so the pre-checks here
//! make the whole module a no-op for them.

use crate::audio::token_assignment::{assign_tokens_to_speakers, SpeakerTurn, Token};
use crate::audio::transcription::TranscriptUpdate;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Notify;

/// Event carrying a transcript block's live display sub-rows.
pub const EVENT_LIVE_TRANSCRIPT_BLOCKS: &str = "live-transcript-blocks";

/// Bounded provisional set (block count). Overflow drops the oldest held block,
/// which keeps its last emitted rendering (spec fallback).
pub const PROVISIONAL_CAPACITY: usize = 256;

/// One live stable speaker turn published by the Fast-mode diarizer.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveTurn {
    pub start_time: f64,
    pub end_time: f64,
    /// Raw cluster label (`SPEAKER_NN` / `MIC_SPEAKER_NN`).
    pub speaker: String,
    pub source_device: String,
    pub display_name: Option<String>,
    pub matched_by: Option<String>,
    pub match_score: Option<f32>,
}

/// One emitted display sub-row of a transcript block.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiveBlock {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Raw cluster label (drives color/identity), matching the turn stream.
    pub speaker: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_score: Option<f32>,
}

/// Payload of `live-transcript-blocks`: the current display revision of one
/// parent transcript block. Children carry no `sequence_id` of their own.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LiveTranscriptBlocks {
    pub parent_sequence_id: u64,
    pub source_device: String,
    /// Monotonically increasing per parent; the frontend renders only the
    /// latest revision.
    pub revision: u64,
    pub blocks: Vec<LiveBlock>,
}

// ============================================================================
// Live turn registry
// ============================================================================

#[derive(Default)]
struct RegistryInner {
    /// Stable turns per channel ("Microphone" | "System"), append-only.
    channels: HashMap<String, Vec<LiveTurn>>,
    /// Per-channel monotonicity flag for the stability spike (task 1.1): false
    /// once a published turn went backwards in time.
    monotonic: HashMap<String, bool>,
}

/// Per-channel append-only stream of stable turns, shared between the online
/// diarization processor (publisher) and the reconcile consumer (reader).
pub struct LiveTurnRegistry {
    inner: Mutex<RegistryInner>,
    notify: Notify,
    closed: AtomicBool,
}

impl LiveTurnRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RegistryInner::default()),
            notify: Notify::new(),
            closed: AtomicBool::new(false),
        }
    }

    /// Append a stable turn and wake the reconcile consumer. Returns `false`
    /// when the turn went backwards in time for its channel (spike
    /// instrumentation, task 1.1); the turn is still recorded.
    pub fn publish(&self, turn: LiveTurn) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let monotonic = {
            let last = inner
                .channels
                .get(&turn.source_device)
                .and_then(|v| v.last())
                .cloned();
            let ordered = match &last {
                Some(last) => {
                    turn.start_time >= last.start_time && turn.end_time >= last.end_time
                }
                None => true,
            };
            let flag = inner
                .monotonic
                .entry(turn.source_device.clone())
                .or_insert(true);
            if !ordered && *flag {
                if let Some(last) = &last {
                    log::warn!(
                        "Live turn stream went backwards on {}: last=({:.3},{:.3}) now=({:.3},{:.3}) label={} — watermark may release early",
                        turn.source_device,
                        last.start_time,
                        last.end_time,
                        turn.start_time,
                        turn.end_time,
                        turn.speaker
                    );
                }
                *flag = false;
            }
            *flag
        };
        log::debug!(
            "live-turn publish channel={} label={} start={:.3} end={:.3} monotonic={}",
            turn.source_device,
            turn.speaker,
            turn.start_time,
            turn.end_time,
            monotonic
        );
        inner.channels.entry(turn.source_device.clone()).or_default().push(turn);
        drop(inner);
        self.notify.notify_one();
        monotonic
    }

    /// Snapshot of a channel's stable turns.
    pub fn turns(&self, source_device: &str) -> Vec<LiveTurn> {
        self.inner
            .lock()
            .unwrap()
            .channels
            .get(source_device)
            .cloned()
            .unwrap_or_default()
    }

    /// Whether every turn published on this channel so far kept time order.
    /// True for a channel that has published nothing yet (nothing to regress).
    pub fn is_ordered(&self, source_device: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .monotonic
            .get(source_device)
            .copied()
            .unwrap_or(true)
    }

    /// Any stable turn published on any channel (Fast-mode-active gate).
    pub fn has_any_turns(&self) -> bool {
        self.inner
            .lock()
            .unwrap()
            .channels
            .values()
            .any(|v| !v.is_empty())
    }

    /// Watermark rule: a block ending at `block_end` is decidable once a
    /// same-channel stable turn starts at or after it (no future turn can
    /// overlap the block).
    pub fn decidable(&self, source_device: &str, block_end: f64) -> bool {
        self.inner
            .lock()
            .unwrap()
            .channels
            .get(source_device)
            .map(|turns| turns.iter().any(|t| t.start_time >= block_end))
            .unwrap_or(false)
    }

    /// Reset for a new recording session (same process).
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.channels.clear();
        inner.monotonic.clear();
        drop(inner);
        self.closed.store(false, Ordering::SeqCst);
    }

    /// No more publications; wake the consumer so it can exit.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Wait until a turn is published (or the registry closes).
    pub async fn changed(&self) {
        if self.is_closed() {
            return;
        }
        self.notify.notified().await;
    }
}

impl Default for LiveTurnRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Attribution
// ============================================================================

/// Provenance of the turn best overlapping `[start, end]` for `label`.
fn provenance<'a>(
    turns: impl Iterator<Item = &'a LiveTurn>,
    label: &str,
    start: f64,
    end: f64,
) -> Option<&'a LiveTurn> {
    let mut best: Option<&LiveTurn> = None;
    let mut best_overlap = 0.0f64;
    for t in turns {
        if t.speaker != label {
            continue;
        }
        let overlap = end.min(t.end_time) - start.max(t.start_time);
        if best.is_none() || overlap > best_overlap {
            best = Some(t);
            best_overlap = overlap.max(0.0);
        }
    }
    best
}

/// Attribute a block's tokens to the covering live turns of its own channel.
/// Returns `None` when there are no turns, no token got a speaker, or the
/// first token is unattributed (a leading gap would drop words from the
/// emitted sub-row text).
pub fn attribute(tokens: &[Token], source_device: &str, registry: &LiveTurnRegistry) -> Option<Vec<LiveBlock>> {
    let turns = registry.turns(source_device);
    if turns.is_empty() || tokens.is_empty() {
        return None;
    }

    // Map live cluster labels to stable integer ids for the shared token
    // assignment (same function stop-time finalize uses).
    let mut label_to_idx: HashMap<String, i32> = HashMap::new();
    let mut idx_to_label: Vec<String> = Vec::new();
    let mut token_turns: Vec<SpeakerTurn> = Vec::with_capacity(turns.len());
    for t in &turns {
        let idx = match label_to_idx.get(&t.speaker) {
            Some(v) => *v,
            None => {
                let v = idx_to_label.len() as i32;
                label_to_idx.insert(t.speaker.clone(), v);
                idx_to_label.push(t.speaker.clone());
                v
            }
        };
        token_turns.push(SpeakerTurn {
            start: t.start_time as f32,
            end: t.end_time as f32,
            speaker: idx,
        });
    }

    let assignment = assign_tokens_to_speakers(tokens, &token_turns);
    if assignment.blocks.is_empty() || assignment.blocks[0].start_idx != 0 {
        return None;
    }

    let mut blocks = Vec::with_capacity(assignment.blocks.len());
    for block in &assignment.blocks {
        let label = idx_to_label.get(block.speaker as usize)?.clone();
        let text = tokens[block.start_idx..=block.end_idx]
            .iter()
            .map(|t| t.text.as_str())
            .collect::<String>();
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let prov = provenance(turns.iter(), &label, block.start as f64, block.end as f64);
        blocks.push(LiveBlock {
            start: block.start as f64,
            end: block.end as f64,
            text,
            speaker: label,
            display_name: prov.and_then(|t| t.display_name.clone()),
            matched_by: prov.and_then(|t| t.matched_by.clone()),
            match_score: prov.and_then(|t| t.match_score),
        });
    }
    if blocks.is_empty() {
        None
    } else {
        Some(blocks)
    }
}

// ============================================================================
// Reconciler
// ============================================================================

struct HeldBlock {
    parent_sequence_id: u64,
    source_device: String,
    audio_end: f64,
    tokens: Vec<Token>,
    /// Last emitted revision (0 = never emitted).
    revision: u64,
    /// Last emitted blocks, for change detection.
    last: Option<Vec<LiveBlock>>,
}

/// Bounded provisional set of blocks awaiting a decidable turn stream, plus
/// the emit decision shared by submit (block finalization) and reconcile
/// (turn arrival).
pub struct Reconciler {
    held: VecDeque<HeldBlock>,
    capacity: usize,
}

impl Reconciler {
    pub fn new(capacity: usize) -> Self {
        Self {
            held: VecDeque::new(),
            capacity,
        }
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }

    pub fn held_len(&self) -> usize {
        self.held.len()
    }

    fn hold(&mut self, held: HeldBlock) {
        self.held.push_back(held);
        while self.held.len() > self.capacity {
            // Drop oldest; it keeps its last emitted rendering (spec fallback).
            let dropped = self.held.pop_front();
            if let Some(b) = dropped {
                log::warn!(
                    "Live diarization provisional set overflow: released block seq {} (keeps last rendering)",
                    b.parent_sequence_id
                );
            }
        }
    }

    /// A finalized block arrived. Emits the initial revision when there is
    /// something to show, and holds the block while the turn stream is not yet
    /// decidable.
    pub fn submit(
        &mut self,
        update: &TranscriptUpdate,
        registry: &LiveTurnRegistry,
    ) -> Option<LiveTranscriptBlocks> {
        let tokens = update.tokens.clone()?;
        if tokens.is_empty() {
            return None;
        }
        let source_device = update.source_device.clone();

        let blocks = attribute(&tokens, &source_device, registry);
        let block_end = update.audio_end_time;
        let decidable = registry.decidable(&source_device, block_end);

        let (revision, last, payload) = match blocks {
            Some(b) if !b.is_empty() => (
                1,
                Some(b.clone()),
                Some(LiveTranscriptBlocks {
                    parent_sequence_id: update.sequence_id,
                    source_device: source_device.clone(),
                    revision: 1,
                    blocks: b,
                }),
            ),
            _ => (0, None, None),
        };

        if !decidable {
            self.hold(HeldBlock {
                parent_sequence_id: update.sequence_id,
                source_device,
                audio_end: block_end,
                tokens,
                revision,
                last,
            });
        }

        payload
    }

    /// New turns arrived: re-attribute every held block, emitting a new
    /// revision for those whose grouping changed, and release decidable ones.
    pub fn reconcile(&mut self, registry: &LiveTurnRegistry) -> Vec<LiveTranscriptBlocks> {
        let mut out = Vec::new();
        let mut keep = VecDeque::with_capacity(self.held.len());
        while let Some(mut held) = self.held.pop_front() {
            let blocks = attribute(&held.tokens, &held.source_device, registry);
            let decidable = registry.decidable(&held.source_device, held.audio_end);
            if let Some(b) = blocks {
                if !b.is_empty() && held.last.as_ref() != Some(&b) {
                    held.revision += 1;
                    held.last = Some(b.clone());
                    out.push(LiveTranscriptBlocks {
                        parent_sequence_id: held.parent_sequence_id,
                        source_device: held.source_device.clone(),
                        revision: held.revision,
                        blocks: b,
                    });
                }
            }
            if !decidable {
                keep.push_back(held);
            }
        }
        self.held = keep;
        out
    }
}

// ============================================================================
// Session globals + wiring
// ============================================================================

static REGISTRY: OnceLock<Arc<LiveTurnRegistry>> = OnceLock::new();
static RECONCILER: OnceLock<Arc<Mutex<Reconciler>>> = OnceLock::new();

/// Process-wide live turn registry (one recording at a time is enforced).
pub fn registry() -> Arc<LiveTurnRegistry> {
    REGISTRY
        .get_or_init(|| Arc::new(LiveTurnRegistry::new()))
        .clone()
}

/// Process-wide reconciler state.
pub fn reconciler() -> Arc<Mutex<Reconciler>> {
    RECONCILER
        .get_or_init(|| Arc::new(Mutex::new(Reconciler::new(PROVISIONAL_CAPACITY))))
        .clone()
}

/// Reset registry + provisional set for a new recording session.
pub fn reset_session() {
    let registry = registry();
    registry.clear();
    let rec = reconciler();
    {
        let mut guard = rec.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
    }
}

fn emit<R: Runtime>(app: &AppHandle<R>, payload: &LiveTranscriptBlocks) {
    if let Err(e) = app.emit(EVENT_LIVE_TRANSCRIPT_BLOCKS, payload) {
        log::warn!("Failed to emit live-transcript-blocks: {}", e);
    }
}

/// Hand a finalized (already token-refined when alignment was available) block
/// to the live reconcile stage. Cheap no-op until live turns exist, so
/// Efficient/Off are unaffected.
pub fn submit<R: Runtime>(app: &AppHandle<R>, update: &TranscriptUpdate) {
    if update.is_partial {
        return;
    }
    let registry = registry();
    if !registry.has_any_turns() {
        return;
    }
    let rec = reconciler();
    let payload = {
        let mut r = match rec.lock() {
            Ok(r) => r,
            Err(_) => return,
        };
        r.submit(update, &registry)
    };
    if let Some(p) = payload {
        emit(app, &p);
    }
}

/// Spawn the consumer that re-evaluates held blocks whenever new turns arrive.
pub fn spawn_cascade<R: Runtime>(app: AppHandle<R>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let registry = registry();
        loop {
            registry.changed().await;
            if registry.is_closed() {
                break;
            }
            let rec = reconciler();
            let payloads = {
                let mut r = match rec.lock() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                r.reconcile(&registry)
            };
            for p in payloads {
                emit(&app, &p);
            }
        }
        log::info!("Live diarization reconcile consumer finished");
    })
}

/// Stop the cascade with a bounded wait (display-only, so leftovers are fine;
/// stop-time finalize remains authoritative).
pub async fn close_cascade(handle: tokio::task::JoinHandle<()>) {
    let registry = registry();
    registry.close();
    match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
        Ok(Ok(())) => log::info!("✅ Live diarization reconcile cascade stopped"),
        Ok(Err(e)) => log::warn!("Live diarization cascade ended abnormally: {:?}", e),
        Err(_) => log::warn!("Live diarization cascade stop timed out; continuing to finalize"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str, start: f32, end: f32) -> Token {
        Token {
            text: text.to_string(),
            start,
            end,
            refined: false,
        }
    }

    fn turn(label: &str, start: f64, end: f64, device: &str) -> LiveTurn {
        LiveTurn {
            start_time: start,
            end_time: end,
            speaker: label.to_string(),
            source_device: device.to_string(),
            display_name: None,
            matched_by: None,
            match_score: None,
        }
    }

    #[test]
    fn is_ordered_flags_only_the_regressing_channel() {
        let registry = LiveTurnRegistry::new();
        // Nothing published yet: nothing to regress.
        assert!(registry.is_ordered("Microphone"));

        registry.publish(turn("MIC_SPEAKER_00", 1.0, 3.0, "Microphone"));
        registry.publish(turn("MIC_SPEAKER_00", 4.0, 6.0, "Microphone"));
        assert!(registry.is_ordered("Microphone"));

        // A turn that starts before the previous one on the microphone only.
        registry.publish(turn("MIC_SPEAKER_00", 2.0, 5.0, "Microphone"));
        assert!(!registry.is_ordered("Microphone"));
        assert!(registry.is_ordered("System"));
    }

    fn update(seq: u64, device: &str, start: f64, end: f64, tokens: Vec<Token>) -> TranscriptUpdate {
        TranscriptUpdate {
            text: tokens.iter().map(|t| t.text.as_str()).collect(),
            timestamp: "[00:00]".to_string(),
            source: "Audio".to_string(),
            sequence_id: seq,
            chunk_start_time: start,
            is_partial: false,
            confidence: 0.9,
            audio_start_time: start,
            audio_end_time: end,
            duration: end - start,
            source_device: device.to_string(),
            speaker: None,
            tokens: Some(tokens),
        }
    }

    #[tokio::test]
    async fn live_turn_registry_appends_and_tracks_watermark() {
        let reg = LiveTurnRegistry::new();
        assert!(!reg.has_any_turns());
        assert!(!reg.decidable("Microphone", 5.0));

        assert!(reg.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone")));
        assert!(reg.has_any_turns());
        assert_eq!(reg.turns("Microphone").len(), 1);
        assert!(!reg.decidable("Microphone", 5.0));

        // A turn starting after the block end makes it decidable.
        assert!(reg.publish(turn("SPEAKER_01", 6.0, 7.0, "Microphone")));
        assert!(reg.decidable("Microphone", 5.0));
        // Other channels are unaffected.
        assert!(!reg.decidable("System", 5.0));
    }

    #[tokio::test]
    async fn live_turn_registry_flags_non_monotonic() {
        let reg = LiveTurnRegistry::new();
        assert!(reg.publish(turn("SPEAKER_00", 4.0, 6.0, "Microphone")));
        // Backwards turn is recorded but reported non-monotonic.
        assert!(!reg.publish(turn("SPEAKER_01", 1.0, 2.0, "Microphone")));
        assert_eq!(reg.turns("Microphone").len(), 2);
    }

    #[tokio::test]
    async fn attribute_single_speaker_covers_whole_block() {
        let reg = LiveTurnRegistry::new();
        reg.publish(turn("SPEAKER_00", 0.0, 10.0, "Microphone"));
        let tokens = vec![
            token("Hello", 0.0, 1.0),
            token(" world", 1.0, 2.0),
        ];
        let blocks = attribute(&tokens, "Microphone", &reg).expect("blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "Hello world");
        assert_eq!(blocks[0].speaker, "SPEAKER_00");
        assert_eq!(blocks[0].start, 0.0);
        assert_eq!(blocks[0].end, 2.0);
    }

    #[tokio::test]
    async fn attribute_splits_validated_speaker_change() {
        let reg = LiveTurnRegistry::new();
        reg.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        reg.publish(turn("SPEAKER_01", 2.0, 10.0, "Microphone"));
        let tokens = vec![
            token("Hi", 0.0, 1.0),
            token(" there", 1.0, 2.0),
            token(" how", 2.0, 3.0),
            token(" are", 3.0, 4.0),
        ];
        let blocks = attribute(&tokens, "Microphone", &reg).expect("blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].speaker, "SPEAKER_00");
        assert_eq!(blocks[0].text, "Hi there");
        assert_eq!(blocks[1].speaker, "SPEAKER_01");
        assert_eq!(blocks[1].text, "how are");
        // Contiguous, gap-free at the boundary.
        assert_eq!(blocks[0].end, blocks[1].start);
    }

    #[tokio::test]
    async fn attribute_drops_leading_unattributed_gap() {
        let reg = LiveTurnRegistry::new();
        // Two distinct speakers so the nearest-turn fallback does not apply to
        // the far leading token (>30 s from every turn).
        reg.publish(turn("SPEAKER_00", 40.0, 44.0, "Microphone"));
        reg.publish(turn("SPEAKER_01", 100.0, 104.0, "Microphone"));
        let tokens = vec![
            token("far", 0.0, 1.0),
            token(" here", 40.0, 41.0),
            token(" next", 41.0, 42.0),
        ];
        // Leading token is unattributed -> would drop "far" from the emitted
        // text, so no emission is produced (block stays as today).
        assert!(attribute(&tokens, "Microphone", &reg).is_none());
    }

    #[tokio::test]
    async fn attribute_channel_isolated() {
        let reg = LiveTurnRegistry::new();
        reg.publish(turn("SPEAKER_00", 0.0, 10.0, "System"));
        let tokens = vec![token("mic", 0.0, 1.0)];
        assert!(attribute(&tokens, "Microphone", &reg).is_none());
    }

    #[tokio::test]
    async fn submit_emits_and_reconciles_late_turn() {
        let reg = LiveTurnRegistry::new();
        let mut reconciler = Reconciler::new(PROVISIONAL_CAPACITY);

        // Only the first speaker is known: block's tail is uncovered, so the
        // block is held (not decidable: no turn starts after the block end).
        reg.publish(turn("SPEAKER_00", 0.0, 1.0, "Microphone"));
        let tokens = vec![
            token("one", 0.0, 1.0),
            token(" two", 2.0, 3.0),
            token(" three", 3.0, 4.0),
        ];
        let first = reconciler.submit(&update(7, "Microphone", 0.0, 4.0, tokens), &reg);
        // Nearest-turn fallback attributes the tail to SPEAKER_00 for now.
        assert_eq!(first.as_ref().map(|p| p.revision), Some(1));
        assert_eq!(reconciler.held_len(), 1);

        // The late turn covering the tail arrives: a new revision splits A|B.
        reg.publish(turn("SPEAKER_01", 2.0, 4.0, "Microphone"));
        let revisions = reconciler.reconcile(&reg);
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].parent_sequence_id, 7);
        assert_eq!(revisions[0].revision, 2);
        assert_eq!(revisions[0].blocks.len(), 2);
        assert_eq!(revisions[0].blocks[0].speaker, "SPEAKER_00");
        assert_eq!(revisions[0].blocks[1].speaker, "SPEAKER_01");
    }

    #[tokio::test]
    async fn submit_releases_decidable_block() {
        let reg = LiveTurnRegistry::new();
        let mut reconciler = Reconciler::new(PROVISIONAL_CAPACITY);
        reg.publish(turn("SPEAKER_00", 0.0, 3.0, "Microphone"));
        // A later turn makes the block decidable.
        reg.publish(turn("SPEAKER_00", 10.0, 12.0, "Microphone"));
        let tokens = vec![token("done", 0.0, 1.0)];
        let payload = reconciler.submit(&update(3, "Microphone", 0.0, 2.0, tokens), &reg);
        assert!(payload.is_some());
        assert_eq!(reconciler.held_len(), 0);
    }

    #[tokio::test]
    async fn provisional_overflow_drops_oldest() {
        let reg = LiveTurnRegistry::new();
        reg.publish(turn("SPEAKER_00", 0.0, 1.0, "Microphone"));
        let mut reconciler = Reconciler::new(2);
        for seq in 1..=3u64 {
            let tokens = vec![token("x", 5.0, 6.0)];
            reconciler.submit(&update(seq, "Microphone", 5.0, 6.0, tokens), &reg);
        }
        assert_eq!(reconciler.held_len(), 2);
        // Oldest (seq 1) was released; remaining are seq 2 and 3.
        let held: Vec<u64> = reconciler.held.iter().map(|h| h.parent_sequence_id).collect();
        assert_eq!(held, vec![2, 3]);
    }

    #[tokio::test]
    async fn reconcile_reports_nothing_when_grouping_unchanged() {
        let reg = LiveTurnRegistry::new();
        let mut reconciler = Reconciler::new(PROVISIONAL_CAPACITY);
        reg.publish(turn("SPEAKER_00", 0.0, 1.0, "Microphone"));
        let tokens = vec![token("stable", 0.0, 1.0)];
        let _ = reconciler.submit(&update(5, "Microphone", 0.0, 2.0, tokens), &reg);
        // A far-away turn is still not decidable-only change; grouping is the
        // same, so no new revision is emitted.
        reg.publish(turn("SPEAKER_00", 0.5, 1.0, "Microphone"));
        assert!(reconciler.reconcile(&reg).is_empty());
    }
}
