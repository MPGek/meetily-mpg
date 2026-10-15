## Why

On 2026-08-21 a ~31.5 minute meeting saved only ~16.5 minutes of audio: `audio.mp4` ends abruptly at 990.0s (exactly 66 of the 15s incremental checkpoints) while the live transcript correctly covers all 1891.9s. Audio saving silently stopped partway through the session and the app still reported the recording as "completed", so the user and any downstream consumer (retranscription, summaries, playback) are unaware that roughly half the recording is missing. The recorded audio must never be silently lost because of a single bad checkpoint.

## What Changes

- **Sanitize encoder input**: replace NaN/±Inf float samples with silence (0.0) before handing PCM to ffmpeg, matching the sanitization already present on the decode path (`decoder.rs`). ffmpeg's native AAC encoder rejects NaN input with "Input contains (near) NaN/+-Inf", which aborts the entire checkpoint encode.
- **Unwedge checkpoint writes**: when a checkpoint encode fails, the accumulated buffer must be discarded (or the offending data isolated) and checkpointing must continue at the next boundary instead of retrying the same ever-growing buffer forever.
- **Bound ffmpeg interactions**: time out ffmpeg spawn / stdin write / wait so a hung encoder process cannot stall the recording saver indefinitely.
- **Fix checkpoint interval math**: `checkpoint_interval_samples` currently counts interleaved stereo samples as if they were mono, halving the intended 30s checkpoint interval to ~15s and doubling ffmpeg spawn load. Checkpoint sizing must be computed in real audio frames.
- **Surface saver failures**: the pipeline currently discards recording chunks with `let _ = sender.send(...)`. Saver stalls must be logged and surfaced (recording-error event / UI) instead of failing silently, and the final merged duration must be validated against the recorded session duration with a visible warning on mismatch.

## Capabilities

### New Capabilities
- `recording-save-durability`: behaviors that guarantee recorded meeting audio is not silently lost during incremental checkpointing and finalization — non-wedging on checkpoint failure, bounded ffmpeg operations, error surfacing, and saved-vs-session duration validation.

### Modified Capabilities
- `audio-encoding`: encode input must be sanitized (NaN/±Inf → silence) so the AAC encoder never aborts a recording save; checkpoint duration math must reflect actual audio frames per channel, producing the intended 30s checkpoint cadence.

## Impact

- **Rust** (`frontend/src-tauri/src/audio/`): `encode.rs` (encode_single_audio sanitization + timeout); `incremental_saver.rs` (checkpoint failure handling, interval math, ffmpeg timeout, duration validation); `pipeline.rs` (recording send error logging/handling); `recording_saver.rs` (failure surfacing, stop/save validation); `recording_commands.rs` / `recording_state.rs` (error event plumbing if a new error variant is added); `decoder.rs` (unchanged, referenced as the sanitization precedent).
- **Behavior**: recorded meetings may now produce a partial-audio warning instead of silently reporting success when audio is lost; normal recordings are unaffected.
- **No breaking changes** to file formats, commands, or the frontend API surface.