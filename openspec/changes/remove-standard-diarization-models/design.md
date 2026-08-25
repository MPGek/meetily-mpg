# Design: Enhanced-only diarization (remove standard models)

## Context

Today diarization supports two model families (see proposal.md — Why):
- **Standard**: `powerset_int8` + `resnet34_int8`, resolved through the polyvoice `ModelRegistry`/manifest (`diarization_model_paths`), runtime-downloadable, 256-d embeddings, tag `resnet34_int8`.
- **Enhanced**: `segmentation-3.0` + `titanet_large.onnx`, bundled at build time, verified by file size, 192-d embeddings, tag `titanet_large`.

Every consumer (`create_polyvoice_diarizer`, `create_speaker_embedder`, `create_segmenter`, the online processor, `recording_commands.rs`, recognition) selects enhanced-if-present and falls back to standard otherwise. The settings UI exposes a download flow for the standard set plus a read-only status for the enhanced set. The user decision (recorded in the proposal) is to drop standard models and legacy read/matching support entirely: existing `resnet34_int8` 256-d voiceprints/centroids are ignored, not migrated.

## Goals / Non-Goals

**Goals:**
- Diarization (offline, online-Fast, online-Efficient) runs only on the bundled enhanced set; any path that can no longer find it fails clearly instead of downgrading.
- Remove all standard-model code: resolution, download commands/progress events, embedder/segmenter impls, constants, and fallback branches.
- Existing standard-tagged DB rows are ignored by recognition/enrollment matching; no schema change.
- Frontend surfaces a read-only enhanced-model status with no download/remove controls.

**Non-Goals:**
- No migration or deletion of legacy `speaker_embeddings` / `meeting_speakers` / `speaker_embeddings` cache rows (rows are simply not consumed).
- No change to the polyvoice crate or its manifest; its standard-profile download flow is simply unused.
- No change to Fast-mode streaming pipeline behavior beyond its embedder/segmenter source.
- No changes to Whisper/Parakeet transcription or VAD.

## Decisions

### D1. Single embedder and segmenter path (no fallback)
- Delete `LegacyResnetEmbedder` and `LEGACY_*` constants from `audio/embedder.rs`; `TitanetEmbedder` becomes the `SpeakerEmbedder` returned by `create_speaker_embedder`. The function now builds the Titanet embedder from `enhanced_model_paths(models_dir)` and errors when a file is missing/corrupt instead of falling back. The `SpeakerEmbedder` trait keeps `model_tag()`/`family_threshold()`; they always return `titanet_large` / `TITANET_CLUSTER_THRESHOLD`.
- In `audio/segmentation.rs`, remove `LegacySegmenter` and the `DummySegmenter` chain inside `Segmentation30Segmenter`; `create_segmenter` returns `Segmentation30Segmenter` directly from the enhanced path and errors when absent. (If the enhanced ONNX is not polyvoice-shaped the app reports the model set as unavailable rather than producing a dummy segmenter.)
- Rationale: deleting the alternative impls removes the possibility of silently running the weaker family, per the user decision. Alternative considered: retaining the legacy impls but never constructing them — rejected as dead code with ongoing maintenance cost.
- Consequences: `create_polyvoice_diarizer` no longer validates `diarization_model_paths`; the `use_enhanced` flag and its log branch disappear.

### D2. Enhance-only model path resolution
- Delete `diarization_model_paths` (manifest/registry resolution) and its mod.rs/lib.rs wiring; replace all call sites with `enhanced_model_paths(models_dir)`.
- `DiarizationModelStatus` simplifies to enhanced-only readiness: `{ segmentation_ready, embedding_ready, ready }` where `ready = segmentation_ready && embedding_ready`. `check_diarization_models` computes these via `verify_enhanced_integrity` across app-data, resource, and dev-manifest locations (same three-location fallback as today) and drops the `enhanced_*` fields and the legacy checks.
- Rationale: mirrors the existing enhanced verification logic; keeps the command's multi-location repo/dev behavior. Alternative: a single `ready` boolean — rejected because the settings UI still wants per-file status.

### D3. Remove download / enhanced-download / remove commands
- Delete `download_diarization_models` (+ `DiarizationDownloadProgress`), `download_enhanced_diarization_models`, and `remove_enhanced_diarization_models`. Remove their registrations in `lib.rs` and re-exports in `audio/mod.rs`, and the branded `diarization-model-download-*` events.
- Rationale: with no runtime model acquisition there is nothing to download, progress-report, or remove.
- `cleanup_legacy_models` is retained and extended: besides the sherpa-era files it also deletes `powerset_int8.onnx` and `resnet34_int8.onnx` when present, so standard files disappear from existing installs on the next model-status check.

