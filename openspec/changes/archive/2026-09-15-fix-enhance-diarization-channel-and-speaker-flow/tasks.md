## 1. Reliable channel-layout detection

- [x] 1.1 Add a shared channel-layout detection in `frontend/src-tauri/src/audio/decoder.rs` that reads the channel count from the decoded audio (first decoded packet's spec) when header metadata is missing or disagrees, with no `unwrap_or(1)` mono default. Verify: a unit test on a stereo file whose metadata omits the channel count returns 2.
- [x] 1.2 Preserve the header-only fast path for files whose metadata already reports channels, so no decode is paid in the common case. Verify: existing `test_probe_audio_metadata_returns_channels` passes and a probe on a channel-reporting file does not decode packets.

## 2. Offline diarization channel split

- [x] 2.1 Replace the `channels == 2` gate in `run_diarization_blocking_with_app` (`audio/diarization.rs`) with the decoded layout, and when the layout is stereo always spawn independent left/right streams — never `ffmpeg -ac 1`. Verify: an integration test over a stereo AAC/MP4 fixture produces `MIC_SPEAKER_*` (channel=`mic`) and `SPEAKER_*` (channel=`system`) rows.
- [x] 2.2 Handle an undetectable layout without downmixing (pass the native stream and split it) and emit an actionable error if the stream cannot be interpreted. Verify: a unit test simulating unknown layout asserts no `-ac 1` argument is produced.
- [x] 2.3 Ensure `is_stereo` passed to `compute_speaker_matches` and `persist_and_recognize_session` matches the decoded layout. Verify: diarizing a stereo meeting writes no `SPEAKER_%` clusters tagged `channel='mic'`.

## 3. Word-alignment per-channel span source

- [x] 3.1 Update `meeting_span_source` (`audio/recording_commands.rs`) to use the decoded layout instead of the metadata probe. Verify: a stereo file with missing metadata maps a `source_device="System"` segment's span to the right channel.
- [x] 3.2 Confirm offline refinement and the N-way token split still operate per channel. Verify: offline re-diarization of a stereo meeting refines tokens per channel and splits cross-speaker rows at token boundaries.

## 4. Retranscription re-establishes speaker attribution

- [x] 4.1 Carry per-block user overrides (`transcripts.speaker_override_id`) forward by time overlap onto the newly inserted rows after retranscription commits. Verify: a test asserts a confirmed block keeps its identity across retranscription without user action.
- [x] 4.2 Auto-run the channel-correct offline diarization after a successful retranscription when diarization is enabled with auto-run; otherwise mark the meeting as needing analysis and surface the existing speaker-analysis affordance. Verify: after Enhance with auto-run on, speaker labels and provenance render without pressing Speakers.
- [x] 4.3 Ensure stale `meeting_speakers` rows whose cluster labels are not produced by the new run do not drive displayed attribution, confidence, or confirmation state. Verify: immediately after Enhance and before re-analysis, the transcript shows no `(auto)` suffix or confidence for labels with no fresh cluster.

## 5. Sequencing and UI state

- [x] 5.1 Add a shared guard so diarization cannot start while retranscription is writing, and so the post-retranscription re-run is sequenced after the insert transaction commits. Verify: an attempt to start diarization during retranscription returns a busy error and does not interleave writes.
- [x] 5.2 Reflect the busy/re-analysis state in the frontend controls (Speakers disabled or waiting while retranscription runs). Verify: manual UI check that pressing both controls in sequence yields one ordered run, not a race.

## 6. Regression verification

- [ ] 6.1 Re-run speaker analysis on a previously mono-confused meeting (for example `Meeting 2026-09-11_16-21`) and confirm the result has `MIC_SPEAKER_*` plus `channel='system'` rows and no system-channel cluster bound to a microphone-only identity. Verify: DB query over `meeting_speakers` for that meeting.
- [ ] 6.2 Run the audio test suite and the change validator. Verify: `cargo test` (audio modules) and `openspec validate fix-enhance-diarization-channel-and-speaker-flow` both pass.
