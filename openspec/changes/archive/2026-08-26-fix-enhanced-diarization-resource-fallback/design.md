# Design: Fix enhanced diarization resource fallback

## Context

After `remove-standard-diarization-models`, diarization is enhanced-only (`segmentation-3.0.onnx` + `titanet_large.onnx`, 192-d, tag `titanet_large`). Build-time `frontend/src-tauri/build.rs:206` `ensure_enhanced_models()` fetches both files into `frontend/src-tauri/models/` and `frontend/src-tauri/tauri.conf.json:98` `bundle.resources: ["models/*.onnx"]` packages them into `resource_dir/models` next to the executable (Windows: `C:\Program Files\meetily\resources\models`). 

Runtime diverged:
- `audio/diarization.rs:1561` `check_diarization_models()` verifies across three dirs — `app_data_dir/models` (`%APPDATA%\com.meetily.ai\models`), `resource_dir/models`, `CARGO_MANIFEST_DIR/models` — via `embedder::verify_enhanced_integrity`.
- `audio/diarization.rs:235` offline path, `audio/segmentation.rs:40` `create_segmenter`, `audio/embedder.rs:190` `create_speaker_embedder`, and `audio/online_diarization.rs:487` `OnlineDiarizationProcessor::new` only inspect the passed `models_dir` which is always `app_data_dir/models`.

See proposal.md Why for the failure mode. Existing fallback is already coded for the status check; the consumers missed it. `resource_dir` is read-only in installed builds, so loading directly from there is valid for ONNX sessions, but a lazy copy to AppData can also decouple future runs from the install location.

## Goals / Non-Goals

**Goals:**
- Offline and online diarization resolve models through a single shared 3-location fallback chain, with Settings and engine never disagreeing.
- Error messages list all searched directories and distinguish "bundled near executable" from "AppData".
- Dev (`cargo tauri dev`) continues to work via `CARGO_MANIFEST_DIR/models`.
- Optional first-launch copy from resources → AppData (verified, idempotent) so later runs do not depend on install-dir readability.
- CI/release validation that a network-available build actually bundled both ONNX files.

**Non-Goals:**
- No reintroduction of standard models, downloads, progress events, or ModelRegistry.
- No change to segmentation/embedding/clustering algorithms, thresholds, or pool sizing.
- No DB migration; no change to `speaker-identity-registry` matching beyond which directory provided the embeddings (model tag unchanged).
- No change to Whisper/Parakeet/VAD model resolution.

## Decisions

### D1. Single resolver in `audio/embedder.rs`
- Add `resolve_enhanced_models_dir(app: &AppHandle<R>) -> Option<PathBuf>` and `enhanced_model_paths_for_app(app) -> Option<(PathBuf, PathBuf)>` that iterate `app_data`, `resource_dir`, `manifest` in priority order, returning the first where `verify_enhanced_integrity` passes for *both* files (`verify_enhanced_integrity` already checks existence + `>1KB`; both must pass in same dir). Also add a core helper `resolve_enhanced_models_dir_from_paths(candidate_dirs: &[PathBuf])` testable without `AppHandle`.
- Add `format_enhanced_search_locations(app)` for error messages.
- Keep existing `enhanced_model_paths(p: &Path)` / `is_enhanced_installed(p)` / `verify_enhanced_integrity(p)` unchanged; resolver composes them.
- Rationale: centralizes the policy now duplicated between status check and engines; matches template of `check_diarization_models`'s current three-location logic but makes it reusable. Alternative considered: inlinefallback in each caller — rejected as drift-prone.
- Consequence: `embedder.rs` gains a `tauri` dependency on `AppHandle` for the convenience wrapper; core helper keeps tests free of Tauri.

### D2. Engine entry points resolve via `AppHandle`
- `audio/diarization.rs`: `run_diarization_blocking` currently takes `models_dir: &PathBuf`. Change to take `&AppHandle<R>` (or add overload `run_diarization_blocking_with_app`) and internally call `resolve_enhanced_models_dir`. `create_polyvoice_diarizer` changes signature to `create_polyvoice_diarizer(app, max_speakers, config)` or takes resolved `PathBuf`. Similarly `check_diarization_models` delegates to resolver instead of reimplementing iteration. `audio/segmentation.rs`: `create_segmenter` gains `create_segmenter_for_app(app, pool_size)` wrapper; existing `create_segmenter(path, pool)` stays for tests but production path uses app resolver. `audio/embedder.rs`: `create_speaker_embedder` gains `create_speaker_embedder_for_app(app, pool_size)`; old `create_speaker_embedder(path, pool)` stays for unit tests.
- `audio/online_diarization.rs`: `OnlineDiarizationProcessor::new` currently takes `models_dir: &Path`. Add `OnlineDiarizationProcessor::new_with_app(app, ...)` or change to take `AppHandle`. `create_enhanced_embedder` already checks file existence; after resolver the path is guaranteed verified, but keep its guard with improved error that includes searched locations.
- `audio/recording_commands.rs`: recording-start passes `AppHandle` already; forward it to the processor constructor.
- Rationale: passing `AppHandle` is idiomatic Tauri and already available at every call site (`start_diarization` has `app:AppHandle`, `OnlineDiarizationProcessor::new` is called from recording commands with `app`). Alternative: thread `resource_dir` PathBuf explicitly — more invasive.
- Trade-off: touching `run_diarization_blocking` signature is broader; a minimal variant is to keep signature but compute resolved dir outside and pass resolved path — choose whichever touches fewer call sites after grepping.

