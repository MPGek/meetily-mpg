## 1. Backend: emit live speaker turns (Fast mode)

- [x] 1.1 Thread an `AppHandle` into `OnlineDiarizationProcessor` (constructor param) and pass `app.clone()` from `recording_commands.rs` where the processor is created
- [x] 1.2 Define the `online-speaker-turn` event payload (`start_time`, `end_time`, `speaker`, `source_device`) and emit it from Fast mode
- [x] 1.3 In Fast mode `process_chunk`, translate each stable turn's `start`/`end` to absolute time via the existing `TimelineMapper` and emit immediately, while still buffering the turn for `finalize`
- [x] 1.4 Apply the channel-scoped prefix (`MIC_SPEAKER_NN` / `SPEAKER_NN`) to live IDs, honoring the no-system-audio case
- [x] 1.5 Verify no `online-speaker-turn` events are emitted in Efficient or Off modes, and that `finalize` behavior is unchanged

## 2. Frontend: listen and match

- [x] 2.1 Add an `online-speaker-turn` listener (in `recordingService.ts` or a diarization service) with a typed payload
- [x] 2.2 Add a TypeScript `matchSpeakerToTranscript(turn, segment)` helper that mirrors `find_best_speaker` temporal-overlap logic (and channel routing)
- [x] 2.3 In `TranscriptContext`, store live turns per channel and recompute `speaker` on segments when a turn or a transcript arrives, updating state keyed by `sequence_id`
- [x] 2.4 Ensure live `speaker` state does not leak into the DB save (persistence still comes from `speaker_assignments` at stop)

## 3. Frontend: display

- [x] 3.1 Pass `speaker` through in `app/_components/TranscriptPanel.tsx` segment mapping (add `speaker` alongside `source_device`)
- [x] 3.2 Confirm `VirtualizedTranscriptView` renders speaker dot/label live with no further changes (it already renders when `speaker` is set)
- [x] 3.3 Confirm retroactive label fill-in renders correctly when a turn arrives after its transcript segment

## 4. Verification

- [x] 4.1 Run `cargo build` and `cargo test` in `frontend/src-tauri`
- [x] 4.2 Run frontend lint/typecheck (per project scripts)
- [x] 4.3 Manual: record in Fast mode with 2+ speakers; confirm labels appear live during recording and persist correctly after stop
- [x] 4.4 Manual: record in Efficient and Off modes; confirm no live labels appear and stop-time labeling is unchanged
