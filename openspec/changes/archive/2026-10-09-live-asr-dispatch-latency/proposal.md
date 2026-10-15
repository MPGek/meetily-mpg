# Proposal

## Why

In live mode a finished phrase reaches speech recognition only after the **next** phrase on the same channel has ended. The pipeline sends pending speech to ASR when a newly reported VAD segment starts ≥ 500 ms after the pending tail, and VAD reports a segment only when that speech ends. There is no flush on silence alone. The `live-segment-merging` spec already says the earlier segment "SHALL be dispatched to transcription immediately"; the code does not do that.

Measured on the 30-minute meeting `transcript_compare/2026-10-08_10-00` (wall-clock `display_time` of each saved live row against the row's audio end):

- After a phrase ends, text appears after a median 4.4 s; p90 20 s; maximum 214 s.
- The worst cases are on the microphone: when the user stops talking, their last phrase waits until they talk again.
- A replay of the flush rule on the meeting's VAD segments reproduces these numbers (p99 151 vs 152 s; max 214 = 214 s), so the delay comes from this rule, not from recognition speed.

The status lines show nothing about this buffer. A user cannot tell whether speech is being collected, about to be sent, or stuck, and the 25 s and 500 ms triggers are invisible.

## What Changes

- **Flush on silence.** On every VAD dispatch (200 ms), a channel's pending speech is sent to ASR as soon as 500 ms have passed after its last segment with no new speech, or as soon as new speech is known to start ≥ 500 ms after it. The existing triggers stay: a later segment ≥ 500 ms away, > 25 s of pending speech, recording stop. Chunk boundaries do not change (the same 500 ms gap rule groups the same segments), so transcript text and WER do not change; only when it is sent.
- **Buffer telemetry.** Each channel's pipeline telemetry also reports the length of speech the VAD is still collecting (not yet a segment) and the silence since the pending tail.
- **Buffer indicator (variant A).** Each channel status line gets a 25 s tank: pending speech plus speech still being collected, against the 25 s cap. Next to it, a ring fills over the 500 ms silence countdown. The tank turns red, with a "waiting for speech" label, if pending speech outlives the silence trigger, which after this change means a regression.
- **Status lines while recording.** The two channel lines and the model row are shown for every recording, not only when diarization is on, because the ASR buffer and level bars do not depend on diarization. The diarization indicator then shows its disabled state.

Out of scope:

- Interim (partial) text while a person speaks, and a faster engine for it (GigaAM). Measured in the same exploration; a separate change.
- The 500 ms gap and 25 s cap values themselves.
- Merging transcript sub-rows that resolve to the same speaker name.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `live-segment-merging`: pending speech is dispatched once 500 ms of silence follow it, without waiting for the next segment.
- `online-diarization-telemetry`: channel lines are shown whenever a recording is in progress; each line carries the ASR buffer tank and silence countdown; telemetry reports the in-progress speech length and the silence since the pending tail.

## Impact

- `frontend/src-tauri/src/audio/pipeline.rs`: silence check after each VAD dispatch; telemetry updates.
- `frontend/src-tauri/src/audio/vad.rs`: read-only accessors for the in-progress speech (start, length).
- `frontend/src-tauri/src/audio/telemetry.rs`, `frontend/src/lib/ipc/recording.ts`: two new `PendingState` fields.
- `frontend/src/components/DiarizationStatusLines.tsx`, `frontend/src/lib/diarization-status-lines.ts`, `frontend/src/components/RecordingControls.tsx`: tank, ring, visibility.
- Live transcription rows appear sooner. Online diarization receives the same chunks sooner. No changes to saved audio, settings, database, or re-transcription.
