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

use crate::audio::sync_ext::LockRecover;
use super::super::core::timeline::split_tokens_by_speaker;
use super::super::DiarizationSegment;
use crate::audio::token_assignment::Token;
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

/// How many already-decided blocks stay eligible for the stop-time final pass
/// (05b D2). A block only needs its tokens and its last rendering kept, so the
/// bound is generous enough to cover a long meeting's blocks; past it, the
/// oldest blocks keep the rendering they were last shown with.
pub const SETTLED_CAPACITY: usize = 4096;

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
    /// Promotion state (05b D2): every revision published while recording is
    /// provisional, and the stop-time pass publishes the final one. A block
    /// the final pass leaves alone keeps its last provisional revision, which
    /// is the same rendering the final pass would have produced.
    #[serde(rename = "final")]
    pub is_final: bool,
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
        let mut inner = self.inner.lock_or_recover();
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
        let mut inner = self.inner.lock_or_recover();
        inner.channels.clear();
        inner.monotonic.clear();
        drop(inner);
        self.closed.store(false, Ordering::SeqCst);
    }

    /// Wake the consumer without publishing a turn: used when something other
    /// than new speech changed what should be on screen (a rename, 05b 3.4).
    pub fn wake(&self) {
        self.notify.notify_one();
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
    attribute_with(tokens, &registry.turns(source_device))
}

