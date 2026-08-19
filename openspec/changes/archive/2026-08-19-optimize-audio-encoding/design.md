## Context

See proposal.md (Why/What). Current state that shapes this design:

- One shared encoder, `encode_single_audio` in `frontend/src-tauri/src/audio/encode.rs`, pipes interleaved f32 PCM to ffmpeg: `-c:a aac -b:a 192k -profile:a aac_low -movflags +faststart -f mp4`.
- Two callers: `IncrementalAudioSaver` (stereo, channels=2, every-30s checkpoints, merged via `-c copy`) and the legacy `write_audio_to_file_with_meeting_name` in `audio_processing.rs` (channels=1).
- Capture and mixing are stereo interleaved (left = mic, right = system) per the `stereo-recording` capability; the incremental saver's `channels=2` matches this. The legacy path passing `channels=1` is inconsistent with that layout.
- The saved file is never used for transcription (decoders always resample to 16 kHz mono), so encoding quality affects only human listening.
- ffmpeg's native AAC encoder supports VBR via `-q:a` (0.1-1.0 fractional scale). Approximate total bitrate at 44.1 kHz stereo: `0.3` ≈ 48k, `0.5` ≈ 64k, `0.7` ≈ 96k, `1.0` ≈ 128k. The native encoder is AAC-LC only (no HE-AAC), which is what the code already targets with `aac_low`.

## Goals / Non-Goals

**Goals:**
- Cut per-hour storage of saved meetings to ~43 MB or less (from ~86 MB) with no audible quality loss for voice.
- Preserve the exact output surface: AAC-LC in MP4/M4A, 48 kHz, `+faststart`, stream-copy checkpoint merging.
- Make every save path emit stereo (left=mic, right=system) so no recording silently drops a channel.

**Non-Goals:**
- Switching codec/container (Opus, HE-AAC) - rejected in the proposal.
- Re-encoding or migrating already-saved recordings.
- Changing capture sample rate (48 kHz), the mixing pipeline, or any transcription/diarization path.

## Decisions

**D1: Switch from CBR `-b:a 192k` to VBR `-q:a 0.7`.**
Rationale: VBR spends bits only where there is signal. Meetings are pause-heavy, so effective file size drops well below the ~96 kbps nominal target while keeping a comfortable margin above speech transparency (48-64 kbps). `0.7` targets ~96 kbps total for stereo, i.e. ~2x the speech-transparency floor - safe even if system-audio playback is occasionally music-like.
Alternatives considered: (a) CBR 96k - predictable size but wastes space on silence; (b) `-q:a 0.5` (~64k) - more aggressive, but leaves no headroom for non-speech audio in meetings (screen-share videos, hold music). `0.7` is the "don't think about it" point.

**D2: Centralize encoding parameters as named constants in `encode.rs`.**
Rationale: two callers (checkpoint + legacy write) already share `encode_single_audio`; a single `const AAC_QUALITY: &str = "0.7"` (and the existing hardcoded codec/profile) makes future tuning a one-line change and guarantees both paths stay in sync.
Alternative: a config struct threaded through callers - overkill for two constants.

**D3: Fix the legacy path to encode stereo (`channels=2`), matching the incremental saver.**
Rationale: the pipeline produces stereo interleaved data (left=mic, right=system); encoding it as 1 channel misreads the interleaved layout. The incremental saver already uses 2, and `write_transcript_json_to_file` already records `sample_rate: 48000` with dual-channel capture assumed. Unify at the `encode_single_audio` call site in `write_audio_to_file_with_meeting_name`.
Precondition: verify at implementation time that the legacy saver's `mixed_chunks` are stereo interleaved (same pipeline as the incremental path). If the legacy buffer is somehow genuinely mono-mixed, keep `channels=1` there and revisit - but the `stereo-recording` capability states the pipeline is stereo.

**D4: Keep `aac_low` profile, MP4 container, `+faststart`, and 48 kHz passthrough unchanged.**
Rationale: no consumer depends on bitrate; changing the container/codec surface (extensions, `audioFormats.ts`, decoder paths) is risk without benefit. `+faststart` is inert for local files but harmless; leave it to avoid unrelated churn.

## Risks / Trade-offs

- [Native AAC VBR bitrate is approximate and content-dependent] → Mitigation: verification task runs `ffprobe` on a saved recording (codec, channels, average bitrate) and confirms the 64-128 kbps band; adjust `AAC_QUALITY` if the real-world average lands outside it.
- [VBR can raise bitrate on dense, music-like content] → Mitigation: `0.7` targets ~96k total; even a worst-case dense segment stays well under the current 192k, so this change can only reduce or match today's size.
- [Legacy-path channel fix could be wrong if its data layout is not stereo interleaved] → Mitigation: implementation-time verification (see D3) before merging; the dual-channel architecture and the incremental saver's `channels=2` strongly indicate stereo.
- [AAC priming at each 30s checkpoint boundary (pre-existing) could surface as tiny clicks after concat] → Mitigation: pre-existing and out of scope; note it, do not fix here.

## Migration Plan

No data migration. New recordings use the new encoder settings; already-saved files are untouched and remain playable by every existing decode path (format unchanged). Rollback is reverting the constants in `encode.rs` and the channel argument in `audio_processing.rs` - no schema, DB, or file-format changes.
