# Proposal

## Why

The microphone noise suppressor has never worked. `NoiseSuppressionProcessor` (`audio_processing.rs:112-219`) passes samples in `[-1.0, 1.0]` to `nnnoiseless::DenoiseState::process_frame`, which expects 16-bit PCM range `[-32768.0, 32767.0]` (its own docs, `nnnoiseless-0.5.2/src/denoise.rs:86-90`). At that scale RNNoise sees a signal about 90 dB quieter than it is and leaves it unchanged. Measured on 4 min of the mic channel of `transcript_compare/2026-10-08_10-00`: as called today, VAD probability is above 0.5 on 0 % of frames, and the noise floor and speech level do not change (-66.3 → -66.3 dBFS, -28.7 → -28.6 dBFS). With correct scaling, VAD is above 0.5 on 15.6 % of frames and the noise floor drops to -92 dBFS.

So every mic recording, the ASR ground-truth benchmark, the enrolled voiceprints and the diarization tuning were all made on raw mic audio. The suppressor only adds a 10 ms mic delay against the system channel, CPU work and two heap allocations per 10 ms frame on the capture thread. It also writes "10-15 dB reduction" to the log, which is false. The noise floor on the recorded mics is already low (about -61 to -66 dBFS against speech at about -29 dBFS). Turning real denoising on would change, for good, the audio that ASR, diarization and voiceprints see, and it would need its own A/B evaluation. This change removes the dead stage and writes the actual behavior into the specs.

## What Changes

- Remove RNNoise from the live microphone chain. The chain becomes resample → high-pass filter (80 Hz) → unity gain. The saved and transcribed mic audio stays the same as today, apart from losing the 10 ms RNNoise delay.
- Delete `NoiseSuppressionProcessor`, the `RNNOISE_APPLY_ENABLED` flag and its re-export, and the `nnnoiseless` dependency.
- Delete dead code in `audio_processing.rs`: `spectral_subtraction` (no callers, hardcoded for 16 kHz, silently truncates input to 1600 samples), `average_noise_spectrum` (no callers), and `HighPassFilter::reset` plus the unused `sample_rate` / `cutoff_hz` fields (no callers, `#[allow(dead_code)]`). Drop the `realfft` dependency, which only `spectral_subtraction` uses.
- Update the docs that list RNNoise as part of the pipeline.

Out of scope (follow-ups):

- Real noise suppression (fixed scaling, an attenuation cap, ASR-only use, or a stronger model). This needs an A/B on the ground-truth meetings first (WER and diarization).
- Acoustic echo cancellation that uses the system channel as the reference.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `mic-gain-and-ducking`: the "Live microphone capture does not amplify non-speech content" requirement, which required noise suppression, is replaced by "Live microphone capture keeps its natural level and noise floor". The no-AGC scenarios carry over, and the chain is stated as high-pass filter only at unity gain. The OpenSpec validator does not allow a scenario to be dropped from a MODIFIED requirement, so this is written as REMOVED + ADDED.
- `mic-disconnect-recovery`: the "A replacement microphone gets the same processing as a start-time microphone" requirement drops "and, when enabled, noise suppression" from the chain it describes.

## Impact

- Code: `frontend/src-tauri/src/audio/audio_processing.rs`, `audio/pipeline.rs` (`AudioCapture` fields, constructor, `process_audio_data`), `audio/ffmpeg_mixer.rs` (flag), `audio/mod.rs` (re-export).
- Dependencies: `nnnoiseless` and `realfft` removed from `frontend/src-tauri/Cargo.toml` (and `Cargo.lock`).
- Docs: `docs/CODEBASE_MAP.md`, `docs/CODEBASE_MAP_ARCHITECTURE.md`.
- Behavior: no audible change. The mic channel lands 10 ms earlier against the system channel, which is closer to the true alignment. No DB, API or frontend change.
