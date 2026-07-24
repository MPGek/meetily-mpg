## Context

Live mode transcribes each VAD segment as an isolated Whisper call (`pipeline.rs:845` → `worker.rs:458`). No context is preserved between segments: no audio merging, no previous-text prompt. With the unified 200ms redemption time, a 500ms speaker pause fragments the audio into separate segments. Whisper receives each fragment independently — missing the first 30-50ms of speech onset (VAD fires late on soft consonants) and lacking linguistic continuity from previous output.

Enhance mode avoids this through `merge_segments` (gap < 2000ms), which joins adjacent VAD fragments into coherent 10-25s chunks before Whisper. The live pipeline needs the same pattern, adapted for real-time constraints.

## Goals / Non-Goals

**Goals:**
- Apply segment merging with 500ms gap threshold to the live VAD pipeline before transcription dispatch
- Pass previous transcript text as Whisper `condition_on_previous_text` prompt for subsequent segments from the same source
- Reuse `merge_segments` from `vad.rs` with a live-mode-specific gap threshold
- Keep per-segment minimum at 1600 samples (100ms) to match enhance mode

**Non-Goals:**
- Changing the VAD processor or its output format
- Modifying Whisper engine internal parameters beyond enabling prompt forwarding
- Adding a new VAD mode or configuration struct
- Changing the enhance/retranscription flow
- User-configurable merge gap threshold

## Decisions

### Decision 1: Inline merging in the pipeline run loop

The `AudioPipeline::run()` loop already receives VAD segments from `process_audio()` and dispatches them via `transcription_sender`. The change is to accumulate segments into a temporary Vec, periodically call `merge_segments(segments, 500.0, 25*16000)`, and dispatch merged chunks.

**Dispatch trigger:** Dispatch the first completed merge group when either:
1. A VAD segment arrives whose gap from the previous segment is ≥ 500ms → dispatch all accumulated merged segments
2. A VAD segment arrives whose gap from the previous segment is ≥ 500ms and the accumulated audio exceeds 25s → split and dispatch

**Alternative: Per-segment merging in a separate thread.** Rejected — adds complexity (new channel, new task) for marginal benefit. The pipeline loop is single-threaded and can handle merge logic inline.

### Decision 2: 500ms gap threshold

500ms was chosen because:
- Google STT streaming: `speech_end_timeout = 500ms`
- Azure STT: `endSilenceTimeoutMs = 500ms`  
- Natural speech: breath/thinking pauses are 200-400ms, end-of-turn pauses are 600-1200ms
- 500ms splits at utterance boundaries (end-of-thought) while bridging "um" and "uh" pauses

**Alternative: 2000ms (same as enhance).** Rejected — 2000ms means up to 2s extra latency in live mode, which degrades real-time UX.

**Alternative: 250ms.** Rejected — too close to the VAD's 200ms redemption; would merge segments that are legitimately separate utterances.

### Decision 3: condition_on_previous_text via initial_prompt

The transcription worker tracks the last transcript text per source device (Microphone, System). When a new segment arrives, it passes the previous text as `initial_prompt` in the Whisper params.

```
worker.rs state:
  last_mic_text: String    ← updated after each mic transcription
  last_sys_text: String    ← updated after each sys transcription

on new segment from mic:
  whisper.transcribe(audio, language, initial_prompt=last_mic_text)
```

The prompt is reset at recording start (`reset_speech_detected_flag` already exists; add prompt reset there).

**Alternative: Full conversation history.** Rejected — Whisper's prompt context window is limited; accumulating the entire session history would overflow. Last single segment is sufficient for phrase-level continuity.

### Decision 4: No changes to merge_segments function

`merge_segments` in `vad.rs` is already parameterized with `max_gap_ms` and `max_duration_samples`. The live pipeline calls it with `(500.0, 25*16000)`. No code changes to the function itself.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| 500ms merge delay adds latency | The 500ms threshold is a gap between VAD segments, not an artificial delay. Natural speech pauses of 500ms already mean the user isn't speaking, so waiting 500ms to confirm the pause ends an utterance doesn't add user-facing latency |
| Merged segments grow arbitrarily large | Split cap of 25s enforced by `merge_segments` |
| previous-text prompt causes repetition loops | Whisper's `condition_on_previous_text` with temperature ≤ 0.5 is stable; the existing `prompt_reset_on_temperature` mechanism prevents loops |
| Thread safety of cached prompt text | Transcription worker is single-threaded (`NUM_WORKERS = 1`); simple mutable state is sufficient |
