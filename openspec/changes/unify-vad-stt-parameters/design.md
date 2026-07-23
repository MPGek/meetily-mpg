## Context

VAD parameters are currently scattered across 4 files with 2 duplicated constants and 8 hardcoded values inside `ContinuousVadProcessor::new()`. The only externally configurable parameter is `redemption_time_ms`, which has diverged (400ms for live, 2000ms for batch) because the same VAD is used for both real-time streaming and full-file batch processing — two fundamentally different use cases with different requirements for silence tolerance.

STT parameters in `whisper_engine.rs` are partially adaptive (via `HardwareProfile`) but 7 additional parameters are hardcoded inline with no mechanism to vary them per mode.

## Goals / Non-Goals

**Goals:**
- Single source of truth for all VAD parameters
- Identical VAD behavior for the core Silero model across live and batch modes
- Mode-specific post-processing (segment merging for batch, immediate dispatch for live)
- Reduce zero-silence padding to improve transcription quality
- Lift hardcoded STT parameters into a structured config

**Non-Goals:**
- User-facing VAD tuning UI or database persistence
- Changing the VAD model itself (Silero v6 ONNX is unchanged)
- Modifying how transcription engines receive audio data
- Changing segment emission format or Tauri events

## Decisions

### Decision 1: Unified core VAD + mode-specific merger

**Alternative A: Keep different redemption values per mode.** Rejected — the redemption parameter governs the VAD's internal state machine, not the segment grouping logic. Using 2000ms redemption means the VAD itself leaks across natural pauses, producing segments with long internal silences that degrade Whisper quality.

**Alternative B: Two-layer architecture (VAD → merger).** Chosen. The VAD runs at 200ms redemption for both modes, producing tight, accurate speech boundaries. Batch modes (retranscription, import) apply a post-VAD merger that combines adjacent segments where the inter-segment gap < 2000ms.

**Why 200ms?** Silero's `VADIterator` (streaming API) defaults to 100ms. faster-whisper's batched mode uses 160ms. 200ms bridges micro-pauses (lip smacks, stutters) while keeping the live-mode end-to-end latency under 1.5 seconds.

### Decision 2: `VadConfig` struct with presets

```rust
pub struct VadConfig {
    pub threshold: f32,          // 0.50
    pub neg_threshold: f32,      // 0.35
    pub min_speech_ms: u32,      // 250
    pub redemption_ms: u32,      // 200
    pub pre_pad_ms: u32,         // 150
    pub post_pad_ms: u32,        // 150
    pub min_segment_samples: usize, // 1600 (100ms at 16kHz)
    pub max_segment_samples: Option<usize>, // None (live) or 25*16000 (batch)
}

impl VadConfig {
    pub fn live() -> Self { ... }
    pub fn batch() -> Self { ... }
}
```

**Alternative: Builder pattern.** Rejected for now — only 2 use cases with no evidence that per-call tuning is needed. If future needs arise, builder can wrap the existing presets.

### Decision 3: Reduced padding (300/400 → 150)

**Why?** Current padding injects 300ms of zeros before speech and extends segments 400ms after. This is 13× the Silero reference (30ms). The zeros are a lie to Whisper — they create artificial silence prefixes that can trigger `no_speech_threshold` or cause the model to misinterpret the leading context. 150ms is sufficient for Whisper's spectrogram padding (250ms mel window) while minimizing fake silence.

**Alternative: Keep 400ms.** Rejected — the original Silero `get_speech_timestamps` uses 30ms because it returns timestamps for slicing real audio. This codebase pads with zeros, so 30ms is insufficient. But 150ms is the sweet spot observed across Whisper.cpp benchmarks.

### Decision 4: Segment merger for batch modes

```rust
fn merge_segments(segments: &[SpeechSegment], max_gap_ms: u32, max_duration_samples: usize) -> Vec<SpeechSegment>
```

Merges adjacent segments where `segments[i+1].start - segments[i].end < max_gap_ms` (2000ms). If merged duration exceeds `max_duration_samples` (25s at 16kHz), splits at the largest silence gap within that chunk.

**Rationale:** Replaces the 2000ms redemption hack with explicit post-processing. The VAD output is clean; the merger handles chunking for optimal Whisper input sizes.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| 200ms redemption produces too many live-mode segments, increasing CPU load | VAD dispatch is already window-batched at 200ms; 200ms redemption means 1 segment per ~5 windows. Worst case: 5 segments/second for continuous speech. Each ~3-8s segment is well within Whisper's processing budget |
| 150ms padding causes Whisper to miss first/last phonemes | The 150ms value was chosen because Whisper's mel spectrogram uses 25ms hop length with a 400-sample Hann window — 150ms provides 6 frames of context, sufficient for the model to lock onto the speech onset |
| Merger produces segments > Whisper's ideal length (30s) | Split threshold is 25s, well under the 30s Whisper context window. The merger's silence-split logic mirrors faster-whisper's `max_speech_duration_s` behavior |
| Changing tested VAD defaults breaks existing meeting quality | The unit test `test_vad_400ms_vs_2000ms_segmentation` validates that 2000ms produces fewer segments than 400ms — this logic is preserved, just moved from VAD-level to merger-level |

## Open Questions

None — all parameters have been validated against Silero v6 defaults, faster-whisper defaults, and industry standards (Google/Azure STT).