/// The attribution itself, against an explicit turn set. The live path passes
/// the registry's published turns; the stop-time final pass passes the refined
/// timeline (05b D2), so both renderings come from the same function and can
/// be compared label for label.
pub fn attribute_with(tokens: &[Token], turns: &[LiveTurn]) -> Option<Vec<LiveBlock>> {
    if turns.is_empty() || tokens.is_empty() {
        return None;
    }

    // Map live cluster labels to stable integer ids for the shared token
    // assignment (same function stop-time finalize uses).
    let mut label_to_idx: HashMap<String, i32> = HashMap::new();
    let mut idx_to_label: Vec<String> = Vec::new();
    let mut spans: Vec<DiarizationSegment> = Vec::with_capacity(turns.len());
    for t in turns {
        let idx = match label_to_idx.get(&t.speaker) {
            Some(v) => *v,
            None => {
                let v = idx_to_label.len() as i32;
                label_to_idx.insert(t.speaker.clone(), v);
                idx_to_label.push(t.speaker.clone());
                v
            }
        };
        spans.push(DiarizationSegment {
            start: t.start_time as f32,
            end: t.end_time as f32,
            speaker: idx,
        });
    }

    let token_blocks = split_tokens_by_speaker(tokens, &spans);
    if token_blocks.is_empty() || token_blocks[0].start_idx != 0 {
        return None;
    }

    let mut blocks = Vec::with_capacity(token_blocks.len());
    for block in &token_blocks {
        let label = idx_to_label.get(block.speaker as usize)?.clone();
        let text = block.text.clone();
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
    /// The watermark released this block, so no later turn could change what
    /// it displayed (05b D2). A block still undecided at stop is always given
    /// a final revision, because nothing ever confirmed its rendering.
    decided: bool,
}

/// Bounded provisional set of blocks awaiting a decidable turn stream, plus
/// the emit decision shared by submit (block finalization) and reconcile
/// (turn arrival).
pub struct Reconciler {
    held: VecDeque<HeldBlock>,
    /// Blocks the watermark already released, kept only so the stop-time final
    /// pass can compare their rendering against the refined timeline (05b D2).
    /// Never re-attributed against the live turn stream.
    settled: VecDeque<HeldBlock>,
    capacity: usize,
    settled_capacity: usize,
    /// Cluster label -> the name the user gave it during this recording
    /// (05b 3.4). Applied to every rendering, so a re-attribution can never
    /// revert to the label the turn stream was published with.
    bindings: HashMap<String, String>,
}

impl Reconciler {
    pub fn new(capacity: usize) -> Self {
        Self {
            held: VecDeque::new(),
            settled: VecDeque::new(),
            capacity,
            settled_capacity: SETTLED_CAPACITY,
            bindings: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.held.clear();
        self.settled.clear();
        self.bindings.clear();
    }

    /// Stamp the user's name onto every sub-row of a cluster they renamed.
    /// The turns already published carry the name they had at publish time, so
    /// without this a re-attribution would quietly undo the rename.
    fn apply_bindings(&self, blocks: &mut [LiveBlock]) {
        if self.bindings.is_empty() {
            return;
        }
        for block in blocks.iter_mut() {
            if let Some(name) = self.bindings.get(&block.speaker) {
                block.display_name = Some(name.clone());
                block.matched_by = Some("user".to_string());
                block.match_score = None;
            }
        }
    }

    pub fn held_len(&self) -> usize {
        self.held.len()
    }

    /// Blocks eligible for the stop-time final pass: the decided ones plus
    /// whatever is still provisional.
    pub fn tracked_len(&self) -> usize {
        self.settled.len() + self.held.len()
    }

    fn hold(&mut self, held: HeldBlock) {
        self.held.push_back(held);
        while self.held.len() > self.capacity {
            // Drop oldest; it keeps its last emitted rendering (spec fallback)
            // and stays eligible for the final pass, still marked undecided.
            let dropped = self.held.pop_front();
            if let Some(b) = dropped {
                log::warn!(
                    "Live diarization provisional set overflow: released block seq {} (keeps last rendering)",
                    b.parent_sequence_id
                );
                self.settle(b);
            }
        }
    }

    fn settle(&mut self, block: HeldBlock) {
        self.settled.push_back(block);
        while self.settled.len() > self.settled_capacity {
            if let Some(b) = self.settled.pop_front() {
                log::warn!(
                    "Live diarization final-pass set overflow: block seq {} keeps the rendering it was last shown with",
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

        let mut blocks = attribute(&tokens, &source_device, registry);
        if let Some(b) = blocks.as_mut() {
            self.apply_bindings(b);
        }
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
                    is_final: false,
                }),
            ),
            _ => (0, None, None),
        };

        let tracked = HeldBlock {
            parent_sequence_id: update.sequence_id,
            source_device,
            audio_end: block_end,
            tokens,
            revision,
            last,
            decided: decidable,
        };
        if decidable {
            self.settle(tracked);
        } else {
            self.hold(tracked);
        }

        payload
    }

    /// New turns arrived: re-attribute every held block, emitting a new
    /// revision for those whose grouping changed, and release decidable ones.
    pub fn reconcile(&mut self, registry: &LiveTurnRegistry) -> Vec<LiveTranscriptBlocks> {
        let mut out = Vec::new();
        let mut keep = VecDeque::with_capacity(self.held.len());
        while let Some(mut held) = self.held.pop_front() {
            let mut blocks = attribute(&held.tokens, &held.source_device, registry);
            if let Some(b) = blocks.as_mut() {
                self.apply_bindings(b);
            }
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
                        is_final: false,
                    });
                }
            }
            if decidable {
                held.decided = true;
                self.settle(held);
            } else {
                keep.push_back(held);
            }
        }
        self.held = keep;
        out
    }

    /// Stop-time final pass (05b D2). Re-attributes every tracked block
    /// against the refined per-channel timeline and returns a final revision
    /// only for a block that was never decided or whose cluster sequence
    /// changed - the visible churn stays proportional to the actual
    /// correction. `relabel_all` re-emits every block that can be attributed
    /// (05b 3.2).
    ///
    /// Comparison is on the cluster label sequence, not on the boundaries: the
    /// refined timeline almost never reproduces a streaming turn's exact
    /// edges, and a shift that leaves every word with the same speaker is not
    /// something to redraw.
    pub fn finalize(
        &mut self,
        final_turns: &HashMap<String, Vec<LiveTurn>>,
        relabel_all: bool,
    ) -> Vec<LiveTranscriptBlocks> {
        self.finalize_protecting(final_turns, relabel_all, &[])
    }

    /// `finalize`, plus the per-turn overrides whose blocks must be left
    /// exactly as the user set them (05b 3.3).
    pub fn finalize_protecting(
        &mut self,
        final_turns: &HashMap<String, Vec<LiveTurn>>,
        relabel_all: bool,
        protected: &[ProtectedWindow],
    ) -> Vec<LiveTranscriptBlocks> {
        let mut blocks: Vec<HeldBlock> = self.settled.drain(..).collect();
        blocks.extend(self.held.drain(..));
        blocks.sort_by_key(|b| b.parent_sequence_id);

        let mut out = Vec::new();
        let (mut revised, mut unchanged, mut unattributed) = (0usize, 0usize, 0usize);
        let mut user_owned = 0usize;
        for mut block in blocks {
            // A window the user assigned by hand is not a prediction to
            // improve on.
            if protected.iter().any(|w| {
                w.source_device == block.source_device
                    && block.audio_end > w.start
                    && block_start(&block) < w.end
            }) {
                user_owned += 1;
                continue;
            }
            let turns = match final_turns.get(&block.source_device) {
                Some(turns) if !turns.is_empty() => turns,
                // No refined timeline for this channel (it fell back, or it
                // never had speech): the block keeps what it was shown with.
                _ => {
                    unattributed += 1;
                    continue;
                }
            };
            let Some(mut refined) = attribute_with(&block.tokens, turns) else {
                unattributed += 1;
                continue;
            };
            self.apply_bindings(&mut refined);
            // A cluster the user bound carries their name and
            // `matched_by = "user"`. Moving such a span to another speaker
            // would silently undo their correction, so the block keeps what
            // they set - even under `relabel_all`.
            if let Some(previous) = &block.last {
                if would_override_user(previous, &refined) {
                    user_owned += 1;
                    continue;
                }
            }
            let changed = match &block.last {
                Some(previous) => cluster_sequence(previous) != cluster_sequence(&refined),
                None => true,
            };
            if !(relabel_all || changed || !block.decided) {
                unchanged += 1;
                continue;
            }
            revised += 1;
            block.revision += 1;
            out.push(LiveTranscriptBlocks {
                parent_sequence_id: block.parent_sequence_id,
                source_device: block.source_device.clone(),
                revision: block.revision,
                blocks: refined,
                is_final: true,
            });
        }
        log::info!(
            "Live diarization final pass: {} block(s) revised, {} unchanged, {} user-assigned, {} not attributable (relabel_all={})",
            revised,
            unchanged,
            user_owned,
            unattributed,
            relabel_all
        );
        out
    }
}

