# Design: simplify-model-diagnostics

## Context

The telemetry panel is poll-based: `page.tsx` samples `get_recording_telemetry` every ~150 ms into `recordingTelemetry`, rendered by `DiarizationStatusLines.tsx` with pure formatters in `lib/diarization-status-lines.ts`. Backend state lives in atomics in `audio/telemetry.rs` (`ModelsActivity`: vad/asr/alignment/diarization) plus `online_diarization.rs` `ChannelStatusLine`. The existing spec `online-diarization-telemetry` mandates detail that this change removes (status content beyond level/queue, buffer bars, tooltip context), so the delta modifies/removes those requirements.

## Goals / Non-Goals

**Goals:**
- Keep per-channel dB level bars as-is.
- STT queue counter: use existing `AsrActivity.pending` (queued − completed), cleared automatically as the worker drains the queue.
- New diarization pending-block gauge in `online_diarization.rs` stats (enqueued atomics, decrement on process) surfaced through `ChannelStatusLine`.
- Blink indicators: extend `ModelsActivity` per-model with `requested` and `processing` flags; frontend maps them to blinking green/red CSS animation.

**Non-Goals:**
- Summary/LLM provider telemetry (nothing exists yet; separate change).
- Changing the 150 ms poll interval or moving to events.
- Any pipeline behavior change — instrumentation only.

## Decisions

1. **Track "requested" via monotonic sent counters, "processing" via pending>0/in-flight flag.** Each model instrument gets `sent_total` and `completed_total` atomics plus `in_flight` (ASR already has `queued`/`completed`; pending>0 ⇒ blinking green mapping for "processing"). Red-blink when sent_total > completed_total but nothing is actively in flight is hard to observe at 150 ms; instead: red-blink while a request was submitted this session and the model is not currently processing it (sent>done ∧ in_flight=0). Rationale: distinguishes "waiting model" from "working model". Alternative (timestamp-based flash) rejected as unnecessary state.
2. **Keep the remaining data poll-based.** New fields ride the existing snapshot chain: `telemetry.rs` → `recording_commands.rs` (`get_recording_telemetry`) → `diarizationStatusService.ts` → formatters. Alternative (Tauri events per block) violates the spec's no-per-chunk-events rule.
3. **Frontend simplification is display-level.** Delete buffer-bar builders (`buildBufferBars`, gated-bar rendering) and diarization counter text / tooltip `modelLines` from `DiarizationStatusLines`/formatters; backend channels keep their `ChannelStatusLine` struct but only level + pending-diary-block fields are rendered. Alternative: strip the backend struct too — deferred to keep the diff surgical; backend fields are cheap atomics.
4. **CSS blink via Tailwind arbitrary animation** (`animate-pulse` or a small custom keyframe class) toggled per indicator state; no new dependencies.
5. **VAD indicator**: VAD processes every frame, so "requested" is meaningless; its indicator blinks green while speech is detected (`speaking` flags), steady green otherwise when pipeline installed.

## Risks / Trade-offs

- [Archive conflict: MODIFIED requirements replace text] → delta includes full updated requirement blocks so spec merge stays consistent.
- [Blink visibility at 150 ms poll for very fast ops (VAD speech)] → VAD speaking period spans many polls; STT/diarization blocks take ≥ hundreds of ms.
- [Removing detail loses debug info (embed failures, threshold context)] → Mitigation: counters remain in backend logs / debug output; a future change can re-add an opt-in detail view.
- [Diarization pending gauge gets out of sync on error paths] → increment on enqueue, decrement in process path AND on drop/error path; gauge clamps at 0 in snapshot builder.
