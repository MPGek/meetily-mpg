## 1. Backend: embedder and segmentation become enhanced-only

- [x] 1.1 Remove `LegacyResnetEmbedder`, `LEGACY_MODEL_TAG`, `LEGACY_CLUSTER_THRESHOLD`, and `LEGACY_RECOGNITION_THRESHOLD` from `audio/embedder.rs`
- [x] 1.2 Rework `create_speaker_embedder` to build only `TitanetEmbedder` from `enhanced_model_paths(models_dir)` and return a clear error when the enhanced files are missing or corrupt (no fallback)
- [x] 1.3 Ensure the `SpeakerEmbedder` trait's `model_tag()`/`family_threshold()` always report `titanet_large` / `TITANET_CLUSTER_THRESHOLD`; remove legacy-branch code
- [x] 1.4 In `audio/segmentation.rs`, remove `LegacySegmenter` and the dummy-segmenter fallback inside `Segmentation30Segmenter`; make `create_segmenter` return the enhanced segmenter only and error when the enhanced file is missing

## 2. Backend: model paths, status, and commands

- [x] 2.1 Delete `diarization_model_paths` (manifest/registry resolution) and replace every call site with `enhanced_model_paths` (diarization.rs, online_diarization.rs, recording_commands.rs, segmentation.rs)
- [x] 2.2 In `create_polyvoice_diarizer`, drop the `use_enhanced` flag, the legacy file validation block, and the enhanced-selection log
- [x] 2.3 Simplify `DiarizationModelStatus` to `{ segmentation_ready, embedding_ready, ready }` and update `check_diarization_models` to verify only the enhanced files across app-data/resource/dev-manifest locations
- [x] 2.4 Remove `download_diarization_models`, `DiarizationDownloadProgress`, and the `diarization-model-download-*` progress events
- [x] 2.5 Remove `download_enhanced_diarization_models` and `remove_enhanced_diarization_models` commands
- [x] 2.6 Extend `cleanup_legacy_models` to also delete stale standard files `powerset_int8.onnx` and `resnet34_int8.onnx`
- [x] 2.7 Update Tauri command registrations in `lib.rs` and re-exports in `audio/mod.rs` for the removed commands and types

## 3. Backend: recognition, online path, and recording tags

- [x] 3.1 In `diarization.rs`, make the prototype/centroid load path single-family (enhanced `titanet_large`, 192-d) instead of loading both legacy and enhanced pools
- [x] 3.2 In `speaker_recognition.rs`, remove the legacy threshold branch; keep only the enhanced recognition τ
- [x] 3.3 In `online_diarization.rs`, set the embedder and `model_tag` unconditionally to the enhanced family and simplify `family_threshold` logic
- [x] 3.4 In `recording_commands.rs`, make `store_model_tag` always `titanet_large`
- [x] 3.5 Update the diarization spike tests' `find_models_dir` to look for `segmentation-3.0.onnx` + `titanet_large.onnx`, preserving the graceful skip when models are absent

## 4. Backend verification

- [x] 4.1 Rewrite `embedder.rs` unit tests to assert enhanced-only selection and missing-file errors; drop legacy fallback assertions
- [x] 4.2 Run `cargo check`/`cargo test` for the Tauri crate and fix any fallout from the removed symbols

## 5. Frontend

- [x] 5.1 In `services/recordingService.ts`, narrow `checkDiarizationModels` to the `{ segmentation_ready, embedding_ready, ready }` response and remove `downloadDiarizationModels`, `downloadEnhancedDiarizationModels`, `removeEnhancedDiarizationModels`, and the download progress/complete/error listeners
- [x] 5.2 In `components/DiarizationSettings.tsx`, remove the download button, progress bar, and download listeners; render a read-only "Enhanced Models (segmentation-3.0 + TitaNet-Large)" status card (per-file ✓/○ + Ready badge) with build-time-bundling copy
- [x] 5.3 Gate diarization entry points on enhanced readiness: `TranscriptContext.tsx`, `hooks/useRecordingStop.ts` (reword the error toast to "Enhanced diarization models not bundled"), and `components/MeetingDetails/TranscriptButtonGroup.tsx` (hide button / point to settings when not ready)
- [x] 5.4 Grep the frontend for removed invoke command names and delete stale references

## 6. Overall verification

- [x] 6.1 Run frontend typecheck/lint/build to confirm no dangling references to removed commands/types
- [x] 6.2 Grep the repo for `powerset_int8`, `resnet34_int8`, `LEGACY_MODEL_TAG`, and `download_diarization_models` and confirm only intentional references (e.g., docs) remain
- [x] 6.3 Manually verify: Settings shows read-only enhanced status; diarization runs when the enhanced set is bundled and fails with a clear message when it is not