/// Where a tracked block starts, for overlap tests against a protected
/// window. The block's own span is `[start of its first token, audio_end]`.
fn block_start(block: &HeldBlock) -> f64 {
    block
        .tokens
        .first()
        .map(|t| t.start as f64)
        .unwrap_or(block.audio_end)
}

/// Whether the refined rendering would move a span the user named to another
/// speaker. Only spans they actually decided are protected: an automatic match
/// (`matched_by = "auto"`) is a prediction, and improving it is the point.
fn would_override_user(previous: &[LiveBlock], refined: &[LiveBlock]) -> bool {
    previous
        .iter()
        .filter(|b| b.matched_by.as_deref() == Some("user"))
        .any(|owned| {
            refined.iter().any(|next| {
                next.end > owned.start && next.start < owned.end && next.speaker != owned.speaker
            })
        })
}

impl Reconciler {
    /// A cluster was renamed mid-recording (05b 3.4). Records the name and
    /// re-emits every already-rendered block that shows that cluster, so rows
    /// the user can already see pick up the name immediately instead of
    /// waiting for the next chunk of speech - which, for a speaker who has
    /// stopped talking, may never come.
    pub fn rebind(&mut self, cluster_label: &str, display_name: &str) -> Vec<LiveTranscriptBlocks> {
        self.bindings
            .insert(cluster_label.to_string(), display_name.to_string());
        let mut out = Vec::new();
        for block in self.settled.iter_mut().chain(self.held.iter_mut()) {
            let Some(last) = block.last.as_mut() else {
                continue;
            };
            if !last.iter().any(|b| b.speaker == cluster_label) {
                continue;
            }
            for sub in last.iter_mut() {
                if sub.speaker == cluster_label {
                    sub.display_name = Some(display_name.to_string());
                    sub.matched_by = Some("user".to_string());
                    sub.match_score = None;
                }
            }
            block.revision += 1;
            out.push(LiveTranscriptBlocks {
                parent_sequence_id: block.parent_sequence_id,
                source_device: block.source_device.clone(),
                revision: block.revision,
                blocks: last.clone(),
                is_final: false,
            });
        }
        out.sort_by_key(|p| p.parent_sequence_id);
        log::info!(
            "Live rename of {} to {}: {} already-visible block(s) re-emitted",
            cluster_label,
            display_name,
            out.len()
        );
        out
    }
}

