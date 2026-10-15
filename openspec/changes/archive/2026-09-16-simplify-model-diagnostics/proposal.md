# Proposal: simplify-model-diagnostics

## Why

The recording telemetry panel (`DiarizationStatusLines`) currently shows a lot of per-action detail: VAD dispatch window bars, pending speech bars, mix window bars, diarization counter text (chunks, embeddings ok/fail, buffered secs, turns, last speaker), tooltip model lines. This is noisy and hard to read in real time; the user wants a minimal, glanceable status.

## What Changes

- REMOVE the complex per-action visualization from the recording status panel:
  - VAD buffer bars (`v`), pending speech bars (`p`), mix window bars (`m`)
  - Diarization counter lines (chunks `«N»c`, embeds ok/fail, buffered secs, turns, last speaker)
  - Model detail tooltip lines
- KEEP per-channel volume level (MIC / SYS dB bar) as-is.
- KEEP a simple block-queue counter ("blocks in queue") for:
  - Speech-to-text model: pending blocks = queued − completed (already in telemetry as `asr.pending`); counter disappears (shows 0 / hidden) when queue is drained.
  - Diarization model: a new pending-block gauge (blocks enqueued to diarization, decremented when processed); counter clears when all blocks are processed.
- ADD blinking indicator lights per model (VAD, STT/ASR, ALIGN, DIAR — and any other long-running model instruments):
  - GREEN blinking = model is currently processing (work in progress).
  - RED blinking = a request has been sent to this model and it is waiting/not yet processing.
  - Static green (non-blinking) when idle but loaded; unchanged when not recording.
- Summary/LLM provider: no telemetry exists today; do NOT extend to it in this change (out of scope).

## Capabilities

### New Capabilities

- (none)

### Modified Capabilities

- `online-diarization-telemetry`: remove per-action detail (buffer fill bars, diarization counter text, global-context tooltip, per-model detail); keep per-channel level bars; reduce model activity to block-queue counters for STT and diarization; add blinking processing/requested indicator lights.

## Impact

- `frontend/src-tauri/src/audio/telemetry.rs` — add diarization pending-block atomics + `requested` flags per model.
- `frontend/src-tauri/src/audio/online_diarization.rs` — enqueue/process hooks to update pending gauge.
- `frontend/src-tauri/src/audio/transcription/worker.rs` — mark requested/processing for ASR, alignment.
- `frontend/src-tauri/src/audio/recording_commands.rs` — extend `ModelsActivity` snapshot.
- `frontend/src/services/diarizationStatusService.ts` — extend TS types.
- `frontend/src/lib/diarization-status-lines.ts` — simplify formatters; new blink states.
- `frontend/src/components/DiarizationStatusLines.tsx` — render simplified lines and blinking dots.
