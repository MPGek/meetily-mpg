## Context

The VAD engine lives in `audio/vad.rs` (ContinuousVadProcessor) and delegates to the `silero_rs` crate's `VadSession`, which loads a statically embedded ONNX model. The current model is Silero VAD v4 (or early v5) — pre-dating the v5/v6 improvements. The `silero_rs` crate (emotechlab fork, commit `26a6460`) uses `ort` ONNX runtime and manages LSTM state as separate `h` and `c` tensors each of shape `[2, 1, 64]`, processes 30ms (480 sample) windows, and has no context mechanism.

The v6 ONNX model changes:
- LSTM state doubled to 128 and combined into a single `state` tensor
- Fixed 512-sample window at 16kHz (not configurable)
- Requires 64 samples of context from previous chunk prepended to each input
- Batch dimension preserved in state (irrelevant for single-stream usage)

## Goals / Non-Goals

**Goals:**
- Replace the ONNX model with Silero VAD v6 weights
- Update the inference wrapper to handle v6 model architecture (combined state, fixed 512-sample window, context prepending)
- Keep the `ContinuousVadProcessor` API identical — no changes to callers in `pipeline.rs`, `retranscription.rs`, `import.rs`
- Preserve all tuned parameters (0.50/0.35 thresholds, 300ms pre-pad, 400ms post-pad, 250ms min speech, configurable redemption time)
- Maintain resampling from arbitrary input rates to 16kHz via rubato

**Non-Goals:**
- No new VAD capabilities or features
- No changes to VAD configuration schema or defaults
- No changes to `SpeechSegment` struct or public API
- No changes to existing VAD-related specs (`per-channel-vad`, `independent-vad`)

## Decisions

### Decision 1: Replace `silero-rs` with an inline wrapper (Option B from proposal)

**Chosen:** Write a `VadSession` replacement directly in the project using `ort`, removing the `silero_rs` dependency.

**Rationale:**
- The `silero_rs` crate is a third-party fork that would need to be forked again and updated — ongoing maintenance burden
- The v6 model changes are significant enough that most of the crate's code would be rewritten
- An inline wrapper is ~200 lines, simple to maintain, and has zero external coupling
- The project already uses `ort` transitively through other deps, so no new crate dependency

**Alternatives considered:**
- Fork and update `silero_rs`: More complex, adds a git dependency to maintain, and the existing crate's structure (separate h/c, 30ms windows) fights the v6 model
- Use Python sidecar via `uv`: Adds deployment complexity, latency for chunk-by-chunk inference, and a Python runtime dependency

### Decision 2: Use `ort` directly with combined state tensor

**Chosen:** The wrapper will maintain a single `state` tensor `[2, 1, 128]` (f32), initialized to zeros, and pass it as the `state` input. The updated `state` is extracted from model outputs.

**Rationale:** v6 merges LSTM hidden and cell states into one tensor. The ONNX model exports `state` as a concatenation of `[h, c]` across dimension 0, so the first 64 elements per batch-item are h, the last 64 are c. `ort` handles the split internally in the ONNX graph — we just pass and receive the combined tensor.

### Decision 3: Context management

**Chosen:** Maintain a 64-sample circular buffer (`VecDeque<f32>`) that stores the last 64 samples of the previous chunk. Each 512-sample input chunk is prepended with these 64 samples to form a 576-sample input tensor.

**Rationale:** Matches the official Python `OnnxWrapper` in v6 exactly. Without context, the first chunk of each utterance would lack the conditioning the model was trained with, producing slightly lower probabilities.

### Decision 4: Window size change (480 → 512 samples)

**Chosen:** Change `ContinuousVadProcessor.chunk_size` from 480 to 512. The v6 model requires exactly 512 input samples at 16kHz — unlike v4 which accepted 30ms (480 samples), v6 has a hardcoded 512-sample window.

**Impact on callers:** `process_audio()` and `process_chunk()` internal logic uses `self.chunk_size` for the drain loop. No caller-visible changes.

### Decision 5: Model distribution — download at build time

**Chosen:** Download the v6 ONNX model at build time via a `build.rs` script and embed it statically.

**Rationale:**
- The model is ~2 MB; embedding avoids runtime file-not-found issues
- `uv run --with silero-vad==6.0 ...` in `build.rs` extracts the model from the Python package
- Falls back gracefully if `uv` is not available (download from GitHub releases as alternative)

**Alternatives considered:**
- Git LFS in the repo: Increases clone size, adds LFS dependency
- Download at runtime: Slower first run, error-prone on first startup

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Before (current)                                             │
│                                                               │
│  vad.rs: ContinuousVadProcessor                               │
│    └─► silero_rs::VadSession                                  │
│          ├─ h: Tensor [2,1,64]   ──┐                           │
│          ├─ c: Tensor [2,1,64]   ──┤ LSTM state (separate)    │
│          ├─ input: [1, 480] samples │ 30ms @ 16kHz             │
│          └─ no context             ──┘                           │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  After (v6)                                                   │
│                                                               │
│  vad.rs: ContinuousVadProcessor                               │
│    └─► VadSessionV6 (new inline, no silero_rs)                │
│          ├─ state: Tensor [2,1,128]  ──┐                       │
│          │  (combined h+c)             │ LSTM state doubled   │
│          ├─ input: [1, 576] samples    │ 512 + 64 context     │
│          └─ context: VecDeque<f32>[64] ──┘                     │
└──────────────────────────────────────────────────────────────┘
```

### Data flow (per-chunk)

```
[512 samples] → prepend 64 context samples → [576 samples]
    → ort Session::run(input=[1,576], state=[2,1,128], sr=int64)
    → {prob: f32, new_state: [2,1,128]}
    → save last 64 samples as context for next chunk
    → save new_state for next chunk
    → return prob
```

### Key integration points

| File | Change |
|---|---|
| `audio/vad.rs` | Replace `VadSession` with inline `ort` session. Update chunk_size to 512. Add context buffer. |
| `Cargo.toml` | Remove `silero_rs` dependency, keep `ort` (already present or ensure it's available) |
| `build.rs` | Download/extract v6 ONNX model from silero-vad Python package |
| `vad.rs` tests | Verify v6 probabilities match Python reference on known audio |

## Risks / Trade-offs

| Risk | Likelihood | Mitigation |
|---|---|---|
| v6 model produces different segmentation boundaries on existing audio | Medium | Accept as quality improvement. Review test snapshots and update thresholds if regression occurs. |
| `ort` runtime error with opset 16 model | Low | Test model loading in CI. `ort` 2.x supports opset 16+. |
| Build-time model download fails (no uv, no network) | Low | `build.rs` falls back to embedded placeholder with compile error. Document `uv` prerequisite. |
| Context prepending changes timing of first chunk in flush | Low | On flush, no context is prepended — consistent with Python behavior. Verify in tests. |
| Model file size increase (1.8MB → ~2-3MB) | Low | Acceptable for a desktop app. Binary size impact is one-time. |

## Open Questions

- [ ] What is the exact file size of the v6 ONNX model? (Need to extract it via `uv` to confirm)
- [ ] Does the v6 model support 8kHz sampling rate natively, or must we always resample to 16kHz? (v5 added 8kHz support, v6 likely retains it, but `VAD_SAMPLE_RATE` constant is 16000 — no change needed)
- [ ] Should we pin to v6.0 or v6.2 (latest)? v6.2 has "significant quality improvements on edge cases" — recommend v6.2 for best quality.
