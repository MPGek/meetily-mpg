# Proposal: live-speaker-labels-during-recording

## Why

Online diarization currently assigns speaker labels only at recording stop, so the recording page never shows who is speaking while a meeting is being recorded. The Fast mode's polyvoice `StreamingPipeline` already identifies stable speaker turns during recording, but those turns are silently buffered until stop. Emitting them live closes this gap and turns the recording view into a real-time multi-speaker transcript.

## What Changes

- **Fast mode emits stable speaker turns live**: as the `StreamingPipeline` finalizes each stable turn, the backend translates it to absolute recording time and emits it to the frontend via a new `online-speaker-turn` Tauri event (instead of only buffering it).
- **Frontend matches turns to live transcripts**: the recording page matches each transcript segment to the speaker turn covering its `[audio_start_time, audio_end_time]` window by temporal overlap and sets the segment's `speaker` field.
- **Live speaker badges on the recording page**: the recording transcript panel passes `speaker` through to the renderer, which already supports speaker labels (colored dot + label).
- **Retroactive label fill-in**: a segment that renders before its turn becomes stable gains its speaker label once the turn arrives.
- **Stop-time labeling unchanged**: the existing `recording-stopped` → `speaker_assignments` path remains the authoritative final pass and persists labels to the database exactly as today.
- **Efficient mode unchanged**: Efficient mode still clusters at stop; it does not emit live labels in this change.

## Capabilities

### New Capabilities

- `live-speaker-labels`: Streaming emission of speaker turns during recording and live display of speaker labels on the recording page.

### Modified Capabilities

- `online-speaker-diarization`: Fast mode no longer buffers stable turns silently until stop — it emits them live to the frontend during recording.

## Impact

- **Rust backend**:
  - `audio/online_diarization.rs` — Fast mode emits stable turns via `AppHandle` during `process_chunk`; `TimelineMapper` translation used immediately rather than at `finalize`.
  - `audio/recording_commands.rs` — thread an `AppHandle` into the online diarization processor so it can emit events.
- **Frontend**:
  - `services/recordingService.ts` (or a diarization service) — new `online-speaker-turn` listener.
  - `contexts/TranscriptContext.tsx` — live matching of turns to transcript segments, storing `speaker` on segments.
  - `app/_components/TranscriptPanel.tsx` — pass `speaker` into the segments rendered by `VirtualizedTranscriptView`.
- **New Tauri event**: `online-speaker-turn`.
- **No database schema changes**, no new dependencies.
