# Design

## Context

See proposal.md (Why) for the measurements. The current live path per channel, in `audio/pipeline.rs`:

```
device chunk -> per-channel buffer -> every 200 ms: ContinuousVadProcessor::process_audio
   -> new SpeechSegment(s) (reported only when speech ENDS: >= 200 ms silence after >= 250 ms speech)
   -> remap_segment_times_to_real(anchors)        (VAD-counter ms -> recording-time ms)
   -> if first new segment starts >= 500 ms after pending tail: flush_pending_segments
   -> pending.extend(new); if sum(pending) > 25 s: flush_pending_segments
recording stop -> flush_remaining_audio (VAD flush + flush_pending_segments)
```

`flush_pending_segments` merges with `merge_segments(pending, 500.0, 25 s)`, drops pieces below `min_segment_samples`, and sends `AudioChunk`s to the transcription worker and the embedding channel.

Two clocks are involved. Segment times in `pending` are recording time (after `remap_segment_times_to_real`). `ContinuousVadProcessor::processed_ms()` is VAD-counter time. The two differ by the per-dispatch anchors that absorb dropped device samples.

Telemetry: `ChannelPipelineStats::set_pending(segments, buffered_ms)` and `set_pending_triggers(500, 25_000)` already exist and reach the frontend through `get_recording_telemetry` every 150 ms. The frontend does not render them.

## Goals / Non-Goals

**Goals:**
- Dispatch pending speech within one VAD window (200 ms) after the 500 ms trigger, with chunk contents identical to today.
- Report enough per-channel state for the tank, the ring, and the stuck alert, without per-chunk events.

**Non-Goals:**
- Changing VAD parameters, the 500 ms gap, or the 25 s cap.
- Changing the transcription worker, the embedding path, or re-transcription.

## Decisions

### D1. Silence check runs after every VAD dispatch, in recording time

After `process_audio` returns and the existing gap/cap logic has run, the pipeline checks the channel:

```
tail      = end of the last pending segment            (recording ms, as stored)
start     = recording(vad.current_speech_start_ms())   if speech is in progress
earliest  = recording(vad.processed_ms() - pre_pad_ms) - ANCHOR_JITTER_MS   otherwise
if pending non-empty and (start or earliest) - tail >= 500: flush_pending_segments(channel)
```

- The gap rule compares segments in recording time (after `remap_segment_times_to_real`), so this check does too. `recording()` applies the shift of the latest anchor at or before the counter time, exactly as the remap will when the segment is emitted (`anchor_shift_ms`, shared with the remap).
- When speech is in progress, its padded start is known; if it lies ≥ 500 ms after the tail, the gap rule would split there anyway, so the pending speech goes now instead of when that speech ends.
- When the channel is silent, speech detected later starts at least `pre_pad_ms` (150 ms) before the current position, and a later anchor's shift can be up to one VAD frame (32 ms) smaller than the current one (the samples the VAD holds back between frames). So the silent branch waits 500 + 150 + 32 ms after the tail. With less, speech starting just after the flush could still reach back to within 500 ms of the tail; the old rule would have merged it and the chunks would differ. The decision is the pure function `silence_gap_ms`.
- An earlier draft decided in VAD-counter time. Replaying the 30-minute meeting showed 195 system chunks against 191 with the old rule: the per-dispatch anchor shift jitters by up to one frame, so pauses near 500 ms split differently. In recording time with the jitter margin the replay is identical (D6).

Alternatives:
- *A wall-clock timer per channel.* Rejected: it needs a second loop and a lock on pending state. The VAD dispatch already runs every 200 ms while a channel delivers audio. A WASAPI loopback channel may deliver nothing while output is silent (see the anchor comment on `vad_*_anchors`); then the silence check does not run for it. On the measured meeting this did not happen: a replay that assumes continuous VAD on both channels reproduces the real lags (p99 151 vs 152 s, max 214 = 214 s), which would not hold if the system channel had stopped. The residual case is shown by the stuck indicator (D4).
- *Lower `redemption_ms` so segments close faster.* Rejected: it changes segmentation and therefore WER, and it does not fix the wait for the next segment.

