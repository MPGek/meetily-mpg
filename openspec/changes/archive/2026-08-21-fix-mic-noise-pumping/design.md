## Context

Verified current state of the live mic chain (`audio/pipeline.rs:476-537`): high-pass filter (80 Hz) → RNNoise (on by default via `RNNOISE_APPLY_ENABLED`) → `LoudnessNormalizer` (EBU R128, target -23 LUFS) → true-peak limiter, applied only to `DeviceType::Microphone` chunks. The system stream is only resampled — no enhancement.

The amplification defect is exactly this `LoudnessNormalizer`: it measures the cumulative integrated loudness of everything it has heard (`ebur128.loudness_global()`), computes `gain = 10^((−23 − measured)/20)`, and applies that gain to every sample, updating every 512 samples with no speech gating, no maximum-gain clamp, and no attack/release. RNNoise pushes the noise floor down; the AGC measures an even lower loudness; the gain estimate rises; street noise on the mic gets boosted toward conversational level (`audio_processing.rs:222-260`).

Additional verified facts shaping the design:
- The live recording path interleaves stereo `[left=mic, right=system]` without ducking (`AudioMixerRingBuffer` / `interleave_stereo`, `pipeline.rs:921-938`).
- The legacy ducking mixer in `audio/ffmpeg_mixer.rs` (`AudioMixer`/`FFmpegAudioMixer`) is currently exercised only by unit tests; its duck decision is `mic_rms > 0.01` computed on the post-enhancement mic signal.
- Dormant automatic-gain code that would re-create the defect: `normalize_v2` (`audio_processing.rs:96`, no callers, RMS boost target 0.9 with forced min 1.5x) and `audio_v2::AudioNormalizer` (`audio_v2/normalizer.rs`, per-chunk peak normalizer `gain = 0.25/peak`, used by the unwired `ModernRecorder`).
- `ebur128` is used only by `LoudnessNormalizer`; the private `TruePeakLimiter` is used only by `LoudnessNormalizer`.

## Goals / Non-Goals

**Goals:**
- No automatic gain is applied to the live mic signal; the chain is high-pass → RNNoise → unity gain, so quiet/non-speech content is never boosted.
- The system-audio ducking decision — wherever it is applied — is driven by microphone speech activity (Silero VAD) with a debounce, never by the processed mic level.
- Dormant automatic-gain implementations are deleted (v1) or made passthrough (v2), so the defect cannot resurface on any capture path.
- Regressions are covered by unit tests for the ducking decision.

**Non-Goals:**
- No loudness normalization at export/playback in this change. EBU R128 is a post-production target; if it is desired later on finished files it must be a separate offline step on content-complete material.
- Do NOT wire the dormant ducking mixer into the current live recording path. The live path today has no ducking, and adding it would change recording output — out of scope.
- No volume control UI, no user-facing gain setting, no frontend changes.

## Decisions

**D1 — Remove the live AGC outright rather than gating it.**
`pipeline.rs` STEP 3 (the `LoudnessNormalizer` call, lines 523-536) is removed; the mic chain becomes HPF → RNNoise with unity gain.
Rationale: simplest correct fix. Live capture does not need loudness coherence — the transcription and VAD stages consume relative levels, and playback can normalize content-complete material.
Alternative considered: keep the normalizer but gate gain adaptation on RNNoise's per-frame VAD probability (currently discarded at `audio_processing.rs:339`) with a gain clamp and attack/release smoothing. Rejected: it keeps live loudness processing that nothing downstream needs, adds tuning surface, and leaves the EbuR128 machinery in the hot path.

**D2 — Delete dormant AGC code; make the v2 placeholder a passthrough.**
Remove `normalize_v2`, `LoudnessNormalizer`, and the private `TruePeakLimiter` from `audio_processing.rs`; drop the `ebur128` dependency from `frontend/src-tauri/Cargo.toml`. Keep `audio_v2::AudioNormalizer` (public API referenced by `ModernRecorder`) but make `normalize()` return the input unchanged (passthrough) and replace the peak-normalizing body and its TODO, so the modern recorder cannot amplify silence when it is eventually wired.
Rationale: deleting zero-caller v1 code is unambiguous; deleting the v2 `AudioNormalizer` entirely would ripple through the unwired `ModernRecorder` and `audio_v2` public exports for no behavior gain.

**D3 — Ducking decision is speech-driven.**
The `AudioMixer` in `audio/ffmpeg_mixer.rs` no longer computes `mic_rms > 0.01` on the post-enhancement mic signal. Instead the mix path takes a microphone **speech-active** flag (from the per-channel Silero VAD that already runs in `AudioPipeline`, `pipeline.rs:704`); the mixer maintains the debounce (engage duck immediately on speech; hold the duck for ~500 ms after speech ends, then return to full level) and transitions smoothly between ducked (0.6) and full (1.0) system gain rather than hard-switching.
Rationale: the VAD is the project's existing speech ground truth; RMS is level, not speech — loud non-speech noise must not duck system audio.
Alternative considered: use pre-enhancement (raw) mic RMS as the decision input. Rejected: RMS still cannot distinguish loud noise from speech; the VAD flag is more robust and already available.

**D4 — Keep RNNoise and the high-pass filter unchanged.**
They are not the defect; with the AGC removed they improve the SNR. The `RNNOISE_APPLY_ENABLED` flag stays.

**D5 — Do not retain ebur128 for future use.**
If export-time loudness normalization is ever wanted, implement it as a separate offline step (e.g., ffmpeg `loudnorm`) over the finished recording, where EBU R128 gating semantics are correct. Keeping the dependency "just in case" would recreate the temptation to wire it back into the live path.

## Risks / Trade-offs

- **Mic level is no longer equalized across recordings/speakers** → quieter speakers will be quieter in the recording. Mitigation: natural levels are the conservative default; users can adjust OS input gain; a future offline export normalizer (D5) can level-match finished files without touching live capture.
- **Removing the limiter means a near-full-scale input could clip on the mic channel** → RNNoise and typical headroom keep levels well below clipping; a simple clipper can be added separately if ever observed. Noted, not built.
- **The ducking change is only unit-tested while the mixer is dormant** → its behavior can drift before wiring. Mitigation: keep it covered by existing `ffmpeg_mixer.rs` tests with scripted speech-activity sequences; wiring the mixer into the live path remains future work that must reuse this decision.
- **The v2 path (modern recorder) could still reproduce the bug through its own mixer's RMS ducking (`audio_v2/mixer.rs` `DuckingProcessor`) if wired before this change is revisited** → the v2 `AudioNormalizer` is already neutralized by D2; when the modern recorder is actually connected, its ducking must be driven by the same VAD signal (D3), not RMS. Flagged for that future work.

## Migration Plan

Deploy as a single code change: remove the AGC step, delete dormant code (D2), fix the ducking decision (D3), drop the dependency. No data migration; no DB change; no frontend change.

Verified by:
1. `cargo test` — `ffmpeg_mixer.rs` ducking tests (scripted speech sequences) plus the rest of the workspace.
2. Manual capture check — start a recording with only ambient noise for 30-60 s and confirm the saved left (mic) channel RMS/peak stays at its natural level with no upward ramping (the existing diagnostic logging at `pipeline.rs:530-534` can confirm gain is gone).

Rollback: revert the commit. Behavior returns to the pre-change state.

## Open Questions

None that change the specs or tasks. A future "export loudness normalization" feature (D1/D5) is a separate change when it is decided that finished recordings should be level-matched.