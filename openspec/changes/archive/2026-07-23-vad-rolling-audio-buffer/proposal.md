## Why

Live mode transcription loses the first 30-50ms of speech after pauses, causing missing first words or syllables. The root cause: when the VAD detects speech (probability crosses 0.50), the `ContinuousVadProcessor` starts the segment from the current 512-sample window (32ms). All previous windows that contained the actual speech onset (soft consonants like "s", "f", "h", "p", "t", "th") are already discarded.

The current `pre_speech_pad` logic (vad.rs:592-602) is a no-op when `processed_samples > 2400` (after the first ~150ms of recording). It calculates `padding_needed = 2400 - 2400 = 0`, so no audio is backfilled. The VAD processor has no rolling buffer of recent audio to prepend.

Enhance mode partially masks this issue because the segment merger combines adjacent segments, giving Whisper more context to infer missing onsets. But the root cause persists in both modes.

## What Changes

- Add a rolling audio buffer (ring buffer) to `ContinuousVadProcessor` that maintains the last N processed windows (e.g., 10 windows = 320ms at 16kHz)
- When speech is detected, prepend the buffered audio to `current_speech` instead of zeros
- The buffer slides forward as new windows arrive, always keeping the most recent audio available for backfill
- Update the `pre_speech_pad` logic to use real audio from the buffer when available

## Capabilities

### New Capabilities
- `vad-rolling-buffer`: Rolling audio buffer in VAD processor for backfilling speech onset audio

### Modified Capabilities
- `independent-vad`: VAD processor now maintains a rolling buffer and uses it for pre-speech padding

## Impact

- **Affected files**: `audio/vad.rs` (ContinuousVadProcessor struct and process_chunk method)
- **Memory**: Rolling buffer adds ~320ms × 16kHz × 4 bytes = ~20KB per VAD processor (2 processors = ~40KB total)
- **Performance**: Minimal — circular buffer operations are O(1) per window
- **No API changes**: Transcription worker and pipeline continue to receive SpeechSegment as before
- **Quality improvement**: Live mode will recover 30-50ms of speech onset that was previously lost
