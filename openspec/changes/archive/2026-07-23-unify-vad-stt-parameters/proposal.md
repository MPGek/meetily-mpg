## Why

VAD parameters are scattered across four files with duplicate constants and divergent values between live (400ms redemption) and enhance/import (2000ms redemption). STT parameters are hardcoded inline in `whisper_engine.rs`. This creates maintenance burden, makes tuning error-prone, and prevents applying the same proven configuration across all modes.

## What Changes

- **New `VadConfig` struct** with `live()` and `batch()` presets, passed to `ContinuousVadProcessor` replacing the single `redemption_time_ms` parameter
- **Unified VAD redemption to 200ms** for the core Silero VAD, consistent with industry defaults (Google/Azure: 500ms, faster-whisper batched: 160ms, Silero streaming: 100ms)
- **Reduced speech padding from 300/400ms to 150/150ms**, halving the zero-silence injection into Whisper segments while retaining enough buffer for proper spectrogram processing
- **New segment merger for batch modes** that combines tight VAD segments into coherent 10-25s chunks (gaps < 2000ms merged), replacing the 2000ms redemption hack
- **De-duplicated constants** — shared between `vad.rs`, `retranscription.rs`, `import.rs`, and `pipeline.rs`
- Hardcoded STT parameters in `whisper_engine.rs` lifted into `AdaptiveWhisperConfig`

## Capabilities

### New Capabilities
- `vad-config`: Centralized VAD parameter management with mode-specific presets (live vs batch) and a post-VAD merger for batch processing

### Modified Capabilities
- `independent-vad`: VAD processors now receive a `VadConfig` instead of only `redemption_time_ms`; behavior preserved but parameter source changes
- `per-channel-vad`: Same VAD configuration used across live, retranscription, and import modes

## Impact

- **Affected files**: `vad.rs`, `pipeline.rs`, `retranscription.rs`, `import.rs`, `hardware_detector.rs`, `whisper_engine.rs`
- **Backward compatible**: Existing segment structure unchanged; only default values shift (400→200ms redemption, 300/400→150ms padding)
- **Test impact**: VAD unit tests need updated expected segment counts due to redemption/padding changes; the `test_vad_400ms_vs_2000ms_segmentation` test must be rewritten
- **No DB or UI changes**: Purely internal refactoring
