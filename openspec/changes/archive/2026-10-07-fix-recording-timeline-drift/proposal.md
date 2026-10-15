# Proposal

## Why

The saved `audio.mp4` runs slower than the clock, so it drifts away from the transcript. Transcript times are anchored to elapsed recording time (`pipeline.rs:1098-1141`), while a position in the file is just a count of saved samples. In `Meeting 2026-10-05_15-01` the gap grows by about 0.83 s per minute: +8 s at 10 min, +20 s at 24 min. Clicking ▷ on the block at 24:10 plays speech from about 23:50. Every recent two-channel recording shows the same thing: files are 0.6-1.1 % longer than the session (+65 s on the 2 h 17 min `2026-10-02_16-16`). Anything that cuts the saved file by transcript time inherits the offset: play-from-block, voiceprint clips (`voiceprint_clips.rs`) and the offline speaker passes.

Two causes, both measured:

- **Mixer padding, ~1.25 %.** `AudioMixerRingBuffer::extract_window` (`pipeline.rs:158-211`) cuts a 600 ms window as soon as *either* channel has 600 ms. It empties the other channel and fills the rest of its window with zeros. With both streams live and slightly out of phase, every window adds a few milliseconds of silence to whichever side is behind. The file shows these dips at the window boundaries, in both channels and at the same phase. A model of the exact code gives +0.8 to +1.6 % for 10-20 ms chunks with normal jitter, and +8.9 % for 100 ms bursts.
- **AAC priming per checkpoint, ~0.12 %.** Every 30 s checkpoint is a separate AAC encode, and `ffmpeg -f concat -c copy` keeps each one's 1024-sample priming plus the 768-sample padding of its last frame. That is +37.3 ms per checkpoint, which matches the drift of the window grid between checkpoints in the real file. No `-c copy` or concat-demuxer variant removes it.

## What Changes

- The mixer cuts a window only when **both** channels have a full window. A short channel keeps its samples as the start of its next window.
- A channel counts as **starved** only when the other channel is a full window (600 ms) or more ahead of it. Then the starved channel is padded with silence so recording continues: WASAPI loopback sends nothing while output is silent, and the mic sends nothing during a hot-swap gap.
- When a starved channel delivers again, it is lined up with the live channel's buffered tail, so it lands at the moment it resumed. The fixed offset of up to the slack is avoided, and no capture timestamps are used. Zeros added this way count toward the mic-gap padding, so the existing mic gap fill only covers time when no window was cut at all.
- At stop, the mixer's remaining partial window is saved. Today up to 600 ms are dropped.
- Checkpoints are written lossless (FLAC, 24-bit) instead of AAC. The final `audio.mp4` is encoded once from the concatenated checkpoints at stop, sample-exact, with no double AAC encoding. Crash recovery does the same for FLAC checkpoints and keeps the old `-c copy` merge for checkpoint folders left by older versions.
- The merge/encode timeout, and the stop flow's outer save timeout, scale with the recorded length. The final encode takes time proportional to the recording: 9.5 s for 27 min measured on the dev machine.

Out of scope (follow-ups):

- Re-aligning meetings already recorded with drift (would rewrite row times in the DB).
- Pause: transcript times include paused time (`get_recording_duration()` = `start.elapsed()`), but paused audio is not saved. This follows from the code; it has not been checked on a real paused recording.
- `metadata.duration_seconds` and the saved-duration check get `None` because `RecordingState::cleanup()` clears `recording_start` before `save_recording_only` (`recording_manager.rs:314`).
- Progress feedback during the longer finalize step.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `stereo-recording`: new requirement that the saved stereo file stays on the session timeline. Silence goes into a channel only while that channel sends no audio. A channel that resumes lines up with the other. The final partial mixer window is kept.
- `audio-encoding`: new requirement that merging checkpoints, at stop and in crash recovery, keeps the recorded sample count with no per-checkpoint encoder delay or padding.
- `recording-save-durability`: the "Encoder and merge operations are time-bounded" requirement changes. Merge and finalize timeouts scale with the recording length instead of a fixed 2 min / 5 min, so a long meeting is not cut off while its final file is encoded.

## Impact

- Backend (`frontend/src-tauri/src/audio/`):
  - `pipeline.rs`:
    - `AudioMixerRingBuffer` cut and starvation rule, resume alignment
    - ordering with `close_mic_gap`
    - saving the tail at pipeline end
    - tests
  - `incremental_saver.rs`:
    - FLAC checkpoint files
    - concat-and-encode merge
    - recovery for both formats
    - `has_audio_checkpoints` accepts `.flac`
  - `encode.rs`:
    - lossless checkpoint encode with the same sanitization and timeout
    - length-scaled merge timeout
  - `recording/stop.rs`: outer save timeout derived from the merge timeout.
- The saved file format (AAC-LC MP4, 48 kHz stereo, L = mic, R = system) and the bitrate do not change. Temporary `.checkpoints/` usage grows from ~0.6 MB/min to ~4 MB/min while recording.
- Stop takes longer by the final encode: about 0.35 s per minute of recording on the dev machine, 2-3× that on slower hardware.
- No new dependencies, DB, settings or IPC changes. The frontend is unchanged: the recovery IPC keeps its shape.
