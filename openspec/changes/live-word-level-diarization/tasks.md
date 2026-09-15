## 1. Spike: turn-stream stability assumptions

- [ ] 1.1 Instrument stable-turn emission in Fast mode with a debug log of `(channel, turn_label, start, end, revision_seen)` while recording a real multi-speaker session; verify the emitted stable turns are append-only, time-ordered, and never revised (record in design.md D4 before continuing; if revisable, downgrade watermark to next-later-turn rule). Verify by reviewing the log on a 3+ minute session.

## 2. LiveTurnRegistry

- [x] 2.1 Add `LiveTurnRegistry` (per-channel append-only turn list + watermark `last_turn_end` + `Notify`; entries carry absolute `start_time/end_time`, cluster `speaker` label, `display_name`, `matched_by`, `match_score`) in a new module `audio/live_diarization_reconcile.rs`; unit-test append/notify/watermark progression (`cargo test live_turn_registry`).
- [x] 2.2 Publish stable turns to the registry in the Fast-mode stable-turn branch of `online_diarization.rs` (next to the existing `channel.turns` push), reusing `mapper.to_abs` times and the same display-provenance sources as the emitted `online-speaker-turn`; verify existing turn emission tests still pass and registry receives same values.

## 3. Reconcile consumer (Fast mode)

- [x] 3.1 Create the bounded provisional map keyed by parent `sequence_id` inside `live_diarization_reconcile.rs` (drop-oldest on overflow, release keeps last rendering); unit-test overflow drop and retention of last rendering.
- [x] 3.2 Run token attribution downstream of the alignment stage: for each finalized block with `tokens`, call `assign_tokens_to_speakers` against the registry turns of the block's own `source_device`, and emit an initial `live-transcript-blocks` (single- or multi-block) when decidable; park the block otherwise. Verify with unit test: fully covered single-speaker block emits one block-run; tokens spanning two validated speaker runs emit ≥2 sub-blocks with contiguous gap-free timestamps and token-slice text.
- [x] 3.3 Implement watermark decidability (stable turn with `start >= block.end` exists for the channel) and Notify-driven re-evaluation of held blocks, re-emitting a new `revision` of the same parent when attribution changes. Verify with unit test: block parked with uncovered tail splits into A|B sub-rows when the late covering turn arrives; late-never blocks release at stop unchanged.

## 4. Event wiring (backend)

- [ ] 4.1 Define the `live-transcript-blocks` payload (`parent_sequence_id`, `source_device`, `revision`, `blocks[]` with `start/end/text/speaker/display_name/matched_by/match_score`) and emit for the parent's current revision (single-speaking revisions also re-emit so the frontend has consistent identity); wire the consumer into the same start/stop lifecycle as the alignment queue (spawn on recording start with Fast mode, `close + bounded drain` before stop-time finalize). Verify: recorded session logs event emissions and stop completes without deadlock (`cargo test` + manual Fast-mode recording smoke run).
- [ ] 4.2 Gate the whole stage on Fast mode: Efficient/Off produce no registry activity and no events; verify by recording once in Efficient mode and confirming zero `live-transcript-blocks` emissions and unchanged transcript flow.

## 5. Frontend display

- [x] 5.1 Subscribe to `live-transcript-blocks` in `TranscriptContext` (or recording transcript store), accumulating latest revision per `parent_sequence_id` (replace semantics on revision change), reset on recording start together with `turnsRef`; verify with a unit test on the reducer: revision N+1 replaces revision N, no duplicate rows.
- [ ] 5.2 Render sub-rows for parents carrying block revisions beneath/instead of the single segment row in the recording transcript view (layered with existing speaker dot/label and channel provenance, `activeSegmentId`/playback highlight still addresses the parent); preserve scroll position and no full-list flash on revision updates; verify visually with a Fast-mode two-speaker session and by checking per-row labels match the emitted sub-block speaker labels.
- [x] 5.3 Carry pinned labels and the re-keying of single-turn overrides onto the covering sub-row per `live-speaker-labels` delta: apply `userAssignmentsRef` (parent-link + cluster-level) and window-scoped overrides across splits. Verify unit test: pinned "Alice" survives a split revision for the covering sub-row; override window lands on the overlapping sub-row only.

## 6. Integration & regression

- [ ] 6.1 Stop-time consistency: record with Fast mode, split live view, stop, and verify persisted rows come from the existing stop-time split path (per-speaker rows match the live sub-rows' speakers and spans; `SHARED_SEGMENTS`/transcripts.json/db contain the original block exactly once before finalize) — manual verify + `cargo test finalize*`.
- [ ] 6.2 Degradation matrix: run with alignment disabled (ASR tokens), alignment model missing, turn stream interrupted (diarizer failure), and alignment-queue overflow (simulated): each case degrades silently — no events on no turns, ASR-time attribution, oldest blocks released, recording continues. Verify via logs and UI in one scripted session per case.
- [x] 6.3 Run `cargo test`, `cargo clippy`, and frontend lint/typecheck; fix introduced failures only.
