## Why

Silero VAD v6 delivers 16% fewer errors on noisy real-world data and 11% fewer errors on multi-domain validation compared to the currently bundled v4 model. The existing implementation uses a forked `silero-rs` crate (emotechlab) with an outdated ONNX model, and the v6 model has breaking changes (larger LSTM state, fixed 512-sample windows, context prepending) that require inference wrapper updates. This is a quality-of-result improvement with no API changes.

## What Changes

- **BREAKING**: Replace the `silero_rs` crate's bundled ONNX model (`models/silero_vad.onnx`) with the official v6 ONNX weights
- **BREAKING**: Update the inference wrapper (`VadSession` in `silero-rs`) to handle v6's combined `state` tensor (size 128), fixed 512-sample windows, and context prepending
- Adjust `ContinuousVadProcessor` in `vad.rs` to use the new 512-sample (32ms) window size instead of 480-sample (30ms)
- All existing VAD configuration parameters (thresholds, pre/post-padding, redemption time) remain unchanged — only the inference engine improves

## Capabilities

### New Capabilities

None. This is a pure engine upgrade — no new user-facing capabilities are introduced. Behavioral spec-level requirements (independent VAD per channel, progress reporting, etc.) are unchanged.

### Modified Capabilities

None. The VAD behavior spec requirements do not change. The same thresholds, timestamps, and segment structures are produced with improved accuracy.

## Impact

- **Dependency**: The `silero-rs` crate (git pinned, emotechlab fork) must be updated or replaced. Two options: (A) fork and update the crate, or (B) write a direct `ort` wrapper in `vad.rs` and remove the dependency.
- **Model file**: The ONNX model at `silero-rs/models/silero_vad.onnx` (~1.8 MB) must be swapped for the v6 weights
- **Inference code**: `VadSession::forward()` and related state management must be rewritten to handle combined state tensor (128 vs 64), context prepending (64 samples), and 512-sample fixed windows
- **Resampling**: No change needed — `ContinuousVadProcessor` already resamples to 16kHz via rubato
- **Configuration**: No config changes needed — thresholds, padding, redemption time remain tuned as-is
- **Test impact**: VAD unit tests need validation against v6. Integration tests may show different segmentation boundaries (expected quality improvement)
