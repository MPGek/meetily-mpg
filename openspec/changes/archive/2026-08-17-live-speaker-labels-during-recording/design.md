## Context

Online diarization currently assigns speaker labels only at recording stop. The Fast mode already runs polyvoice's `StreamingPipeline` continuously, but its stable turns are buffered in `FastChannel.turns` (`audio/online_diarization.rs`) and only translated + matched against transcripts in `finalize()` during `stop_recording`. Transcription, meanwhile, emits `transcript-update` events live (`transcription/worker.rs`) carrying `sequence_id`, `audio_start_time`, `audio_end_time`, and `source_device`.

Two architectural facts shape the design:

- The diarization processor runs in a `spawn_blocking` loop and only ever sees `AudioChunk`s — it has no `sequence_id`s. Transcription and diarization are separate tasks with no shared segment identity.
- Tauri v2 `AppHandle::emit` is synchronous, and `AppHandle` is `Send + Clone`, so events can be emitted directly from the blocking consumer loop.

The frontend already has all the display machinery: `VirtualizedTranscriptView` renders speaker dots/labels when `speaker` is set, and both `Transcript` and `TranscriptUpdate` already carry `speaker?`. The only gap on the display side is that the recording-page panel (`app/_components/TranscriptPanel.tsx`) drops `speaker` when mapping segments, and nothing populates it live.

## Goals / Non-Goals

**Goals:**

- Emit stable speaker turns live (Fast mode) and show speaker labels on the recording page as they become available.
- Retroactively fill in labels on segments that rendered before their turn went stable.
- Keep the stop-time `recording-stopped` → `speaker_assignments` path as the authoritative, persisted result.

**Non-Goals:**

- Live labels for Efficient mode (it stays batch: embed during recording, cluster at stop).
- Making online labels match offline diarization exactly (arrival-order IDs are accepted as best-effort).
- Streaming user-assigned `speaker_label` names live.
- Any database schema change or new dependency.

## Decisions

### Decision 1: Emit time-ranged speaker turns, match on the frontend

Rather than annotating `TranscriptUpdate.speaker` in the backend, the Fast path emits a new `online-speaker-turn` event `{ start_time, end_time, speaker, source_device }`, and the frontend matches turns to transcript segments by temporal overlap.

**Rationale:** the processor has no access to `sequence_id`s, and introducing a shared sequence↔time map plus cross-task coordination would couple transcription and diarization. Time-overlap matching on the frontend mirrors exactly what `finalize()`/`find_best_speaker` already does, just incrementally.

**Alternatives considered:** (a) annotate `transcript-update` in Rust — rejected for the coupling above; (b) emit only final assignments at stop — does not satisfy "live".

### Decision 2: Port `find_best_speaker` overlap logic to TypeScript and recompute reactively

The frontend keeps emitted turns per channel and, on each new turn or new transcript, recomputes the best-overlap speaker for affected segments. React state keyed by `sequence_id` makes retroactive fill-in natural.

**Rationale:** both a transcript and its turn can arrive in either order; recomputation is the simplest correct model.

### Decision 3: Thread `AppHandle` into the online diarization processor

`OnlineDiarizationProcessor` (or just the Fast `process_chunk` path) receives an `AppHandle` and emits directly. `recording_commands.rs` already clones `app_for_event` for the error path; it will pass a clone into the processor.

**Rationale:** `emit` is synchronous and `AppHandle` is `Send + Clone`; no extra channel/async hop needed. **Alternative:** send turns back over a channel to an async task — more plumbing for no benefit.

### Decision 4: Scope live labels to Fast mode only

Efficient mode remains unchanged (buffer embeddings, AHC cluster at stop).

**Rationale:** live Efficient would require periodic re-clustering of a growing buffer plus a label-stabilization pass (AHC renumbers clusters between runs, causing flicker) and carries O(n²)+ cost. It's a separate, harder project. See Open Questions.

### Decision 5: Live IDs are best-effort; stop-time IDs are authoritative

Live labels use the same channel-scoped scheme (`MIC_SPEAKER_NN` / `SPEAKER_NN`) so they look consistent, but arrival-order speaker IDs may not match the final stop-time labels. The stop-time pass overwrites `speaker` in the DB via `sequence_id`, so any drift is transient and self-correcting.

## Risks / Trade-offs

- **[Arrival-order IDs may not match offline/final labels]** → Accept as best-effort; the stop-time pass is the persisted source of truth and overwrites transient labels.
- **[Timing skew: a transcript renders before its turn is stable]** → Retroactive matching updates the label a moment later; UI already re-renders by `sequence_id`.
- **[Event/re-render churn on busy meetings]** → Events are emitted only for *stable* turns (not every chunk); matching is O(turns × transcripts) and bounded per recording; debounce `setTranscripts` if profiling shows jank.
- **[Processor error state mid-recording stops live emission]** → Matches existing behavior (engine set to `None`); `finalize` returns no assignments and the offline fallback path is unaffected.
- **[`source_device` used for channel matching on the frontend]** → The frontend must apply the same `System` → `SPEAKER_NN` vs mic → `MIC_SPEAKER_NN` routing as `finalize()`, including the no-system-audio case; a mismatch would mislabel channels.

## Migration Plan

- No data migration. New event is additive; the stop-time path is untouched.
- Rollback: remove the `online-speaker-turn` listener and revert the `TranscriptPanel` segment mapping; the backend emit becomes a no-op without a listener.

## Open Questions

- Should live IDs be stabilized to match final stop-time IDs (e.g., re-indexing speaker cache by cluster order)? Deferred — Fast arrival-order accepted for v1.
- Should Efficient mode later gain live labels via incremental re-clustering + label stabilization? Deferred — future change.
