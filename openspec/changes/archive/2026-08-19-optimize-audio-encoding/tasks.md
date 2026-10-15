## 1. Encoder parameter changes

- [x] 1.1 In `frontend/src-tauri/src/audio/encode.rs`, add named constants for the AAC codec parameters (profile `aac_low`, VBR quality `0.7`) and replace `-b:a 192k` with the VBR form `-q:a <quality>` so both save paths inherit the new settings (design D1/D2).
- [x] 1.2 Remove or update the now-stale inline comment referencing "192k / Increased from 64k" in `encode.rs` so it reflects the VBR target instead.

## 2. Stereo channel unification

- [x] 2.1 Verify the legacy saver's `mixed_chunks` in `recording_saver_old.rs` are stereo interleaved f32 (left=mic, right=system) before touching the channel argument (design D3 precondition).
- [x] 2.2 Change the `encode_single_audio` call in `write_audio_to_file_with_meeting_name` (`audio/audio_processing.rs`) from `channels=1` to `channels=2` so the legacy path matches the incremental saver (spec: Consistent stereo channel encoding).
- [x] 2.3 Grep all `encode_single_audio` call sites (`audio_processing.rs`, `incremental_saver.rs`) and confirm none pass mono data that would break with the channel change.

## 3. Verification

- [x] 3.1 Run `cargo check` (or `cargo build`) in `frontend/src-tauri` to confirm the changes compile.
- [x] 3.2 Record a short test meeting (or run an encode-path test) and inspect the output with `ffprobe`: codec `aac`, container MP4/M4A, `channels=2`, `sample_rate=48000`, and average bitrate within the 64-128 kbps band (spec: Voice-appropriate encoding bitrate; design D1 risk mitigation).
- [x] 3.3 Verify the incremental checkpoint → concat merge path still produces a playable `audio.mp4` with stereo channels (spec: Incremental checkpoint path preserves stereo).
- [x] 3.4 Run the existing Rust test suite for the audio module (`cargo test` in `frontend/src-tauri`) to confirm no regressions.
- [x] 3.5 Record a one-hour-equivalent or long sample and confirm file size stays at or below ~50 MB/hour (spec: Storage footprint of a one-hour meeting); adjust the quality constant if the measured bitrate lands outside the target band.
