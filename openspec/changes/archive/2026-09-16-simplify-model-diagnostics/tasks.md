## 1. Backend telemetry

- [x] 1.1 Add per-model `sent_total`/`completed_total`/`in_flight`/`requested` state to `ModelsActivity` in `frontend/src-tauri/src/audio/telemetry.rs` (VAD, ASR, alignment, diarization) and expose it in `get_recording_telemetry`; verify `cargo check` passes in `frontend/src-tauri`
- [x] 1.2 Wire ASR `sent_total`/`in_flight` in `frontend/src-tauri/src/audio/transcription/worker.rs` (increment on enqueue, set in_flight during recognition, complete on done/error); verify with a short recording: `asr.pending` returns to 0
- [x] 1.3 Wire alignment model activity (`requested`/`in_flight`) in the alignment queue path (`frontend/src-tauri/src/audio/word_alignment/queue.rs`); verify `queued_jobs` decrements as jobs refine
- [x] 1.4 Add pending-block gauge to diarization stats in `frontend/src-tauri/src/audio/online_diarization.rs`: increment when a block is enqueued, decrement when processed or dropped (clamp ≥ 0), increment diarization `sent_total`/`completed_total` accordingly; surface pending count via `ChannelStatusLine`/`online_diarization_status()`
- [x] 1.5 VAD indicator state: expose `speaking` (already exists) plus pipeline `requested` flag; verify snapshot JSON in `get_recording_telemetry` includes all new fields

## 2. Frontend types and formatters

- [x] 2.1 Extend TS types in `frontend/src/services/diarizationStatusService.ts` (`RecordingTelemetry` models: sent/completed/in_flight/requested, diarization pending blocks) and verify snapshot fields render in debug
- [x] 2.2 Simplify formatters in `frontend/src/lib/diarization-status-lines.ts`: remove `buildBufferBars`, diarization counter text, and tooltip `modelLines`; keep `buildLevelBar`; add blink-state mapping (processing → green blink, requested-not-processing → red blink, loaded-idle → steady, disabled → idle, error → error); verify with unit tests if a test exists for formatters, otherwise verify via rendered output
- [x] 2.3 Map per-model queue counters: STT pending from `queued − completed`, diarization pending from new gauge; verify counter shows during processing and disappears (hidden/zero) when drained

## 3. UI component

- [x] 3.1 Rework `frontend/src/components/DiarizationStatusLines.tsx`: keep MIC/SYS lines with level bar + (when > 0) pending-block count; keep model-indicator row with blinking dots (CSS animation, no new deps); remove buffer bars, counter text, tooltip; verify visually during a recording (npm dev build compiles, no TS errors via typecheck)
- [x] 3.2 Verify behavior matrix: red-blink on request submit, green-blink while processing, steady when idle, edge/dropped → warning/error treatment unchanged

## 4. Integration verification

- [x] 4.1 Run `cargo check`/`cargo test` in `frontend/src-tauri` and TypeScript typecheck/lint in `frontend`; fix any failures
- [x] 4.2 Record a real meeting session with diarization + alignment on; confirm: levels move, STT/diarization pending counters appear and clear, lights blink green→steady at correct times; confirm transcript, speaker assignments, saved audio unchanged (non-interference requirement)
