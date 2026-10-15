## 1. Offline clustering fix

- [x] 1.1 In `frontend/src-tauri/src/audio/diarization.rs` `create_polyvoice_diarizer`, replace the `AhcClusterer::new(max)` / `AhcClusterer::default()` auto-threshold construction with `AhcClusterer::with_threshold(max_clusters, polyvoice::types::DEFAULT_AHC_THRESHOLD)` where `max_clusters` derives from `max_speakers` (`None`/`<= 0` → `0` = no ceiling)
- [x] 1.2 Verify the change compiles and the import path for `DEFAULT_AHC_THRESHOLD` (or `Profile::Balanced.default_threshold()`) resolves

## 2. Online Efficient-mode clustering fix

- [x] 2.1 In `frontend/src-tauri/src/audio/online_diarization.rs` `EmbeddingBuffer::cluster`, replace the `AhcClusterer::new(max_speakers)` / `AhcClusterer::default()` construction with `AhcClusterer::with_threshold(max_speakers, DEFAULT_AHC_THRESHOLD)`
- [x] 2.2 Confirm `recording_commands.rs` passes the user's `maxSpeakers` value to `OnlineDiarizationProcessor::new` instead of the hardcoded `0` (0 stays "no ceiling")

## 3. Build and unit verification

- [x] 3.1 Run `cargo build` in `frontend/src-tauri` and fix any API drift
- [x] 3.2 Run `cargo test` (or the project's Rust test command) on the audio module to confirm no clustering tests regress

## 4. Manual verification

- [x] 4.1 Re-run "Re-analyze Speakers" on the previously failing recording (`audio_Meeting 2026-08-12_19-36.mp4`) and confirm the system channel now yields distinct `SPEAKER_NN` labels instead of only `SPEAKER_00`
- [ ] 4.2 Record a new multi-person meeting in Efficient mode and confirm distinct speaker labels appear at stop
- [ ] 4.3 Confirm a single-speaker recording still yields a single speaker label (no over-splitting)

## 5. Singleton cluster pruning

- [x] 5.1 In `diarization.rs` `create_polyvoice_diarizer`, wrap the fixed-threshold clusterer in `MinClusterSizeClusterer::new(_, 2)` (store as `Box<dyn Clusterer>` on `PolyvoiceDiarizer`)
- [x] 5.2 In `online_diarization.rs` `EmbeddingBuffer::cluster`, wrap the fixed-threshold clusterer in `MinClusterSizeClusterer::new(_, 2)`

## 6. Gap-fill for short unmatched utterances

- [x] 6.1 In `diarization.rs` `find_best_speaker`, add nearest-neighbor fallback: single-speaker channel → assign that speaker; multi-speaker channel → nearest turn within 30s
- [x] 6.2 In `online_diarization.rs` `find_best_speaker`, apply the same gap-fill fallback

## 7. Build and verify follow-up

- [x] 7.1 Run `cargo build` and `cargo test --lib audio::` to confirm the pruning + gap-fill changes compile and don't regress
- [ ] 7.2 Re-run "Re-analyze Speakers" on `audio_Meeting 2026-08-12_19-36.mp4` and confirm the 5 short mic utterances are now labeled `MIC_SPEAKER_00` and singleton system fragments are gone
