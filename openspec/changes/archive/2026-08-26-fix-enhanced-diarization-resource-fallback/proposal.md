# Fix enhanced diarization resource fallback

## Why

The enhanced diarization models (`segmentation-3.0.onnx` + `titanet_large.onnx`) are fetched at build time by `build.rs` and bundled as Tauri resources (`tauri.conf.json` `bundle.resources` `models/*.onnx`). However runtime consumers (`create_segmenter`, `create_speaker_embedder`, `OnlineDiarizationProcessor::new`, `run_diarization_blocking`) resolve models only in `app_data_dir/models` (`%APPDATA%\com.meetily.ai\models\`), while `check_diarization_models` correctly checks `app_data || resource_dir || manifest`. The result is a split-brain state: Settings shows "Ready ✓" (because the bundle is found in `resource_dir`) but offline/online diarization fails with `Enhanced segmentation model not found at C:\...\AppData\Roaming\com.meetily.ai\models\segmentation-3.0.onnx`. Offline and CI builds without network also produce installers that silently omit the bundle. The resource-near-executable contract is broken and must be fixed.

## What Changes

- **BREAKING (fix)**: Offline and online diarization SHALL resolve the enhanced model files from a 3-location fallback chain — `app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models` (dev) — rather than `app_data` only. The first location where `verify_enhanced_integrity` passes for *both* files is used for `create_segmenter` and `create_speaker_embedder`. Error messages SHALL report all searched locations when none is found.
- Introduce a single resolver `resolve_enhanced_models_dir(app)` / `resolve_enhanced_model_paths(app)` that centralizes the fallback and the `is_enhanced_installed`/`verify_enhanced_integrity` checks; all diarization entry points use it.
- Keep `check_diarization_models` behavior but rewrite it to delegate to the same resolver so UI and engine can never disagree.
- Optionally copy bundled models from `resource_dir/models` to `app_data_dir/models` on first launch (lazy, verified-size-gated) so subsequent runs do not depend on a read-only resource path; if chosen, it SHALL be idempotent and not overwrite a valid `app_data` file.
- Clarify build contract: `build.rs` `ensure_enhanced_models` either succeeds with both files or the build is flagged — at minimum CI SHALL validate that a release build contains both `models/*.onnx` when network was available; offline dev builds continue to warn but SHALL surface the missing-bundle state clearly in logs.
- Update error copy to list searched locations and the "models near executable (bundled) vs AppData" distinction.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speaker-diarization`: model resolution requirement changes from "app_data only" to 3-location fallback (app_data → resource → manifest); engine SHALL load segmentation and embedding from the resolved directory; `check_diarization_models` and engine readiness SHALL use the same resolver.
- `online-speaker-diarization`: Efficient and Fast processors SHALL initialize with the same fallback resolver; initialization error copy SHALL reflect all searched locations.

## Impact

- **Backend**: `frontend/src-tauri/src/audio/embedder.rs` (new resolver `resolve_enhanced_models_dir`/`enhanced_model_paths_for_app`, `create_speaker_embedder` signature or overload), `frontend/src-tauri/src/audio/segmentation.rs` (`create_segmenter`), `frontend/src-tauri/src/audio/diarization.rs` (`create_polyvoice_diarizer`, `run_diarization_blocking`, `check_diarization_models`), `frontend/src-tauri/src/audio/online_diarization.rs` (`OnlineDiarizationProcessor::new`, `create_enhanced_embedder`), potentially `frontend/src-tauri/src/audio/recording_commands.rs` if it resolves models.
- **Frontend**: No API shape change; `check_diarization_models` result stays `{ segmentation_ready, embedding_ready, ready }`. Error toasts that surface diarization failure will show improved message.
- **Build**: `frontend/src-tauri/build.rs` and `frontend/src-tauri/tauri.conf.json` `bundle.resources` verified; optional CI check that `models/*.onnx` are present in bundled resources.
- **DB**: No migration.
- **Dependencies**: No new crates; `tauri::Manager::path().resource_dir()` is the existing API.