### D4. Family-aware matching becomes enhanced-only
- In `audio/diarization.rs`, the prototype/centroid load path becomes single-family: load only `titanet_large` (192-d) pools.
- In `audio/speaker_recognition.rs`, the per-tag threshold branch retains only the enhanced case (τ=0.68 recognition; clustering τ from `TITANET_CLUSTER_THRESHOLD`); the legacy constants and alternate branch are removed.
- This implements the "drop legacy read/matching support" decision: `resnet34_int8` rows are never candidates, and a speaker whose prototypes are only legacy remains unmatched until re-enrolled with enhanced embeddings.

### D5. Online and recording paths always tag enhanced
- In `audio/online_diarization.rs`, `create_online_processor` embeds with `create_speaker_embedder` (enhanced only) and sets `model_tag = titanet_large` unconditionally; `family_threshold` logic simplifies to the enhanced constant.
- In `audio/recording_commands.rs`, `store_model_tag` is always `titanet_large`.
- Rationale: removes the enhanced-installed checks whose outcome is now constant.

### D6. Frontend model status surface
- `services/recordingService.ts`: `checkDiarizationModels()` return type becomes `{ segmentation_ready, embedding_ready, ready }`; remove `downloadDiarizationModels`, `downloadEnhancedDiarizationModels`, `removeEnhancedDiarizationModels` and the progress/complete/error listeners.
- `components/DiarizationSettings.tsx`: drop download button, progress bar, and listeners; render a read-only "Enhanced Models (segmentation-3.0 + TitaNet-Large)" status card (per-file ✓/○ + ready badge) and reference build-time bundling in the caption.
- Callers gate on enhanced readiness: `TranscriptContext.tsx`, `hooks/useRecordingStop.ts` (message reworded to "Enhanced diarization models not bundled"), and `components/MeetingDetails/TranscriptButtonGroup.tsx` (hidden tooltip/link when not ready).

### D7. Tests target the enhanced set
- `embedder.rs` unit tests that assert fallback behavior are rewritten to assert Titanet-only selection and missing-file errors; constants tests drop legacy assertions.
- `diarization.rs` `spike_tests::find_models_dir` looks for `segmentation-3.0.onnx` + `titanet_large.onnx` instead of `powerset_int8.onnx` + `resnet34_int8.onnx`.

## Risks / Trade-offs

- **Existing installs without bundled enhanced models** will get a clear "enhanced models required (bundled at build time)" error instead of silently running the weaker standard set → by design; the settings card explains the models are bundled at build time and a rebuild with network is required.
- **Legacy voiceprints stop matching** → users may need to re-assign speaker names; matching simply treats legacy-only speakers as unknown. Mitigation: no data is deleted; once the user re-assigns a speaker, new enhanced prototypes are enrolled on the next naming/assignment, restoring recognition.
- **A release built without network misses the enhanced bundle** → diarization is unavailable for that build. Mitigation: build-time fetch stays a required step of the release pipeline (`build.rs` / `scripts/fetch-enhanced-models.*`); the availability check makes the state visible in Settings rather than failing silently.
- **Cleanup writes to the model directory** → `check_diarization_models` already deletes stale files today; deleting `powerset_int8.onnx`/`resnet34_int8.onnx` simply extends that existing behavior. Deleting files a user placed manually is accepted (they are unversioned standard artifacts).
- **DB retains legacy rows indefinitely** → storage stats continue to count them. Acceptable (non-goal); keeps the change non-destructive.

## Migration Plan

1. Land the backend removal (embedder/segmenter/paths/commands/recognition) and the frontend status rewrite together; the `check_diarization_models` response schema changes, so frontend and backend must ship in the same release.
2. No DB migration. On first run after the update, `check_diarization_models` purges stale standard and sherpa-era files from the model directory.
3. Rollback: revert the combined change; the standard download flow and fallbacks are restored by the previous commit. No data migration is needed in either direction.
4. Release builds must bundle the enhanced set; CI should run the enhanced-integrity check.

## Open Questions

None that affect the specs, approach, or task breakdown. Threshold values for the enhanced family stay as currently implemented (`TITANET_CLUSTER_THRESHOLD`, `TITANET_RECOGNITION_THRESHOLD`) and are not part of this change.