## Why

After unifying VAD parameters across modes, live transcription quality still lags behind enhance mode — specifically missing first words of phrases after pauses. The root cause is not VAD parameters but the downstream handling: each VAD segment is sent to Whisper as an isolated call with no inter-segment context, no previous-text prompt, and no merging of adjacent utterances.

## What Changes

- **Live-mode segment merging** — adjacent VAD segments with gap < 500ms are merged in the pipeline before dispatching to transcription, matching the pattern used in enhance mode but with a tighter threshold suitable for real-time
- **Whisper `condition_on_previous_text`** — the transcription worker passes the previous transcript as context prompt for subsequent segments from the same audio source, giving Whisper linguistic continuity across pauses
- Minimum segment threshold raised from 800 to 1600 samples to match enhance mode's filter and reduce tiny-noise segments

## Capabilities

### New Capabilities
- `live-segment-merging`: Real-time merging of adjacent VAD segments in the live audio pipeline, producing coherent multi-utterance chunks for Whisper transcription

### Modified Capabilities
- `source-labeled-transcription`: Transcription worker now passes previous transcript text as Whisper prompt context for subsequent segments from the same source device

## Impact

- **Affected files**: `pipeline.rs` (segment merge in run loop), `transcription/worker.rs` (previous-text context forwarding)
- **No API changes**: Tauri events, transcript segment format unchanged
- **Latency**: Merging with 500ms gap threshold adds at most 500ms extra delay to a pending segment before dispatch
- **No DB or UI changes**
