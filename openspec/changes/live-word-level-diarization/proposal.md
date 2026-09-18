## Why

During recording, speaker labeling is currently segment-level only (`rematchTranscripts` matches a whole transcript block to the best-overlapping speaker turn), while word-level (token-level) speaker attribution exists only at recording stop and offline. When a transcript block spans a speaker change, the live view shows one row under one speaker until stop, which contradicts the word-true boundaries the app already produces live (CTC token refinement re-emits during recording).

## What Changes

- Add a **live word-level diarization reconcile** stage in the Rust backend: finalized transcript blocks (with live CTC-refined or ASR word tokens) are attributed to speakers per token against live stable turns from the Fast-mode online diarizer, and a block spanning multiple speakers is split into several live display rows (one per contiguous speaker block, ≥2-token boundary rule reused from stop-time `assign_tokens_to_speakers`).
- Add a per-channel **LiveTurnRegistry** shared between the online diarization processor and the reconcile stage, plus a **watermark decision rule** (a block is decidable once a stable turn starts after its end) that parks not-yet-decidable blocks in a bounded provisional set and re-decides as turns arrive.
- Emit a new display-only `live-transcript-blocks` event carrying per-block `{start, end, text, speaker, display_name, matched_by, match_score}`; the frontend renders split sub-rows under the segment. **No persistence during recording**: `SHARED_SEGMENTS`, incremental transcripts, and the database are untouched by live splits; stop-time finalize remains the authoritative splitter.
- Render live split sub-rows inside their parent block: the record keeps its background bubble, border, rounded corners, and active highlight, with one labeled sub-row per speaker run inside that surface (never bare text on the page background), laid out on the parent block's channel side (Microphone left / System right, `split-transcript-ui` cues for alignment, label/dot order, and timestamp side) regardless of the sub-row's cluster label.
- Degradation ladder with silent fallbacks (alignment off → ASR tokens; no live turns → current segment-level labels; overflow → segment-level label).
- Scope: **Fast mode only** (Efficient/Off have no live turns; their behavior is unchanged).

## Capabilities

### New Capabilities
- `live-word-diarization`: Live token-level speaker attribution and N-way live splitting of finalized transcript blocks in Fast mode, via a display-only blocks event, a shared live-turn registry, watermark-based reconciliation, and silent degradation.

### Modified Capabilities
- `live-speaker-labels`: Per-turn speaker overrides and pinned user labels must survive live splits (override applies by time window to the covering sub-block; user-assigned labels stay pinned across sub-row re-renders).

## Impact

- Rust: `audio/online_diarization.rs` (publish stable turns to registry; no stop-time logic change), new `audio/live_diariation_reconcile` consumer co-located with `audio/word_alignment/queue.rs` post-finalize path, `audio/token_assignment.rs` (reused, no algorithm change), `recording_commands.rs` (event wiring).
- Frontend: `TranscriptContext.tsx` / recording transcript rendering (sub-row display + replace-semantics on the new event; turn listener unchanged), `lib/live-speaker-labels.ts` (override application by time window over sub-rows), `lib/source-side-layout.ts` (new pure helper carrying the channel side rule used by the sub-row branch).
- No schema, IPC, or database changes. No changes to stop-time finalize, offline diarization, Efficient mode, or the CTC alignment queue contract.