/// The cluster labels a rendering shows, in order - what "the speaker changed"
/// means for a block.
fn cluster_sequence(blocks: &[LiveBlock]) -> Vec<&str> {
    blocks.iter().map(|b| b.speaker.as_str()).collect()
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

/// Revisions produced by a rename, waiting for the cascade consumer to
/// publish them. The consumer owns the app handle, so the correction path
/// hands the payloads over instead of emitting them itself.
static PENDING_REBINDS: Mutex<Vec<LiveTranscriptBlocks>> = Mutex::new(Vec::new());

/// A cluster was renamed while recording (05b 3.4): re-render its
/// already-visible rows and wake the cascade to publish them.
pub fn rebind_cluster(cluster_label: &str, display_name: &str) {
    let registry = registry();
    if !registry.has_any_turns() {
        // Nothing was ever rendered live (Efficient/Off): the rename is
        // applied at stop through the usual persistence path.
        return;
    }
    let rec = reconciler();
    let payloads = {
        let mut guard = rec.lock_or_recover();
        guard.rebind(cluster_label, display_name)
    };
    if payloads.is_empty() {
        return;
    }
    PENDING_REBINDS.lock_or_recover().extend(payloads);
    registry.wake();
}

fn take_pending_rebinds() -> Vec<LiveTranscriptBlocks> {
    std::mem::take(&mut *PENDING_REBINDS.lock_or_recover())
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
    PENDING_REBINDS.lock_or_recover().clear();
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

/// One channel's refined stop-time timeline, in the same label namespace the
/// live turns used (05b D2). The processor hands these over; the identity a
/// label was shown with is joined back in here, from the live turn stream.
#[derive(Debug, Clone, Default)]
pub struct FinalChannel {
    pub source_device: String,
    /// `(start, end, cluster label)`, gap-merged, in recording time.
    pub spans: Vec<(f64, f64, String)>,
}

/// A time window on one channel whose speaker the user set during the
/// recording (a per-turn override). The final pass never revises a block
/// overlapping one (05b 3.3).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectedWindow {
    pub source_device: String,
    pub start: f64,
    pub end: f64,
}

/// What the display pass needs from a finished session.
#[derive(Debug, Clone, Default)]
pub struct FinalDisplayPass {
    pub channels: Vec<FinalChannel>,
    /// Session-resolved `diarizationFinalRelabelAll` (05b 3.2).
    pub relabel_all: bool,
    /// Per-turn overrides recorded during the recording.
    pub protected: Vec<ProtectedWindow>,
}

/// Turn the refined spans into turns carrying the identity their label was
/// displayed with, so a final revision never silently drops a name the user
/// already saw. A label with no live counterpart carries no identity and is
/// resolved by the frontend's own binding/override layer.
fn final_turns_with_identity(
    channels: &[FinalChannel],
    registry: &LiveTurnRegistry,
) -> HashMap<String, Vec<LiveTurn>> {
    let mut out: HashMap<String, Vec<LiveTurn>> = HashMap::new();
    for channel in channels {
        let live = registry.turns(&channel.source_device);
        let turns = channel
            .spans
            .iter()
            .map(|(start, end, label)| {
                let prov = provenance(live.iter(), label, *start, *end);
                LiveTurn {
                    start_time: *start,
                    end_time: *end,
                    speaker: label.clone(),
                    source_device: channel.source_device.clone(),
                    display_name: prov.and_then(|t| t.display_name.clone()),
                    matched_by: prov.and_then(|t| t.matched_by.clone()),
                    match_score: prov.and_then(|t| t.match_score),
                }
            })
            .collect();
        out.insert(channel.source_device.clone(), turns);
    }
    out
}

/// Publish the stop-time final revisions (05b D2). Called once per session,
/// after the refined timeline exists and before the reconcile cascade is
/// closed. Display-only, like the rest of this module: persistence keeps
/// following `finalize`'s own assignments.
pub fn finalize_session<R: Runtime>(app: &AppHandle<R>, pass: &FinalDisplayPass) {
    let registry = registry();
    if !registry.has_any_turns() {
        // Nothing was ever displayed (Efficient/Off, or a session with no
        // speech): there is no provisional rendering to promote.
        return;
    }
    let final_turns = final_turns_with_identity(&pass.channels, &registry);
    let relabel_all = pass.relabel_all;
    let rec = reconciler();
    let payloads = {
        let mut r = match rec.lock() {
            Ok(r) => r,
            Err(poisoned) => poisoned.into_inner(),
        };
        r.finalize_protecting(&final_turns, relabel_all, &pass.protected)
    };
    for payload in payloads {
        emit(app, &payload);
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
            for p in payloads.into_iter().chain(take_pending_rebinds()) {
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

    // ===== Stop-time promotion (05b D2, tasks 3.1/3.2) =====

    /// One channel's refined timeline, as the processor hands it over.
    fn final_turns(device: &str, spans: &[(f64, f64, &str)]) -> HashMap<String, Vec<LiveTurn>> {
        let mut map = HashMap::new();
        map.insert(
            device.to_string(),
            spans
                .iter()
                .map(|(start, end, label)| turn(label, *start, *end, device))
                .collect(),
        );
        map
    }

    fn tokens_over(words: &[(&str, f32, f32)]) -> Vec<Token> {
        words.iter().map(|(t, s, e)| token(t, *s, *e)).collect()
    }

    /// A settled block whose refined cluster sequence is what it already
    /// displayed must not be redrawn: the live view is a reading surface, and
    /// a revision that changes nothing visible still looks like one.
    #[test]
    fn an_unchanged_block_gets_no_final_revision() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        // A later turn makes the block decidable, so it settles on submit.
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        let payload = rec
            .submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("hello", 0.1, 0.6), (" there", 0.7, 1.4)])),
                &registry,
            )
            .expect("the live rendering is emitted on submit");
        assert!(!payload.is_final, "a revision published while recording is provisional");
        assert_eq!(payload.revision, 1);
        assert_eq!(rec.held_len(), 0, "a decidable block settles immediately");
        assert_eq!(rec.tracked_len(), 1, "and stays eligible for the final pass");

        // The refined timeline groups those words the same way.
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_00")]);
        assert!(
            rec.finalize(&refined, false).is_empty(),
            "nothing changed, so nothing is re-emitted"
        );
    }

    /// A block the refinement moves to another speaker gets exactly one final
    /// revision, marked final, above the revision it last displayed.
    #[test]
    fn a_changed_block_gets_exactly_one_final_revision() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        let live = rec
            .submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("hello", 0.1, 0.6), (" there", 0.7, 1.4)])),
                &registry,
            )
            .expect("live rendering");
        assert_eq!(live.blocks.len(), 1);
        assert_eq!(live.blocks[0].speaker, "SPEAKER_00");

        // The refinement decided those two seconds belong to another cluster.
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_01")]);
        let out = rec.finalize(&refined, false);
        assert_eq!(out.len(), 1, "exactly one final revision");
        assert!(out[0].is_final);
        assert!(
            out[0].revision > live.revision,
            "the final revision has to outrank the last provisional one or the frontend drops it"
        );
        assert_eq!(out[0].parent_sequence_id, 1);
        assert_eq!(out[0].blocks.len(), 1);
        assert_eq!(out[0].blocks[0].speaker, "SPEAKER_01");
        assert_eq!(out[0].blocks[0].text, "hello there");
        // The pass consumed the tracked blocks: a second call re-emits nothing.
        assert!(rec.finalize(&refined, false).is_empty());
    }

    /// A block the watermark never released was never confirmed, so it is
    /// finalized even when the refined labels match what it showed.
    #[test]
    fn a_block_still_provisional_at_stop_is_always_finalized() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        let live = rec
            .submit(
                &update(7, "Microphone", 0.0, 2.0, tokens_over(&[("still", 0.2, 0.8), ("open", 0.9, 1.5)])),
                &registry,
            )
            .expect("live rendering");
        assert_eq!(rec.held_len(), 1, "no later turn, so the block is still held");

        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_00")]);
        let out = rec.finalize(&refined, false);
        assert_eq!(out.len(), 1, "an undecided block is confirmed at stop");
        assert!(out[0].is_final);
        assert_eq!(out[0].revision, live.revision + 1);
        assert_eq!(out[0].blocks[0].speaker, "SPEAKER_00");
    }

    /// Task 3.2: the opt-in setting re-emits every attributable block, while
    /// the default leaves the unchanged ones alone.
    #[test]
    fn relabel_all_re_emits_every_block_and_the_default_does_not() {
        let build = || {
            let registry = LiveTurnRegistry::new();
            registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
            registry.publish(turn("SPEAKER_01", 2.0, 4.0, "Microphone"));
            registry.publish(turn("SPEAKER_01", 4.0, 6.0, "Microphone"));
            let mut rec = Reconciler::new(8);
            rec.submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("first", 0.1, 0.9)])),
                &registry,
            );
            rec.submit(
                &update(2, "Microphone", 2.0, 4.0, tokens_over(&[("second", 2.1, 2.9)])),
                &registry,
            );
            rec
        };
        // Refined: block 1 keeps SPEAKER_00, block 2 moves to SPEAKER_02.
        let refined = final_turns(
            "Microphone",
            &[(0.0, 2.0, "SPEAKER_00"), (2.0, 4.0, "SPEAKER_02")],
        );

        let narrow = build().finalize(&refined, false);
        assert_eq!(narrow.len(), 1, "only the block whose speaker changed");
        assert_eq!(narrow[0].parent_sequence_id, 2);

        let wholesale = build().finalize(&refined, true);
        assert_eq!(wholesale.len(), 2, "the opt-in re-emits both");
        assert!(wholesale.iter().all(|p| p.is_final));
        assert_eq!(
            wholesale.iter().map(|p| p.parent_sequence_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    /// Task 3.4: renaming a cluster updates the rows already on screen, even
    /// when that speaker never talks again (the case where waiting for the
    /// next chunk means waiting forever).
    #[test]
    fn a_rename_re_emits_the_rows_already_on_screen() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        registry.publish(turn("SPEAKER_01", 2.0, 4.0, "Microphone"));
        registry.publish(turn("SPEAKER_01", 4.0, 6.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        let first = rec
            .submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("hello", 0.1, 0.9)])),
                &registry,
            )
            .expect("live rendering");
        let second = rec
            .submit(
                &update(2, "Microphone", 2.0, 4.0, tokens_over(&[("other", 2.1, 2.9)])),
                &registry,
            )
            .expect("live rendering");
        assert_eq!(first.blocks[0].display_name, None);

        // No further speech arrives - only the rename.
        let out = rec.rebind("SPEAKER_00", "Anna");
        assert_eq!(out.len(), 1, "only the block showing that cluster");
        assert_eq!(out[0].parent_sequence_id, 1);
        assert_eq!(out[0].revision, first.revision + 1);
        assert!(!out[0].is_final, "a rename mid-recording is still provisional");
        assert_eq!(out[0].blocks[0].display_name.as_deref(), Some("Anna"));
        assert_eq!(out[0].blocks[0].matched_by.as_deref(), Some("user"));
        // The other cluster's block is untouched.
        assert_eq!(second.blocks[0].display_name, None);
    }

    /// The name has to survive a later re-attribution: the turns already in
    /// the registry still carry the label they were published with.
    #[test]
    fn a_rename_survives_the_next_reconcile_pass() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        rec.submit(
            &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("hello", 0.1, 0.9)])),
            &registry,
        );
        assert_eq!(rec.rebind("SPEAKER_00", "Anna").len(), 1);

        // A later turn arrives and the held block is re-attributed against the
        // registry, whose turns never learned the new name.
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        for payload in rec.reconcile(&registry) {
            for block in &payload.blocks {
                if block.speaker == "SPEAKER_00" {
                    assert_eq!(block.display_name.as_deref(), Some("Anna"));
                }
            }
        }
        // And the stop-time pass keeps it too.
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_00")]);
        for payload in rec.finalize(&refined, true) {
            for block in &payload.blocks {
                assert_eq!(block.display_name.as_deref(), Some("Anna"));
                assert_eq!(block.matched_by.as_deref(), Some("user"));
            }
        }
    }

    /// Task 3.3: a cluster the user bound keeps their name and their
    /// `matched_by`, even when the refinement would put another speaker there.
    #[test]
    fn a_user_bound_span_is_never_moved_by_the_final_pass() {
        let registry = LiveTurnRegistry::new();
        let mut bound = turn("SPEAKER_00", 0.0, 2.0, "Microphone");
        bound.display_name = Some("Anna".to_string());
        bound.matched_by = Some("user".to_string());
        registry.publish(bound);
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        let live = rec
            .submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("anna", 0.1, 0.9)])),
                &registry,
            )
            .expect("live rendering");
        assert_eq!(live.blocks[0].display_name.as_deref(), Some("Anna"));
        assert_eq!(live.blocks[0].matched_by.as_deref(), Some("user"));

        // The refinement decided those seconds belong to another cluster.
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_01")]);
        assert!(
            rec.finalize(&refined, false).is_empty(),
            "the user's assignment is not a prediction to improve on"
        );
        // Not even the wholesale opt-in may undo it.
        assert!(rec.finalize(&refined, true).is_empty());
    }

    /// The protection is narrow on purpose: an automatic match is a
    /// prediction, and correcting it is the whole point of the pass.
    #[test]
    fn an_automatic_match_is_still_corrected() {
        let registry = LiveTurnRegistry::new();
        let mut guessed = turn("SPEAKER_00", 0.0, 2.0, "Microphone");
        guessed.display_name = Some("Greg".to_string());
        guessed.matched_by = Some("auto".to_string());
        guessed.match_score = Some(0.71);
        registry.publish(guessed);
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        let mut rec = Reconciler::new(8);
        rec.submit(
            &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("greg", 0.1, 0.9)])),
            &registry,
        );
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_01")]);
        let out = rec.finalize(&refined, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].blocks[0].speaker, "SPEAKER_01");
    }

    /// Task 3.3: a per-turn override protects its window, whatever the
    /// refinement says about it.
    #[test]
    fn a_per_turn_override_window_is_never_revised() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "Microphone"));
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "Microphone"));
        // The pass runs once per session and consumes its tracked blocks, so
        // each scenario gets its own reconciler.
        let build = || {
            let mut rec = Reconciler::new(8);
            rec.submit(
                &update(1, "Microphone", 0.0, 2.0, tokens_over(&[("mine", 0.1, 0.9)])),
                &registry,
            );
            rec
        };
        let refined = final_turns("Microphone", &[(0.0, 2.0, "SPEAKER_01")]);
        let protected = vec![ProtectedWindow {
            source_device: "Microphone".to_string(),
            start: 0.5,
            end: 1.5,
        }];
        assert!(
            build()
                .finalize_protecting(&refined, true, &protected)
                .is_empty(),
            "the block overlaps a window the user assigned by hand"
        );

        // The same override on the other channel protects nothing here.
        let other = vec![ProtectedWindow {
            source_device: "System".to_string(),
            start: 0.5,
            end: 1.5,
        }];
        assert_eq!(
            build().finalize_protecting(&refined, true, &other).len(),
            1
        );
    }

    /// A channel that fell back to its incremental identities, or never had
    /// speech, has no refined timeline: its blocks keep what they displayed
    /// rather than being cleared.
    #[test]
    fn a_channel_without_a_refined_timeline_leaves_its_blocks_alone() {
        let registry = LiveTurnRegistry::new();
        registry.publish(turn("SPEAKER_00", 0.0, 2.0, "System"));
        registry.publish(turn("SPEAKER_00", 2.0, 4.0, "System"));
        let mut rec = Reconciler::new(8);
        rec.submit(
            &update(1, "System", 0.0, 2.0, tokens_over(&[("system", 0.1, 0.9)])),
            &registry,
        );
        // The map carries the microphone only, and an empty system timeline.
        let mut refined = final_turns("Microphone", &[(0.0, 2.0, "MIC_SPEAKER_00")]);
        refined.insert("System".to_string(), Vec::new());
        assert!(rec.finalize(&refined, true).is_empty());
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
