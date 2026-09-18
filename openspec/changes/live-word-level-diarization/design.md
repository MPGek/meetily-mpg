# Design: live-word-level-diarization

## Context

Live Fast-mode diarization already produces: (a) finalized transcript blocks carrying word tokens, refined live by CTC alignment (`audio/word_alignment/queue.rs` re-emits `transcript-update`); (b) stable speaker turns emitted to the frontend from the Fast-mode polyvoice pipeline (`online_diarization.rs` stable-turn branch) and buffered per channel (`channel.turns`) for stop-time finalize. Nothing joins the two live: the frontend (`lib/live-speaker-labels.ts`) labels whole segments by max overlap, and the dedupe-by-`sequence_id` frontend transcript listener drops the alignment-queue re-emits (refined tokens reach only `SHARED_SEGMENTS` and incremental disk, not the rendered view). Token→speaker assignment exists as a shared pure function (`token_assignment.rs::assign_tokens_to_speakers`) used at stop-time (`online_diarization.rs`) and offline (`diarization.rs`).

Live spec constraints: labels are display-only until stop (`live-speaker-labels`), stop-time assignment is authoritative and token-split there is spec'd (`online-speaker-diarization` "Stop-time assignment uses token-level refinement").

## Goals / Non-Goals

**Goals:**
- Live word-level speaker attribution + N-way live splitting of finalized blocks, Fast mode only.
- Handle the turn-stability latency: a block finalizes before its covering turn is stable.
- Zero change to stop-time finalize, offline diarization, persistence paths, or Efficient mode.
- Single source of truth for the split algorithm (Rust, reuse `assign_tokens_to_speakers`).

**Non-Goals:**
- No TS re-implementation of `group_into_blocks`; no frontend-computed split from tokens+turns.
- No mid-recording `SHARED_SEGMENTS` / incremental-file / DB mutation by the split.
- No live splitting in Efficient/Off modes; no changes to per-turn rename semantics beyond override carry-over (delta spec).
- Not fixing the frontend refined-token display gap directly; the design sidesteps it because boundaries are computed backend-side (noted as a separate latent issue).

## Decisions

### D1: Backend view-model event, not backend persistence split, not frontend split

Three options were considered:
- **A. Backend split writes into `SHARED_SEGMENTS`/persistence.** Rejected: live and stop-time splitters would compound (children re-split at finalize), duplicated rows, and mid-recording mutation contradicts "display-only until stop".
- **B. Frontend computes split from tokens+turns.** Rejected: requires fixing the re-emit drop (displayed tokens are ASR, not refined), duplicates the ≥2-token split algorithm in TS, risks display/persistence divergence.
- **C (chosen).** Backend computes the split from refined tokens × a shared live-turn registry and emits a display-only `live-transcript-blocks` event; persistence paths untouched. Aligns with "Live labels are display-only until stop", reuses the Rust algorithm with zero duplication, and stop-time finalize independently reproduces the same rows for saving.

### D2: Reconcile consumer folded into the post-finalize alignment path

The block-side pipeline already has: fresh final blocks, the CTC-refined tokens (or ASR fallback tokens), a bounded ownership queue with drop-oldest, and a close/drain-on-stop contract. The reconcile stage reuses that shape (new `audio/live_diariation_reconcile` module, sibling to `word_alignment/queue.rs`), run **downstream of** token refinement — word-true timestamps are what turn attribution consumes.

```
transcription worker ──final──> transcript-update
                                      │
                          align (CTC, optional) ── refined tokens
                                      │
                                      v
                 reconcile consumer: tokens × LiveTurnRegistry
                       ├─ decidable now ──> emit live-transcript-blocks (revision N)
                       ├─ tail not covered ──> provisional map (bounded, drop-oldest)
                       └─ on registry Notify: re-evaluate held blocks
```

Alternative (a separate consumer subscribed to turn events plus a transcript cache) was rejected: it would re-read token state from `SHARED_SEGMENTS` rather than from the emitted update and add a second ordering problem. Keeping tokens in the same job payload avoids that.

**Implementation note (during apply).** The alignment queue was previously created only when alignment was enabled, so the reconcile stage would have had no path when alignment was off — but the spec requires attribution on the engine's own timestamps in exactly that case. The queue/consumer are therefore now always created for a recording; when alignment is disabled or its model is missing the consumer skips refinement and still hands the block (with ASR tokens) to the reconcile stage. No behavior change for the alignment feature itself.

### D3: LiveTurnRegistry — authoritative shared turn stream

`Arc<Mutex<Vec<LiveTurn>>>` per channel, appended by the Fast-mode stable-turn branch in `online_diarization.rs` next to the existing `channel.turns` push (times are already absolute via `mapper.to_abs`; entries carry `turn_label`, `display_name`, `matched_by`, `match_score`). The processor also bumps `Notify` so the reconcile consumer can wake and re-evaluate held blocks. The existing `online-speaker-turn` emission path is unchanged (frontend keeps its source of turns).

