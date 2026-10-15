## 1. Spikes

- [x] 1.1 Verify bundled ffmpeg exposes libopus and Ogg-Opus plays in WebView2 `<audio>`, and record the fallback decision in design.md
- [x] 1.2 Confirm second-pass PCM slicing points in offline `diarization.rs` and online finalize path, and verify exemplar windows are reachable without the meeting file

## 2. Database migration and models

- [x] 2.1 Add migration rebuilding `speaker_embeddings` with `audio_blob`, `audio_codec`, `audio_sample_rate`, `is_verified`, `verified_at`, and verify migrate up/down preserves existing rows as legacy (NULL blob, unverified)
- [x] 2.2 Extend `SpeakerEmbedding` / `VoiceprintRow` models with clip + verified fields and verify `cargo check` passes on the database crate
- [x] 2.3 Update `storage_stats` to report audio bytes + clip count separately and verify counts match `LENGTH(audio_blob)` sums on a fixture DB

## 3. Clip encode and persist

- [x] 3.1 Implement `audio/clip_encode.rs` (channel-demux slice, 16 kHz mono, Opus ~24k voip via ffmpeg with timeout) and verify a 5 s fixture encodes to a playable Ogg under the ~15 s cap
- [x] 3.2 Wire clip capture into `write_cluster_cache` / `enroll_embeddings_from_buffer` from the same channel as the embedding and verify new rows carry clips with matching channel
- [x] 3.3 Keep `enroll_cluster` reparenting clips with rows and resetting verification on reconfirm, and verify cap pruning deletes whole rows with no orphan blobs

## 4. Commands

- [x] 4.1 Add `get_voiceprint_audio(id)` temp-file cache command and verify blob playback works with the meeting audio file deleted
- [x] 4.2 Add `verify_voiceprint` / `verify_speaker` commands and verify they set only `is_verified`/`verified_at` without touching embeddings or bindings
- [x] 4.3 Extend `list_voiceprints` with `has_audio`, `is_verified`, and per-group unverified counts and verify legacy rows return audio-unavailable/unverified states

## 5. Frontend review UI

- [x] 5.1 Switch `VoiceprintBrowser` clip playback to blob path with legacy file-seek fallback and verify clips play standalone while legacy rows keep range playback
- [x] 5.2 Add per-row Verify buttons, per-group Verify-all, unverified badges, and hide-verified filter, and verify counts refresh without reload
- [x] 5.3 Show audio vs embedding bytes separately in the Storage section and verify formatting matches the existing human-readable size

## 6. Verification

- [x] 6.1 Add repository tests for clip persist, verify flag transitions, and stats separation, and verify `cargo test` passes for the speaker module
- [ ] 6.2 End-to-end on a stereo meeting (diarize, enroll, delete audio file, play clips, verify/hide-verified) and verify review covers only new rows
