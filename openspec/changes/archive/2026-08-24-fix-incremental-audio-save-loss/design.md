## Context

The recording save path mixes mic + system audio in `AudioPipeline` into 600 ms stereo windows, forwards them through an unbounded channel to `IncrementalAudioSaver::add_chunk`, and every N samples spawns a full ffmpeg process (`encode_single_audio`) to write a checkpoint MP4 into `.checkpoints/`. On stop, `finalize()` concats the checkpoints into `audio.mp4`. See proposal.md - Why for the incident that motivated this work.

Current weaknesses that shape the design:

- `encode_single_audio` sends raw f32 to ffmpeg's native AAC encoder with no NaN/±Inf guard. ffmpeg's AAC encoder aborts the whole encode on "(near) NaN/+-Inf" input (verified by experiment). The decode path already clamps non-finite samples (`decoder.rs:291`) — the encode path does not.
- On a checkpoint encode failure, `add_chunk`/`save_checkpoint` propagate the error but keep the poisoned buffer and do not increment `checkpoint_count`; the same growing buffer then fails every 15 s forever → checkpointing wedges, all later audio silently lost.
- The pipeline sends recording chunks with `let _ = sender.send(...)` (`pipeline.rs:911`), so a dead/stalled saver channel is silently ignored while transcription (a separate path) keeps working — matching the observed "transcript complete, audio half".
- `checkpoint_interval_samples = sample_rate * 30` counts interleaved stereo samples as mono frames, so the intended 30 s checkpoint fires every ~15 s. Confirmed empirically: leftover checkpoints from a crashed meeting are all ~15.0 s.
- `finalize()` reports success even when the merged audio is far shorter than the session; metadata is stamped `completed` regardless.

## Goals / Non-Goals

**Goals:**
- A single bad checkpoint (or a hung ffmpeg) must no longer cost the rest of the recording.
- Non-finite samples must never abort an audio save.
- The user must see a warning when a meeting audio save ends up partial.
- Restore the intended ~30 s checkpoint cadence (halves ffmpeg spawn pressure).
- Provide observability (logs, surfaced errors) so a future recurrence is diagnosable without guessing.

**Non-Goals:**
- Re-architecting the checkpoint storage (e.g. 10-min chunks, streaming MP4 muxer, or rust-side AAC encoding).
- Recovering the already-lost 2026-08-21_16-16 second half (data is gone).
- Changing the audio encoder, container, or bitrate (VBR AAC 0.7 stays).
- Frontend redesign of how warnings are displayed (reuse existing `recording-error` / new lightweight `recording-audio-warning` event).

## Decisions

### D1. Sanitize once at the encode boundary (`encode_single_audio`)
Replace every non-finite f32 (`x.is_nan() || x.is_infinite()`) with 0.0 immediately before `bytemuck::cast_slice` in `encode.rs`. This is the single choke point for all saved audio (checkpoints AND the legacy one-shot path), so every writer is covered without touching the pipeline.

**Alternatives considered:** sanitize at the pipeline ring buffer — rejected: the encoder remains one more malformed-input path away from a wedge (audio_v2 and any future writer would need their own guard); sanitizing at encode is the strongest guarantee for what reaches disk.

### D2. Drop-and-continue on checkpoint failure
In `save_checkpoint`, on encode error: log the failure, count it (`failed_checkpoints`), clear `checkpoint_buffer`, and return normally so `add_chunk` continues. The lost span is bounded by one checkpoint (~30 s) instead of everything after the failure. Do NOT retry the same buffer — a deterministic failure (NaN) would just re-fail.

**Alternatives considered:** isolate the specific containing window and re-encode the rest — complexity not justified for a rare failure; skip a checkpoint window entirely, then resume mid-buffer — risks desync between buffer position and wall time; decision: clear + continue is simplest and bounds loss.

### D3. Time-bounded ffmpeg operations
In `encode_single_audio` and the existing `merge_checkpoints` path, wrap the write-and-finish sequence in a timeout (checkpoint: ~60 s; merge: ~2 min), killing the child process (`child.kill()`) on expiry and treating it as a failed checkpoint/merge. ffmpeg is spawned via `std::process::Command` inside async code; guard with a wait-with-timeout loop over `try_wait()` + sleep, or move the blocking call to `spawn_blocking` and race with `tokio::time::timeout`. Prefer the timeout race so the accumulation task can never block forever.

**Alternatives considered:** per-write watchdog thread (heavy); relying on ffmpeg's own `-nostdin`/buffer limits (unverified failure modes); the timeout race is the minimal reliable primitive.

### D4. Fix checkpoint interval accounting
Make the threshold frame-based instead of sample-count-based. The buffer holds interleaved stereo data, so compute accumulated frames as `sum(len) / channels` (channels = 2) and trigger at `sample_rate * 30` frames. Alternatively set the sample threshold to `sample_rate * 30 * channels`. Also fix the `duration_seconds` log line in `save_checkpoint` (currently divides interleaved length by sample rate).

### D5. Surface saver failures instead of silent drops
- `pipeline.rs`: replace `let _ = sender.send(recording_chunk)` with an explicit match; on `Err`, log a warning and `state.report_error(...)` (throttled, e.g. once per N failures) so the recording error surface/pipeline picks it up. Add a lightweight `AudioError::SaveUnavailable` variant (recoverable) if classification is needed.
- `IncrementalAudioSaver`: track `failed_checkpoints` counter; expose via `get_stats()` so the stop flow can report partial saves.
- `stop_and_save` / `finalize`: after merge, probe the produced `audio.mp4` duration (use the bundled ffmpeg's sibling `ffprobe`, or ffmpeg `-i` output parse) and compare with the session's active duration; when the gap is significant (>5% and >30 s) or `failed_checkpoints > 0`, emit a new `recording-audio-warning` Tauri event (payload: saved vs expected durations, failed checkpoint count) AND write a corresponding flag into `metadata.json` (e.g. `audio_warning`) rather than a clean `completed`. Failed finalize/merge already surfaces through `stop_and_save`'s error return.

### D6. Keep transcription path untouched
The VAD → transcription segment flow is deliberately independent (it is the reason transcripts survived). None of the fixes redirect audio through it or couple its timing to the saver.

## Risks / Trade-offs

- **Drop-and-continue can lose up to one checkpoint window** → bounded (~30 s) and only on an already-failed encode; enormous win vs. the current unbounded loss. Surface via the warning so it is never silent.
- **Timeouts can kill a healthy-but-slow encode under extreme CPU load** → set generous margins (60 s for 30 s of audio; encode measured at <0.3 s idle) so only pathological hangs hit it.
- **Duration validation adds a probe step at finalize** → ~100-300 ms per recording; cheap relative to the merge; guarded so a probe failure does not fail the save itself.
- **New `recording-audio-warning` event is new UI surface** → minimal scope: reuse toast/notification patterns (same as `recording-error`) on the frontend; behavior contract is in the spec, presentation implementation is free.
- **Sanitizing replaces NaN with silence but the underlying transient remains** → still worth the diagnostic log of how many samples were replaced per checkpoint (debug/trace level) so a recurrence is visible; the user-facing outcome is at worst a short silence instead of data loss.

## Migration Plan

- Pure Rust-side change; no data migration, no schema change, no API break. Existing recordings are untouched.
- Rollback: revert the change; no persistent state is written by the new code beyond the optional `audio_warning` metadata field, which is additive and ignored by older versions.

## Open Questions

- None that change the specs or tasks. (Frontend presentation of the warning is intentionally left to implementation.)