### D3. Error messages include all searched locations
- Failure from resolver produces: `"Enhanced diarization models not found. Searched: <app_data> (<exists?>), <resource> (<exists?>), <manifest> (<exists?>). The enhanced models (segmentation-3.0 + TitaNet-Large) are bundled at build time near the executable; rebuild with network or install a build that includes them."`
- Keep existing phrasing for consistency but append location list. Update `segmentation.rs:44`, `embedder.rs:197`, `online_diarization.rs:307/496`.

### D4. Optional lazy copy resources → AppData (first launch)
- In `lib.rs:setup` (or at first `check_diarization_models` if earlier), if `resource_dir/models/{segmentation-3.0.onnx, titanet_large.onnx}` both verify and `app_data_dir/models` does not fully verify, `fs::copy` both files to `app_data_dir/models` after `create_dir_all`. Guard by `verify_enhanced_integrity` on source and `len>1KB` on dest; do not overwrite a dest file that already verifies (idempotent). Log `info!` on copy, `warn!` on failure but continue — engine can still load directly from resources, so copy failure is non-fatal.
- Rationale: mirrors Whisper/Parakeet models-dir initialization in `lib.rs:464` and makes AppData the warm cache, avoiding reliance on read-only Program Files for every session. Alternative: never copy and always read from resources — simpler but leaves AppData empty which confuses users inspecting `%APPDATA%`.
- If copy is omitted, still valid; spec ADDED Requirements only require loading from resources, not copying.

### D5. Build/CI guard
- `build.rs:206` currently warns and continues when offline (`ok=false`). Keep for dev ergonomics, but add `cargo:warning` that clearly states "Diarization will fail at runtime in this build". Optionally add `scripts/verify-bundled-models.ps1` / GitHub workflow check that `target/*/resources/models/*.onnx` or `frontend/src-tauri/models/*.onnx` exist when `cargo build --release` is invoked with network; CI fails if `ready=false` after build.
- Alternative: `panic!` offline — rejected; devs without network should still get a build (with clear runtime error).

### D6. Tests use resolver helper
- Unit tests for resolver use temp dirs for each fallback tier (no `AppHandle`). Spike tests `diarization.rs:1608` `find_models_dir` updated to also check `resource_dir` candidate (or reuse resolver helper). `embedder.rs` tests `create_speaker_embedder_errors_when_enhanced_missing` updated to assert error contains searched locations.

## Risks / Trade-offs

- **Resource dir read-only / ASAR-like path differences** → loading ONNX directly from `C:\Program Files\...\resources\models` must work with `ort` CPU EP. Mitigation: `ort` already loads from resource templates (`templates/*.json` via `resource_dir` in `lib.rs:513`). Verify `FbankOnnxExtractor::new`/`PowersetSegmenter::with_config` accept absolute resource paths on Windows.
- **Signature change touches many call sites** (`run_diarization_blocking`, `create_polyvoice_diarizer`, `OnlineDiarizationProcessor::new`). Mitigation: keep backward-compatible `*_with_app` wrappers and deprecate old `PathBuf`-only entry points; grep and update call sites in one commit.
- **Copy races on first launch (two windows)** → `fs::copy` of 97 MB twice. Mitigation: copy is idempotent and failure-tolerant; guard with `if !is_enhanced_installed(app_data)` check before copy and ignore `ErrorKind::AlreadyExists`.
- **Dev vs prod priority inversion** → `app_data` winning over `resource` means a stale AppData file shadows a newer bundled one after update. Mitigation: on `check_diarization_models`, if `resource` verifies and `app_data` verifies but sizes differ significantly, log `warn!` suggesting cleanup; `cleanup_legacy_models` scope remains standard files only, not enhanced.
- **CI without network still produces "ready=false" build** → user installs release built offline and sees Settings "not ready". Mitigation: CI workflow sets a required check that release artifacts contain both ONNX files; dev builds keep warning but not hard fail.

## Migration Plan

1. Land resolver in `embedder.rs` + wrappers in `segmentation.rs`/`diarization.rs`/`online_diarization.rs` behind app-handle forwarding; update `check_diarization_models` to delegate.
2. Update `lib.rs:setup` (or `check_diarization_models`) with optional lazy copy (feature-flagged or unconditional non-fatal).
3. Update error messages and front-end toast copy (`hooks/useRecordingStop.ts:386`) if it surfaces resolver output.
4. Add `scripts/verify-bundled-models.*` and CI step in `.github/workflows/build-windows.yml` (and macos/linux) to assert `models/*.onnx` bundled.
5. No DB migration. On first run after update, existing `app_data` installs with models there continue to load from AppData (priority), while fresh installs load from resources; lazy copy seeds AppData on next `check_diarization_models`.
6. Rollback: revert resolver to AppData-only; Settings and engine will again split-brain, but no data loss.

## Open Questions

- Whether lazy copy should run on every `check_diarization_models` (ensures repair after user deletes AppData file) or once at startup — defaults to at-startup + on-demand retry if resolver falls back to resources.
- Exact wording of multi-location error (list all three vs only those that exist) — finalize during implementation to match toast length constraints.
