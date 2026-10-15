## 1. Resolver core (embedder.rs)

- [x] 1.1 Add core helper `resolve_enhanced_models_dir_from_paths(candidates: &[PathBuf]) -> Option<PathBuf>` in `frontend/src-tauri/src/audio/embedder.rs` that returns the first candidate where `verify_enhanced_integrity` passes for both `segmentation-3.0.onnx` and `titanet_large.onnx` in the same directory
- [x] 1.2 Add Tauri wrapper `resolve_enhanced_models_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf>` and `enhanced_model_paths_for_app` / `format_enhanced_search_locations` that build the 3-location chain `app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models` (verify order and priority)
- [x] 1.3 Update `create_speaker_embedder` to add `create_speaker_embedder_for_app(app, pool_size)` wrapper that uses resolver; keep `create_speaker_embedder(path, pool)` for tests but production calls use app wrapper; error message includes all searched locations
- [x] 1.4 Add unit tests for resolver: temp dirs for app_data-only, resource-only, manifest-only, both-present (app_data wins), none-present (None + error lists dirs)

## 2. Segmentation and offline diarization wiring

- [x] 2.1 In `frontend/src-tauri/src/audio/segmentation.rs` add `create_segmenter_for_app(app, pool_size)` wrapper using resolver; update `create_segmenter(path, pool)` error to include searched locations when used via app path; keep existing perf tests green
- [x] 2.2 In `frontend/src-tauri/src/audio/diarization.rs` refactor `create_polyvoice_diarizer` to `create_polyvoice_diarizer_for_app(app, max_speakers, config)` (or add app overload) that obtains resolved dir before building segmenter/embedder; keep old signature behind wrapper for spike tests if needed
- [x] 2.3 In `frontend/src-tauri/src/audio/diarization.rs` update `run_diarization_blocking` to take `AppHandle` (or resolved `PathBuf` computed from app in `start_diarization`) and use resolver; update `start_diarization` caller to pass `app` instead of `models_dir`
- [x] 2.4 Rewrite `check_diarization_models` in `audio/diarization.rs` to delegate to `resolve_enhanced_models_dir(app)` / `format_enhanced_search_locations` so readiness and engine share verification; keep `DiarizationModelStatus { segmentation_ready, embedding_ready, ready }` shape
- [x] 2.5 Update error copy in offline path (`create_polyvoice_diarizer` / `run_diarization_blocking`) to list searched locations and note "bundled near executable; rebuild with network or install build that includes them" (see design D3)

## 3. Online diarization wiring

- [x] 3.1 In `frontend/src-tauri/src/audio/online_diarization.rs` add `OnlineDiarizationProcessor::new_with_app(app, mode, max_speakers, has_system_device, turn_sender, prototype_store)` that uses resolver; keep `new(models_dir, ...)` for tests but production recording path uses app variant
- [x] 3.2 Update `create_enhanced_embedder` error path to include searched locations when called via app resolver
- [x] 3.3 In `frontend/src-tauri/src/audio/recording_commands.rs` (and any other `OnlineDiarizationProcessor::new` call site) forward `AppHandle` and switch to `new_with_app`
- [x] 3.4 Verify Efficient and Fast mode thresholds and channel prefixes unchanged; add a test that `PrototypeStore` load via `ENHANCED_MODEL_TAG` still 192-d when resolved from resource dir

## 4. Lazy copy and build guards

- [x] 4.1 In `frontend/src-tauri/src/lib.rs` `setup` (or at first `check_diarization_models`), add non-fatal lazy copy: if `resource_dir/models` verifies and `app_data_dir/models` does not fully verify, `create_dir_all` + `fs::copy` both ONNX files to AppData (idempotent, skip if dest already verifies, log `info!` on copy, `warn!` on failure)
- [x] 4.2 In `frontend/src-tauri/build.rs` keep `ensure_enhanced_models` offline warning but improve log: "Diarization will fail at runtime in this build (no bundled models)"; add `scripts/verify-bundled-models.ps1/.sh` that asserts `frontend/src-tauri/models/*.onnx` and `resource_dir/models/*.onnx` present for release builds
- [x] 4.3 Add CI step in `.github/workflows/build-windows.yml` (and `build-macos.yml`/`build-linux.yml`) that runs `verify-bundled-models` after `ensure_enhanced_models` when network is available; fail release if `ready=false`

## 5. Verification and polish

- [x] 5.1 Update spike test `audio/diarization.rs:1608` `find_models_dir` to also consider `resource_dir` candidate or reuse resolver helper; ensure `MEETILY_MODELS_DIR` env override still wins
- [x] 5.2 Manual verify: fresh install on Windows with models only in `resource_dir/models` — Settings shows `Ready ✓` and offline `start_diarization` succeeds; fresh dev `cargo tauri dev` with models only in `manifest/models` succeeds; install with both locations present loads from AppData
- [x] 5.3 Manual verify: install built offline without models (no `models/*.onnx`) — Settings shows not ready, offline and online diarization fail with error listing all three searched directories; copy-restore by dropping files into AppData makes next run succeed
- [x] 5.4 Grep `enhanced_model_paths(` / `is_enhanced_installed(` / `verify_enhanced_integrity(` call sites and confirm only intentional `path`-only usages remain in tests; production paths all go through resolver
- [x] 5.5 Update user-visible copy: `hooks/useRecordingStop.ts:386` toast and `components/DiarizationSettings.tsx` caption if they duplicate the build-time bundling explanation, to match new multi-location error wording