Registry state carries, per channel, `last_turn_end` — a monotonic watermark used by D4.

### D4: Watermark = "pipeline moved past the block"

Decidable ⟺ ∃ stable turn `T` (same channel) with `T.start >= block.end`. Stable turns are append-only and time-ordered within a channel, so once a turn starts after the block's end, no future turn can overlap it — waiting longer cannot change the outcome. This yields at most one stability-window of dwell per block and avoids timeout heuristics. Held blocks are stored keyed by `(sequence_id)` with their current best rendering and revision counter; released blocks on overflow keep the last emitted revision (spec ladder).

Spike risk: this assumes monotonicity (see Risks). If `StreamingPipeline` revises emitted turns, the watermark becomes conservative-safe but may under-split until the next turn; the spike task verifies the assumption before/with implementation.

### D5: Sub-row identity = parent sequence_id + revision; no persistence coupling

The event payload carries `parent_sequence_id`, `source_device`, `revision`, and `blocks: [{ start, end, text (token slice), speaker (cluster label), display_name, matched_by, match_score }]`. Children do **not** get their own `sequence_id`s and never enter `SHARED_SEGMENTS`; the frontend holds the latest revision per parent and renders sub-rows beneath the segment. Ordering in the saved transcript remains stop-time's job (which re-splits the parent with the same tokens). This eliminates the double-split hazard by construction.

### D6: Provenance and pinned labels ride the emission

Sub-row `speaker` is the raw cluster label; `display_name`/`matched_by`/`match_score` are resolved from the turn stream (which already resolves user bindings + auto-recognition at turn-emit time — `online_diarization.rs` stable-turn branch). A user binding made *after* a sub-row was emitted flows via the existing frontend rewrite (`rewriteTurnsForBinding`) on turn/binding updates, now applied to sub-rows carrying that cluster label. The `live-speaker-labels` delta adds: single-turn overrides re-key by time window onto the covering sub-row.

### D7: Only Fast mode; Efficient/Off bypass wholesale

The reconcile consumer activates only when the online processor published at least one registry turn for a channel. Efficient/Off produce no registry entries → no event, no provisional holding; existing segment-level flow and stop-time finalize unchanged.

### D8: Sub-rows render inside the parent block surface, channel-aware

Two decisions about the `blocks.length > 1` rendering branch:

1. **Surface: the split never replaces the parent row.** The block keeps the parent record's row shape — background bubble, border, rounded corners, active highlight — and the sub-rows render inside it, one labeled run each. Alternatives considered: (a) one bubble per run — rejected, because two adjacent runs of the same record become indistinguishable from two separate records and each live revision re-emit would visually re-split/re-join blocks; (b) parent bubble plus sub-rows printed beneath it on the page background — rejected, because it drops the "one record" reading and leaves half the text visually unattributed, which is exactly the reported defect ("all transcript blocks must have a background").
2. **Side: the side comes from the parent block's `source_device`**, never from the sub-row's cluster label, and ordering per channel mirrors the single-record rows of `split-transcript-ui` (Microphone: timestamp, play, label, text on the left; System mirrored on the right). The rule lives in a small pure helper (`frontend/src/lib/source-side-layout.ts`) with unit tests, so it cannot drift from the variants. This rule was previously shared through the turn-grouping helper; `drop-turn-grouping` deletes that module, so the rule is re-established here as part of the sub-row rendering.

## Risks / Trade-offs

- [polyvoice turns may not be strictly append-only/monotonic] → Spike: log emitted stable turn (start, end, speaker) sequence over real recordings and assert monotonic non-overlap; if revisions exist, degrade D4 to "decidable only on next confirmed-later turn" (never regresses, only delays splits). Mitigated anyway because a wrong attribution self-corrects at stop-time finalize.
- [Turn stability delay may be seconds, making live split feel late] → Sub-rows appear when decidable; until then the block shows its current rendering. Measure dwell distribution in the spike; if long, consider emitting "coverage-suggestive" provisional split only between tokens whose attribution gaps are unambiguous — deferred as an option, not in scope.
- [Split boundary mid-utterance yields odd sub-row text] → Same ≥2-token validation as stop-time; boundary text is a token slice, consistent with what persistence will do.
- [Extra per-block attribution cost] → Pure CPU over ≤25s of tokens + bounded turns; negligible next to CTC inference.
- [Frontend thrash across revisions] → Revision counter + replace-per-parent rendering; scroll/render rules mirror the existing in-place relabel constraint (`live-speaker-labels` "In-place live relabel").

## Migration Plan

Purely additive: new registry + consumer + event + frontend sub-row rendering, keyed off existing Flow events. Rollback = disable the reconcile consumer (single static gate), everything reverts to today's behavior. No schema or DB migration.

## Open Questions

- None blocking the spec/tasks. The turn-monotonicity spike (D4/Risks) is included as the first implementation task; its outcome caps scope only if turns turn out to be revisable (then splits simply delay to next-turn).
