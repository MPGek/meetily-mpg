## Context

The `ContinuousVadProcessor` processes audio in 512-sample windows at 16kHz (32ms per window). When speech is detected (VAD probability crosses 0.50), the processor starts accumulating audio in `current_speech`. However, the processor has no memory of previous windows — once a window is processed and doesn't trigger speech detection, it's discarded.

The current `pre_speech_pad` logic attempts to backfill audio before the detection point, but it only works in the first ~150ms of recording. After that, it calculates `padding_needed = 0` and adds nothing. The comment at vad.rs:600 says "we don't have raw audio before our buffer" — which is the problem.

## Goals / Non-Goals

**Goals:**
- Maintain a rolling buffer of the last N processed audio windows in `ContinuousVadProcessor`
- When speech is detected, prepend the buffered audio to `current_speech` to recover speech onset
- Keep the buffer size configurable via `VadConfig` (default: 10 windows = 320ms)
- Minimal performance impact — circular buffer operations should be O(1) per window

**Non-Goals:**
- Changing the VAD detection algorithm or thresholds
- Modifying the segment merger logic (that's already implemented in live-transcription-context)
- Adding user-configurable buffer size via UI (use VadConfig defaults)
- Changing the enhance mode behavior (it already works better due to merging)

## Decisions

### Decision 1: Circular buffer with fixed capacity

Use a `VecDeque<f32>` as a circular buffer with a fixed capacity of `buffer_windows * 512` samples (default: 10 × 512 = 5120 samples = 320ms at 16kHz).

**Why VecDeque?** It supports efficient push_back and pop_front operations, which is exactly what we need for a sliding window. The capacity is fixed, so memory usage is bounded.

**Alternative: Ring buffer with index tracking.** More complex to implement, no performance benefit for our use case. VecDeque is simpler and sufficient.

### Decision 2: Buffer size of 10 windows (320ms)

10 windows covers the typical speech onset delay (30-50ms for soft consonants) with margin. This matches the `pre_speech_pad_samples` (2400 samples = 150ms) plus extra buffer for safety.

**Alternative: Match pre_speech_pad_samples exactly (150ms = ~5 windows).** Risky — if the VAD detection is delayed by more than 150ms, we still lose audio. 320ms provides a safety margin.

### Decision 3: Prepend buffer to current_speech on speech detection

When speech is detected (vad.rs:582-605), instead of:
```rust
self.current_speech.clear();
self.current_speech.resize(padding_needed, 0.0);  // zeros
self.current_speech.extend_from_slice(chunk);
```

Do:
```rust
self.current_speech.clear();
self.current_speech.extend_from_slice(&self.audio_history);  // real audio from buffer
self.current_speech.extend_from_slice(chunk);  // current window
```

This recovers the 30-50ms of speech onset that was previously lost.

### Decision 4: Update audio_history after processing each window

After processing each 512-sample window in `process_chunk`, update the buffer:
```rust
// Add current window to history
self.audio_history.extend(chunk);
// Keep only the last buffer_capacity samples
while self.audio_history.len() > self.buffer_capacity {
    self.audio_history.pop_front();
}
```

This ensures the buffer always contains the most recent audio.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| Increased memory usage (~40KB for 2 processors) | Negligible — well within typical app memory budget |
| Performance overhead from VecDeque operations | Minimal — push_back/pop_front are O(1) amortized. We process ~5 windows per second per processor, so ~10 operations/sec total |
| Buffer contains audio from previous speech segment | Acceptable — the buffer is only prepended when starting a new segment after a pause. If the previous segment ended recently, the buffer might contain tail audio, but that's better than missing onset |
| Edge case: first speech detection has no buffer | Handled — if `audio_history` is empty, we prepend nothing (same as current behavior) |

## Open Questions

None — the approach is straightforward and the implementation is localized to `ContinuousVadProcessor`.
