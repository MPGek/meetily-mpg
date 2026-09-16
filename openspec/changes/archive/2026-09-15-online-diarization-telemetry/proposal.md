## Why

Online diarization fails silently during a recording. In Fast mode the first embedding failure sets the engine to `None` (`online_diarization.rs:712`) and the rest of the session produces no speaker labels at all; in Efficient mode embeddings accumulate with nothing observable until stop. Users and developers only discover the degradation after the meeting, from logs, and cannot tell which channel (microphone or system) is affected.

## What Changes

- Add two live status lines, one per diarization channel (`Microphone`, `System`), rendered immediately to the right of the animated recording indicator in the recording controls.
- Each line reports, for its channel only: input chunk count, embedding success/failure, buffered embedding count, stable turn count, the most recent turn (label or display name, attribution source, match score) and turn-order health.
- Surface health signals that already exist internally but are invisible: a channel that stopped embedding (engine disabled in Fast mode) renders as an error, and a turn stream that goes backwards in time renders as a warning.
- Make the global context needed to read the scores discoverable without adding a third line: diarization mode, active model tag and dimension, recognition threshold, and prototype/binding counts.
- Define explicit degradation rules: the block is hidden when diarization is off or not recording, the system line reports a mono session when no system device exists, and Efficient mode states that clustering happens at stop instead of showing permanently zero turns.
- Keep the audio hot path untouched: status is sampled from shared counters on a fixed interval, never emitted per chunk, and no ASR, VAD, recording, or diarization behavior changes.

## Capabilities

### New Capabilities
- `online-diarization-telemetry`: live, per-channel visibility into the online diarization session (chunk intake, embedding health, buffer and turn counts, last turn and its confidence, turn-order health) and the global model/threshold context required to interpret it, rendered as two compact lines beside the recording indicator.

### Modified Capabilities
<!-- None: the diarization behavior itself, including the existing zero-embedding warning requirement, is unchanged. This change only makes existing per-channel state observable. -->

## Impact

- **Rust**: `frontend/src-tauri/src/audio/online_diarization.rs` (per-channel counters and the live-session model/threshold context), `frontend/src-tauri/src/audio/recording_commands.rs` (session wiring and stat handle), `frontend/src-tauri/src/audio/live_diarization_reconcile.rs` (read access to the per-channel turn-order flag), and a new read-only Tauri command in `lib.rs`.
- **Frontend**: `frontend/src/components/RecordingControls.tsx` (the two lines after the animated bars) and `frontend/src/app/page.tsx` (sampling interval and prop wiring alongside the existing `barHeights` animation).
- **No database, model, dependency, or on-disk format changes.** No breaking changes; existing `check_diarization_models`, prototype store, and turn registry are read, not modified.
