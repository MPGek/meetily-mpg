## Context

See `proposal.md` for motivation. The constraints that shape the approach:

- **Live diarization state is unreachable during a recording.** The online diarization processor is moved into a `spawn_blocking` closure and only returned when the embedding channel closes at stop (`recording_commands.rs:655-682`). Its per-channel buffers, counters, and engine handle are not shared.
- **Three surfaces are already shared and readable during a recording:** the prototype store behind a process-level `Arc<RwLock<...>>`, the live turn registry and reconciler behind `OnceLock<Arc<...>>` (`live_diarization_reconcile.rs:454-469`, with `turns(channel)` and `held_len()` accessors), and the model-readiness check that already resolves the enhanced model set.
- **The only live model is TitaNet-Large** (192-d, tag `titanet_large`); segmentation is offline-only and Silero VAD belongs to the pipeline, not diarization. Acceptance thresholds are constants: clustering `0.60` (stop-time only) and recognition `0.68` (live).
- **The hot path is performance-sensitive**: chunk processing deliberately avoids per-chunk logging and event emission.
- **The placement target is narrow.** The recording controls are a `flex` pill inside a `w-2/3 max-w-[750px]` container (`page.tsx:246-263`) that already holds pause, stop, and the animated bars (`RecordingControls.tsx:474-486`). The bars are driven by a random-height interval in the parent (`page.tsx:179-193`), not by real audio.

## Goals / Non-Goals

**Goals:**
- Two lines, one per channel, that answer four questions live: is this channel embedding at all, is data accumulating, who was the last speaker and how confident, and is the channel's turn timeline still sane.
- Reuse existing shared state instead of duplicating counters.
- Sample on an interval so the audio path is untouched.
- Make the display honest about the three states that would otherwise look like faults (mono session, Efficient deferral, no speech yet).

**Non-Goals:**
- No offline/finalize diarization telemetry, no charts, history, or persisted logs.
- No change to diarization behavior, thresholds, or clustering.
- No live clustering for Efficient mode — it remains stop-time by design.
- Not fixing the animated bars (they remain a placeholder animation); this change only avoids implying they carry real data.
- No per-chunk event emission and no new sampling timer: every subsystem publishes into shared counters, and the frontend reads them on the interval it already has.

**Scope extension (user decision, same change).** The original non-goal excluding pipeline and other-engine telemetry was lifted: the surface must now also show the buffers that gate pipeline operations and the activity of every model kind in use (voice activity, speech recognition, word alignment, speaker diarization). The refresh interval is tightened from 300 ms to 150 ms on the existing timer.

## Decisions

**1. Pull-based snapshot on an interval, not a per-chunk event stream.**
The UI reads a compact snapshot from a read-only command, refreshed on a fixed interval; nothing is emitted per chunk.
*Alternatives:* emitting a diarization event per chunk (rejected — violates the hot-path constraint and floods the bridge); scraping logs (rejected — brittle, already sparse and gated behind debug flags); extending the existing fake `audio-levels` event (rejected — that pipeline is a separate, input-only stream and its data is synthetic).

**2. Share a stats handle instead of moving the processor into a shared lock.**
A per-session stats handle (cheap atomic counters plus a small memo for the last turn per channel) is created when the recording starts and handed to the processor, which updates it inside its chunk path. The processor's ownership and its stop-time return are unchanged.
*Alternatives:* holding the processor behind a shared mutex (rejected — the chunk path takes `&mut self` on a blocking thread, so per-chunk locking adds contention and forces rework of the stop-time handoff); a second channel that mirrors the processor's state (rejected — duplicated state that can drift and doubles the hot-path work).

**3. Reuse the already-shared surfaces.**
Last-turn data comes from the turn registry, prototype and binding counts from the prototype store, model readiness from the existing model check, and the threshold/dimension from the model constants. Only per-channel counters (chunks, embedding outcomes, buffered embeddings, turn count) and the turn-order flag accessor are new.
*Alternatives:* re-deriving everything in one new module (rejected — two sources of truth for the same numbers).

**4. Exactly two lines; global context behind a compact prefix and hover.**
Keeping the visible block at two lines respects the requested shape, and the mode, model identifier, dimension, recognition threshold, and prototype/binding counts are exposed in the block's tooltip so scores remain interpretable.
*Alternatives:* a third header line (rejected — the request was explicitly two lines); no global context (rejected — a match score without its threshold is unreadable).

**5. Compact token text with truncation.**
Labels are short tokens and long display names are truncated, because the pill is capped in width and already holds controls and bars.
*Alternatives:* full sentences per line (rejected — would overflow the container and wrap the pill).

**6. An explicit per-channel display state.**
Each channel resolves to one of: unavailable (no diarization session), inactive (no system device — mono), deferred (Efficient), accumulating, healthy, warning (turn order), error (embedding stopped). The text and color follow the state, so zeros are never rendered as failures.
*Alternatives:* raw numbers only (rejected — that is precisely the misreading the change exists to prevent).

**7. Session-scoped lifecycle.**
The stats handle is created at recording start and reset with the rest of the live-diarization session state; the block is hidden when not recording and when the mode is Off.

