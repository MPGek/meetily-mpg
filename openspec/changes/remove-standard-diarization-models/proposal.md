# Remove standard diarization models (enhanced-only)

## Why

Diarization supports two model families: the standard polyvoice set (`powerset_int8` + `resnet34_int8`, runtime-downloaded, 256-d embeddings) and the enhanced set (`segmentation-3.0` + `titaNet-Large`, bundled at build time, 192-d embeddings). The enhanced set is measurably more accurate, but every path still treats standard as the baseline and falls back to it whenever enhanced files are missing — keeping dead code, a runtime download flow, and a weaker fallback in the product. The standard path is no longer needed and should be removed so the app ships with exactly one, higher-quality model set.

## What Changes

- **BREAKING**: Diarization runs exclusively on the enhanced model set (`segmentation-3.0` onnx-community + `titaNet-Large` Recogment, bundled at build time). When the bundled files are missing, diarization SHALL error with a clear message instead of falling back to standard models.
- Remove the standard model runtime download flow: `download_diarization_models`, download progress/complete/error events, and the settings "Download Models" control.
- Remove standard model resolution (`diarization_model_paths` via polyvoice `ModelRegistry`/manifest), `LegacyResnetEmbedder`, and all `resnet34_int8` / legacy constants and fallback branches in `create_polyvoice_diarizer`, `create_speaker_embedder`, the online processor, segmentation, and recording paths.
- **BREAKING**: Existing standard-tagged (`resnet34_int8`, 256-d) voiceprints, centroids, and caches SHALL no longer be loaded for recognition; matching uses only `titaNet_large` 192-d embeddings. No data migration — standard-tagged rows remain stored but are ignored.
- Stale standard model files on disk (`powerset_int8.onnx`, `resnet34_int8.onnx`) SHALL be cleaned up when model status is checked.
- Clustering/recognition thresholds become the enhanced family values (192-d) only; the fixed `0.45` Balanced-profile wording is removed.
- Settings panel becomes a read-only enhanced-model status view (no download / re-download / remove controls).

## Capabilities

### New Capabilities

None — this change removes behaviors, it does not introduce a new capability.

### Modified Capabilities

- `speaker-diarization`: model management requirement changes from runtime download/verification of the standard polyvoice set to a read-only check that the bundled enhanced set is present; the pipeline and its clustering threshold are defined for the enhanced-only family.
- `online-speaker-diarization`: Efficient- and Fast-mode embedding extraction reference the enhanced TitaNet embedder instead of `ResNet34Adapter`/resnet34, and thresholds reference the enhanced family.
- `speaker-identity-registry`: voiceprint storage wording changes to the enhanced family dimension (192-d) and recognition matches only within the `titaNet_large` model tag; legacy-tagged rows are ignored.
- `split-transcript-ui`: the "Re-analyze Speakers" button availability depends on the bundled enhanced set being ready rather than on models having been downloaded.

## Impact

- **Backend**: `frontend/src-tauri/src/audio/embedder.rs` (drop `LegacyResnetEmbedder` + legacy constants, make `TitanetEmbedder` the single embedder), `audio/diarization.rs` (model path resolution, `create_polyvoice_diarizer` no-fallback, `DiarizationModelStatus`, `check_diarization_models`, remove `download_diarization_models` + `DiarizationDownloadProgress`, cleanup of stale standard files, family-aware prototype matching becomes enhanced-only), `audio/online_diarization.rs` (model selection/fallback removal), `audio/segmentation.rs`, `audio/speaker_recognition.rs` (drop legacy matching), `audio/recording_commands.rs` (model-tag selection).
- **Frontend**: `components/DiarizationSettings.tsx` (read-only status, remove download UI and progress listeners), `services/recordingService.ts` (remove download commands/listeners), `contexts/TranscriptContext.tsx`, `hooks/useRecordingStop.ts`, `components/MeetingDetails/TranscriptButtonGroup.tsx` (model-ready gating based on enhanced status).
- **Tauri commands**: remove `download_diarization_models`; `check_diarization_models` response schema simplifies to enhanced-only fields (frontend model status call rewritten).
- **DB**: no schema change; `speaker_embeddings.model` column keeps its values, but only `titaNet_large` rows are consumed for recognition/enrollment validation.
- **Dependencies**: polyvoice crate remains the segmentation/embedding/clustering backbone; its manifest-based standard model download flow is no longer used.
- **Assumptions (recorded)**: the enhanced set is bundled at build time (no runtime download); existing standard-tagged voiceprints/centroids are allowed to remain in the DB but are ignored by matching (per user decision); standard model files on disk are deleted.