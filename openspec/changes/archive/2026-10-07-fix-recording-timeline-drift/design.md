# Design

## Context

See proposal.md (Why) for the measurements. The current state that shapes the approach:

**Mixer** (`audio/pipeline.rs`):
- `AudioMixerRingBuffer` holds one mono `VecDeque<f32>` per channel at 48 kHz.
- `W` = 600 ms (28 800 samples). `max_buffer_size` = 8 W; beyond it `add_samples` drops the oldest samples (`:110-151`).
- `can_mix` is true when *either* side has W (`:153-156`). `extract_window` drains W from a full side and drains a short side completely, then pads it with zeros (`:158-211`).
- `emit_ready_windows` loops on `can_mix` after every chunk and sends each window, interleaved L = mic / R = system, to the saver channel with `try_send` (`:1240-1278`).
- Nothing emits what is left in the ring buffer when the run loop ends (`:1217-1236`). `flush_remaining_audio` flushes VAD only (`:1331-1341`).

**Mic gap fill** (`mic-disconnect-recovery`, `:23-48`, `:1280-1329`):
- A discontinuity marker opens a gap, and `count_mic_padding` counts the zeros padded onto the mic side while the gap is open.
- The first chunk from the new mic calls `close_mic_gap` *before* anything else touches the ring buffer (`:1082-1087`). It adds `gap_seconds × 48 000 − padded` zeros (capped at 30 s) in window-sized steps.
- The gap length comes from chunk timestamps, which are `get_recording_duration()` at callback time (`:628-629`).

**Saver** (`audio/incremental_saver.rs`, `audio/encode.rs`):
- `add_chunk` accumulates interleaved stereo and writes a checkpoint every 30 s. Each checkpoint is `encode_single_audio`: an f32le pipe into ffmpeg producing AAC-LC (`-q:a 0.7`) in MP4, bounded by `ENCODE_TIMEOUT` = 60 s.
- `finalize` writes the remainder, merges with `-f concat -c copy` under `MERGE_TIMEOUT` = 120 s, then deletes `.checkpoints/`.
- `recover_audio_from_checkpoints` does the same merge over `*.mp4` sorted by name. `has_audio_checkpoints` looks for `*.mp4` only.
- The stop flow wraps `save_recording_only` in a fixed 300 s timeout (`recording/stop.rs:393-396`) and emits `recording-stopped` only afterwards.

**Measured during exploration** (ffmpeg 8.1.2, dev machine, 24 cores):
- **`-c copy` merge.** Each checkpoint adds 1792 samples (37.33 ms): 1024 of priming plus 768 of trailing padding, because 1 440 000 frames is not a multiple of 1024. The first checkpoint adds 21.33 ms.
- **Concat-demuxer re-encode, with or without `outpoint`.** Still +16 ms per boundary: the demuxer ignores each file's edit list.
- **FLAC checkpoints, concat demuxer, one AAC encode.** Exact: every test click at 0.00 ms. 27 min encodes in 9.5 s (~172× real time). A 30 s FLAC checkpoint is ~2 MB, 112 MB for 27 min.

## Goals / Non-Goals

**Goals:**
- A file position equals elapsed recording time for any meeting length (spec `stereo-recording`), without reading capture timestamps in the common path.
- Keep the mic-gap behaviour of `mic-disconnect-recovery` unchanged at the spec level.
- Make the final file sample-exact with a single lossy encode (spec `audio-encoding`).
- Every new rule is a pure function of buffer lengths, so it can be unit-tested without devices.

**Non-Goals:**
- Spans where *neither* channel delivers audio, other than the mic gap the existing fill handles, are still absent from the file. This is unchanged. The mic always delivers samples while its stream is alive (a muted USB mic delivers zeros), so this only happens during a mic gap.
- No change to the window size, the stereo layout, the saver channel capacity or the final codec settings.
- No progress reporting during the final encode (proposal: follow-up).

## Decisions

### D1: Cut a window only when both sides are full, or when one side is starved

The new rule is `can_mix = (mic ≥ W && sys ≥ W) || max(mic, sys) ≥ W + SLACK`, with `SLACK = W` (600 ms).

- In the first branch both sides drain exactly W, so nothing is padded and a short side's samples stay as the start of its next window. This is what removes the drift.
- The second branch fires only when one side is a full window or more ahead. It extracts as today: the full side drains W, the short side drains what it has and is padded with zeros. That side is then marked **starved**.

`SLACK = W` was chosen over the 200–300 ms discussed earlier. A false starvation, where a live side is merely late, puts back exactly the inserted silence this change removes. D2 removes the offset that a large slack would otherwise leave. So a generous slack costs only buffer memory: at most 2 W per side, well inside `max_buffer_size` = 8 W.