### D2. VAD exposes in-progress speech read-only

`ContinuousVadProcessor` gets two getters: `current_speech_start_ms() -> Option<f64>` (padded start, VAD-counter ms, `None` when not in speech) and `current_speech_ms() -> f64` (`processed_ms - start` while in speech, else 0). They do not change VAD state. `is_in_speech()` and `processed_ms()` already exist.

### D3. Two telemetry fields, computed where the pipeline already updates telemetry

`PendingState` gains `open_speech_ms: u64` (D2's length) and `silence_ms: u64` (D1's gap for a silent channel with pending speech, clamped at 0, else 0), so the ring is full exactly when the flush fires. They are set in the same place as `set_pending`, at the same per-dispatch cadence. Nothing is emitted per chunk; the 150 ms poll reads the atomics (spec "Non-interference", "Status refresh rate").

### D4. Indicator (variant A) is pure frontend state over the snapshot

`diarization-status-lines.ts` gets a `buildBufferTank(pending)` that returns:

- `pendingFraction = buffered_ms / cap_trigger_ms`, `openFraction = open_speech_ms / cap_trigger_ms` (clamped together to 1);
- `ring = silence_ms / gap_trigger_ms` (clamped to 1);
- `stuck = segments > 0 && silence_ms > gap_trigger_ms + STUCK_SLACK_MS`, with `STUCK_SLACK_MS = 350` (one 200 ms VAD window plus one 150 ms poll);
- a label: speaking / `N.N/25 s` / `silence NNN/500 ms` / `waiting for speech N.N s`.

`ChannelRow` renders the tank (pending amber, in-progress blue, stuck red) and a 12 px SVG ring (16 px would grow the 10 px line) next to the existing level bar, with the label as `title`; a stuck channel's text reads "waiting for speech". Colours follow the mockup chosen in exploration. No new timer: it re-renders on the existing 150 ms snapshot.

### D5. Status lines shown for every recording

`RecordingControls.tsx` shows the block while the recording pipeline runs (`pipeline.sample_rate > 0`; the backend clears it when no session is running) instead of while online diarization is active (`active`, which stays diarization-only). With diarization off the snapshot reports mode `off` and the channels as `unavailable`; `buildModelIndicators` already maps that to the idle treatment, and the channel line drops its diarization text rather than reading "unavailable".

### D6. Tests use real speech and replay the old rule in the same pipeline

Silero scores synthetic tones as non-speech (p ≤ 0.02), so the pipeline tests use a short Windows TTS phrase (`tests/fixtures/tts_phrase_48k.wav`, 48 kHz mono). "Chunk contents unchanged" compares the pipeline with dispatch on silence against the same pipeline with it switched off by a `#[cfg(test)]` field: an offline model of the old rule differs by a few ms of inserted gap silence, because chunks are merged in recording time after anchoring. An `#[ignore]` test does the same on a real recording given by `MEETILY_REPLAY_AUDIO`.

## Risks / Trade-offs

- [More, shorter waits at the ASR worker] Phrases now reach the single transcription worker as soon as they end instead of in bursts. The total work is the same, the order is the same → none needed; the ASR pending count on the model row shows any queueing.
- [Online diarization sees chunks sooner] Same chunks, same order, earlier → no behaviour change expected; covered by the "Chunk boundaries unchanged" scenario.
- [Clock mismatch causes early or late flush] Comparing in a different clock than the gap rule splits pauses near 500 ms differently → D1 decides in recording time with the same anchor mapping; the late-start test and the real-recording replay cover it.
- [Silent channel delivers no audio] If a device stops delivering samples, VAD never runs and pending never flushes by silence → existing stop path still flushes; the stuck alert makes it visible.
- [Showing the lines with diarization off] Users who turned diarization off see status they did not see before → it is two small lines; the level bars and buffer are useful without diarization.

## Migration Plan

No data or settings migration. Rollback is reverting the change; saved transcripts are unaffected. After the change, re-measure display lag on a fresh recording with the `display_time` method from the proposal (expected p50 ≈ 0.5 s plus recognition time, no tail beyond a few seconds).