**8. One snapshot command with three sections, one publisher per subsystem.**
The status command returns `diarization`, `pipeline` (per channel) and `models`, assembled from the owners that already exist rather than from a new central service: the pipeline publishes its own buffer fills and voice-activity activity (it is the only writer of those queued samples and the only owner of the VAD sessions), the transcription worker publishes its queue counters, the alignment consumer publishes its queue and engine state, and diarization keeps the counters it already publishes. The command reads; it never computes pipeline state itself.
*Alternatives:* computing fills by re-deriving pipeline state in the command (rejected — the command cannot see the pipeline's queued samples, and a second derivation would drift); a central telemetry service with its own channels (rejected — duplicates state the pipeline and workers already hold).

**9. Fill is reported as current over threshold, with the gated operation named.**
A buffer's fill is meaningful only relative to what it triggers, so each gated buffer publishes its current sample count and its threshold together, and the line renders a percentage plus a short label for the operation. Thresholds are the existing code constants (voice-activity dispatch window, pending merge window and cap, recording mix window), not new tunables.
*Alternatives:* raw sample counts (rejected — a bare number answers nothing about how close the operation is to firing); a bar per buffer (rejected — the pill is width-capped and already holds the controls).

**10. Model activity distinguishes readiness from work.**
Each engine reports identity and loaded state, and additionally the counters that prove it is working: speech frames evaluated and current speech detection for voice activity; pending queue depth and the most recent recognition for speech recognition; loaded engine, queue occupancy and refinements produced for word alignment; the per-channel counters for diarization. Disabled settings are reported as disabled, never as a failure.
*Alternatives:* readiness only (rejected — the user asked for activity, and readiness is already visible elsewhere); scraping per-engine logs (rejected — brittle and gated behind debug flags).

**11. Fills render as proportional bars; models render as a colour-coded dot row.**
A bar answers "how close is this to firing" faster than a percentage does, so each gated buffer renders as a short track with a proportional fill, labelled with the operation it gates. Fill is clamped to the track for rendering while the fired state stays separate, because a buffer can exceed its threshold (the dispatcher takes a whole batch) and an overflowing bar would be meaningless. The model row uses one indicator per model with four colours: healthy, idle/disabled (grey — never red, so a disabled model cannot be misread as a failure), warning (work dropped), error (cannot process). Labels stay attached to the indicators so the row reads without hovering, and the tooltip keeps the detail.
*Alternatives:* percentages only (rejected — the user asked for the faster visual read, and a bare `68%` competes with the numeric counters already on the line); colour-only indicators with no labels (rejected — a colour without a name is unreadable at this size); putting the model row in the tooltip (rejected — the point is to see model health without interaction).

**12. The level meter is a dBFS meter, not a fill bar, and it decays.**
A level bar answers a different question than the gated bars ("is this channel alive and how loud"), so it is computed from the processed mono samples the pipeline already holds, as RMS plus peak, and rendered on a decibel scale with a practical floor. A linear amplitude meter would look dead for normal speech (RMS around 0.02–0.15), so dB is what makes it readable. Each level carries the age of its last sample, and the bar falls to empty past half a second without audio, so pausing or losing a device cannot leave a stale bar lit. The level is measured before the samples are handed to the recording ring buffer, so it reflects the signal the pipeline actually uses.
*Alternatives:* a linear amplitude bar (rejected — unusable range); no decay (rejected — a paused channel would look live); reusing the existing device level monitor (rejected — it opens its own capture streams, supports input devices only, and its registered implementation currently reports synthetic data).

**13. 150 ms refresh on the existing timer.**
The recording controls' interval is tightened from 300 ms to 150 ms; both the animated bars and the status snapshot stay on that single timer. Note the consequence: producer granularity still bounds the data — diarization and pipeline counters advance per merged speech chunk (hundreds of milliseconds), so faster sampling improves responsiveness rather than revealing new data.

## Risks / Trade-offs

- [The processor may fail to start (models missing), leaving the handle empty while the recording runs] → the display state resolves to unavailable/error using the existing failure path and model-readiness check; the lines never show zeros as if they were live.
- [In Fast mode the first embedding failure disables the engine for the whole session] → this is exactly what the error state is for; it is the primary detector, and it is per-channel so an unaffected channel stays readable.
- [New state touched per chunk risks the hot path] → counters are atomics; the last-turn memo is written once per stable turn, not per chunk; all reads happen on the sampling interval.
- [Two lines can overflow the pill] → compact tokens, truncated names, and hiding the block on narrow viewports; if width is still tight the block may be allowed to wrap under the indicator rather than widen the pill.
- [Real counters next to a fake animation invite misreading] → the block is visually labeled as diarization and separated from the bars by a divider.
- [Stale counters across sessions] → reset at start and hidden at stop; the after-stop case is covered by a spec scenario.
- [Efficient mode shows a "dead" row by design] → the deferred state states clustering happens at stop, so a zero turn count reads as expected.
- [A second polling timer] → reuse the parent's existing recording interval instead of adding a timer.

## Migration Plan

Additive and read-only: no database, on-disk, or model changes, no breaking API change. Rollback is removing the two lines and the read-only command; diarization behavior is unaffected either way. No data migration is required.

## Open Questions

- The exact compact token wording and how much of the last turn fits are tunable at implementation time without changing the specs.
- Whether the animated bars should later carry real levels is deliberately out of scope.