Starvation cuts are logged at info level with the side and the lead, throttled to once per 10 s. Field logs then show whether false starvation happens.

Alternatives considered:
- **Keep the current rule.** Rejected: it is the cause.
- **Clock-driven mixer** that cuts windows by chunk timestamps and pads or trims each side against the recording clock. Rejected for now. Timestamps are callback-processing times with jitter, they include pause time, and a backlog delivered late would be placed by its arrival time. Its errors can exceed what it fixes.
- **Smaller slack.** It lowers the offset only when D2 is absent, so with D2 there is no benefit.

### D2: A starved side that resumes is lined up with the other side's buffered tail

When a chunk of length `c` arrives for a side marked starved, the ring buffer first pushes `pad = other_len − own_len − c` zeros (saturating at 0) onto that side, then clears the flag. After the chunk is added, both tails end at "now": the other side's buffered samples are the real time not yet written to the file, and the new chunk is the last `c` samples of it. The model gave ±3 ms after repeated 3 s and 60 s loopback gaps; without this step the offset was −183 ms.

This uses only buffer lengths. It assumes the live side's buffer is current, which holds within one chunk of jitter. Alignment zeros on the mic side go through `count_mic_padding`, so an open mic gap counts them (D3).

Alternative: place the resuming side by its chunk timestamp. Rejected for the timestamp reasons in D1.

### D3: Order relative to the mic gap fill; timestamp fill only for spans with no live reference

For a mic chunk, the run loop does these steps in order:

1. D2 alignment.
2. `close_mic_gap`. Its `padded` now includes the alignment zeros.
3. VAD, telemetry and `add_samples` as today.

For a system chunk, only D2 runs before `add_samples`.

**System audio alive during the gap.** Starvation cuts pad the mic for most of the gap, and alignment pads the rest. The remainder `gap − padded` is then only timestamp jitter, roughly ±1 chunk. Adding it would delay the mic by that amount.

So `mic_gap_fill_samples` gets a dead band. A remainder below one window (W) is dropped; a larger one, from a span where no window was cut because system audio was silent too, is filled as today, still capped at 30 s. The existing test with a 400 ms remainder (`mic_gap_fill_is_the_remainder_after_partial_coverage`) changes accordingly. The spec-level behaviour in `mic-disconnect-recovery` ("gap recorded as silence, no audio shifted") is kept.

**Seed the gap count from the last mic chunk.** Found during apply, and present on HEAD before this change. The gap starts at `last_mic_chunk_end`, but `mic_pad_since_gap` only counts from the discontinuity marker. A real swap sends the marker seconds after the mic stopped delivering, because detection takes 2 polls of 2 s. While system audio flowed, the mixer already padded the mic for that time, and `close_mic_gap` then fills it a second time.

`mic_gap_counts_padding_from_the_last_mic_chunk` reproduces this on HEAD: mic stops at 2.0 s, marker at 6.0 s, new mic at 7.0 s. Post-gap mic lands at 11.0 s and system audio at 7.1 s, a 3.9 s shift.

The fix: the ring buffer keeps a count of mic-side zeros padded since the last real mic chunk, reset when one is processed. `begin_mic_gap` seeds the gap count with it. The user approved adding this to the change.

### D4: Emit the ring-buffer tail when the pipeline ends

After the run loop exits, and before `flush_remaining_audio` returns, the pipeline emits one final chunk if either side holds samples. Its length is `max(mic_len, sys_len)` frames, and the shorter side is zero-padded. The final chunk is not rounded up to W, so no extra silence is added at the end.

This reaches the saver before the stop flow sets `is_saving = false`. By then `RecordingState::stop_recording` has dropped the capture-side sender (`recording_state.rs:171-173`), and `force_flush_and_stop` → `stop()` has dropped the manager's sender and awaited the pipeline task. The saver already accepts chunks of any length.

### D5: Lossless checkpoints in FLAC

- Checkpoints become `audio_chunk_NNN.flac`, encoded by a sibling of `encode_single_audio`: `-c:a flac -sample_fmt s32`, which is 24-bit as measured. It reuses the same sanitization, the same `run_ffmpeg_with_timeout` and `ENCODE_TIMEOUT`.
- `encode_single_audio` itself stays AAC: the legacy save path in `audio_processing.rs` still uses it.
- Drop-and-continue on encode failure is unchanged.

**Clipping.** 24-bit integer storage clips samples beyond ±1.0, which float AAC input used to carry. This is accepted: there is no gain stage, and playback converts to integer anyway.

Alternatives considered:

| Option | Why not |
|---|---|
| WAV `pcm_s16le` | ~11.5 MB/min, about 3× FLAC |
| WAV f32 | ~23 MB/min |
| Keep AAC checkpoints and decode, trim and re-encode at merge | Measured exact, but encodes the audio twice (generation loss) and needs exact per-file frame counts in recovery |
| `-c copy` with `inpoint` / `outpoint` | Measured not exact |
| One persistent encoder for the whole session | The user chose the simpler path. It changes the durability model and the failure handling |

### D6: Merge = concat demuxer over FLAC → one AAC encode with the existing settings

`merge_checkpoints` writes the same list file and runs `-f concat -safe 0 -i list -c:a aac -q:a 0.7 -profile:a aac_low -movflags +faststart -f mp4 audio.mp4`. The codec arguments come from the constants `encode.rs` already uses, so the bitrate and container requirements of `audio-encoding` keep holding with no second copy of the settings. Success, failure and cleanup handling are unchanged. On failure, `.checkpoints/` is kept, as today.

### D7: Timeouts scale with the audio held

- `merge_timeout(audio_secs) = 120 s + audio_secs / 8`. This tolerates encoders down to about 8× real time: a 3 h recording gets 1470 s, and measured here it would take ~63 s. `audio_secs` comes from the saver's own sample counts: frames written to checkpoints plus the buffered remainder.
- The saver exposes its finalize budget. The stop flow's outer timeout around `save_recording_only` becomes that budget plus 60 s, replacing the fixed 300 s.
- A genuinely hung ffmpeg is still killed: the bound is finite.

Alternative: keep 300 s and accept the loss on slow machines. Rejected: the checkpoints would survive, but the meeting would be saved without `audio.mp4`.

### D8: Recovery handles both formats, ordered by index

- `recover_audio_from_checkpoints` collects `audio_chunk_<n>.<ext>` and sorts by the numeric `n`. Sorting by file name puts index 1000 before 101 past ~8.3 h.
- If any `.flac` exists, it runs the D6 encode under `merge_timeout(count × 30 s)`. Otherwise it runs the existing `.mp4` copy merge for folders left by older versions.
- `has_audio_checkpoints` accepts either extension.
- The IPC result shape (`AudioRecoveryStatus`) does not change.

### D9: How it is verified

**Unit tests, pure, ring buffer:**
- Two sides fed with uneven chunk sizes and jitter: emitted frames equal fed frames minus what is still buffered, and no zero is inserted.
- Starvation: one side silent pads only that side.
- Resume: the alignment places the resumed chunk within one chunk.
- Mic gap with system audio alive: fill is 0. Mic gap with system audio silent: fill is the gap.
- The D4 tail.
- The existing 600 ms-window and gap-padding tests are updated to the new rule.

**Saver integration test** (ffmpeg on PATH, like `test_checkpoint_creation`): clicks at known sample positions across 3 checkpoints. Positions in the merged `audio.mp4` must stay within one AAC frame. The same check runs on recovery output, plus a legacy `.mp4` folder that still recovers.

**Manual:**
- Record ≥ 20 min of a call with both channels. `ffprobe` duration must be within 0.5 s of the session length, and ▷ on a block after 20 min must play that block.
- Stop a ≥ 60 min recording and note the finalize time.

## Risks / Trade-offs

- **[Stop takes longer]** About 0.35 s per minute of recording here, 2–3× that on slower hardware. → The timeouts in D7 keep it safe. A visible progress indicator is a follow-up.
- **[Temporary disk use]** ~4 MB/min in `.checkpoints/`, about 250 MB/h, removed after a successful finalize. → Accepted. Recordings already live in the user's Music folder, and only an interrupted recording leaves the files until recovery.
- **[False starvation]** A live side merely late by > 600 ms still gets silence. → The throttled log in D1 makes it visible. If logs show it, the slack can be raised (memory allows 8 W) without changing the design.
- **[Live side not current at resume]** D2 aligns to a buffer that is itself up to one chunk late. → Bounded by chunk jitter, well inside the 50 ms the spec allows.
- **[Both sides silent]** Unchanged. That span is still missing from the file outside a mic gap. → The non-goal is documented.
- **[24-bit clipping]** Samples above ±1.0 clip in checkpoints. → There is no gain stage. Accepted (D5).

## Migration Plan

- No data migration. Recordings made before the change keep their drift (proposal: out of scope).
- A `.checkpoints/` folder left by an older version still recovers through the legacy `.mp4` path (D8).
- Rollback is a plain revert. A FLAC checkpoint folder created by the new version and recovered by an old version would not be found: the old `has_audio_checkpoints` looks for `.mp4` only. This matters only if a crash and a downgrade happen together.
