## Why

When a meeting has no speech, the microphone channel gets automatically amplified so ambient street noise becomes as loud as speech. Root cause: the live mic path runs the EBU R128 `LoudnessNormalizer` as an always-on automatic gain — it measures the loudness of whatever it hears (including the noise floor that RNNoise just suppressed) and pumps it toward -23 LUFS. This boosts street noise into the recording, makes the mic channel mask the system track in the stereo mix, and can bogusly trigger the mixer's ducking decision. The defect can also resurface: dormant automatic-gain code (`normalize_v2`, the `audio_v2::AudioNormalizer` placeholder) re-implements the same "boost quiet audio" pattern.

## What Changes

- **Remove the live loudness AGC from the microphone enhancement chain**: the mic path becomes high-pass filter → RNNoise → unity gain (`pipeline.rs` STEP 3 removed); quiet and non-speech content is no longer boosted toward a loudness target during capture.
- **Delete or neutralize dormant automatic-gain implementations** so the defect cannot resurface on any capture path: remove callerless `normalize_v2` and the now-unused `LoudnessNormalizer`/`TruePeakLimiter` in `audio_processing.rs`, make the `audio_v2::AudioNormalizer` placeholder a passthrough, and drop the `ebur128` dependency (only used by the removed normalizer).
- **Make the mixer's system-audio ducking decision speech-driven**: the duck decision uses microphone speech activity (per-channel Silero VAD, debounced) instead of a post-processed mic-RMS threshold, so system audio is never ducked merely because mic gain made noise louder.
- **Behavior change to capture, not user-facing breakage**: recordings are no longer normalized to -23 LUFS during capture; the left (mic) channel stays at its natural level.

## Capabilities

### New Capabilities
- `mic-gain-and-ducking`: Capture behavior for the microphone gain chain and the system-audio ducking decision — non-speech content is never amplified by automatic gain, and ducking follows speech activity rather than processed mic level.

### Modified Capabilities
- (none) — existing specs (`audio-engine`, `stereo-recording`) do not constrain gain, normalization, or ducking behavior.

## Impact

- **Backend**: `audio/pipeline.rs` (remove the EBU R128 STEP 3 from the mic enhancement chain), `audio/audio_processing.rs` (remove `LoudnessNormalizer`, `normalize_v2`, and the private `TruePeakLimiter`), `audio/ffmpeg_mixer.rs` (VAD-driven ducking decision with debounce), `audio_v2/normalizer.rs` (placeholder becomes passthrough, gain controlled by user only), `frontend/src-tauri/Cargo.toml` (drop `ebur128`).
- **No DB changes, no frontend changes, no diarization/speaker changes.**
- **Tests**: ducking-mixer decision unit tests; manual verification that a quiet recording's mic RMS stays stable.