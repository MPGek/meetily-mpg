## Why

Voiceprints today store only a 192-d embedding plus provenance timecodes, and the Voiceprint Browser validates them by seeking into the meeting's `audio.mp4`. That file is frequently missing, deleted with the meeting, or plays a shifted segment (checkpoint-concat / timeline-mapping drift), so users cannot trust what they hear and cannot verify whether an enrolled voice actually belongs to the right person.

## What Changes

- Store a short self-contained voice audio clip per `speaker_embeddings` row as an Opus mono blob (16 kHz, ~24 kbps voip, max ~15 s) alongside the embedding, captured from the same channel the embedding came from.
- Play voiceprint clips from the blob via a new `get_voiceprint_audio` path (temp-file cache, blob URL), with no dependency on the meeting's `audio.mp4`; keep the old file-seek path only as a legacy fallback for rows without a blob.
- Add a per-voiceprint verified flag (`is_verified` + `verified_at`) with `verify_voiceprint` / `verify_speaker` commands, per-row Verify buttons, Verify-all per group, a "hide verified" filter, and "N new" badges so periodic review covers only new voiceprints.
- Extend `speaker_storage_stats` / Storage section to account for audio bytes separately from embedding bytes; legacy rows without blobs or flags keep working with "audio unavailable" / "unverified" states.

## Capabilities

### New Capabilities
- `voiceprint-audio-clips`: self-contained Opus mono clip per voiceprint row (capture, encode, schema, blob playback path).
- `voiceprint-verification`: per-voiceprint verified flag, verify commands, browser verify controls and hide-verified filter.

### Modified Capabilities
- `voiceprint-review`: clip playback source changes from meeting-file seek to stored blob; browser shows verification state, new-count badges, and audio-unavailable states.
- `speaker-identity-registry`: storage statistics include audio bytes; prototype cap / enrollment semantics unchanged but enrolled rows now carry audio blobs.

## Impact

- DB: new migration on `speaker_embeddings` (`audio_blob`, `audio_codec`, `audio_sample_rate`, `is_verified`, `verified_at`); table rebuild pattern as in `20260819000000`; no change to the two-owner CHECK.
- Rust: diarization persist path (`diarization.rs`, `online_diarization.rs`, `SpeakerRepository::write_cluster_cache` / `enroll_*`), new `audio/clip_encode.rs` (PCM slice + resample + libopus via bundled ffmpeg), new `get_voiceprint_audio` command + temp cache, updated `storage_stats` / `list_voiceprints`.
- Frontend: `VoiceprintBrowser.tsx` (blob playback, Verify buttons, hide-verified filter, badges), `useAudioPlayer` direct-play mode (no `playRange` for clips).
- Dependencies: bundled ffmpeg must provide `libopus`; Chromium/WebView2 must play Opus-in-Ogg (both verified by spike, with documented fallback).
