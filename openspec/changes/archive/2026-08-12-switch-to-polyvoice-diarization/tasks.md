## 1. Dependency migration (ort rc.12, no diarization code yet)

- [x] 1.1 In `frontend/src-tauri/Cargo.toml`: change `ort` pin from `=2.0.0-rc.10` to `=2.0.0-rc.12`, bump `ndarray` to `"0.17"`, remove `sherpa-onnx` dep, replace polyvoice dep with `default-features = false, features = ["onnx", "download", "segmentation", "embedder", "clusterer"]`, and remove `tar`/`bzip2` deps (verify nothing else uses them via grep for `tar::`/`bzip2`)
- [x] 1.2 Run `cargo build` in `frontend/src-tauri` and fix ort/ndarray API drift in `src/audio/vad.rs` and `src/parakeet_engine/model.rs` (expected surface: `ort::inputs!`, `Session::builder`, `TensorRef` extraction, ndarray imports/shape APIs) — verify: build passes, `cargo test` on vad/parakeet modules passes
- [x] 1.3 Verify `Cargo.lock` no longer contains `sherpa-onnx` / `sherpa-onnx-sys` (grep lockfile) and ort resolves to exactly `2.0.0-rc.12`

## 2. Model management via polyvoice ModelRegistry

- [x] 2.1 In `src/audio/diarization.rs`: replace `SEGMENTATION_ARCHIVE_URL`/`EMBEDDING_MODEL_URL` constants and `diarization_model_paths` with a `polyvoice::models::ModelRegistry::with_cache_dir(models_dir)` helper; `Profile::Balanced` resolves `powerset_int8` + `resnet34_int8`
- [x] 2.2 Rework `check_diarization_models` to verify both registry model files exist on disk (report missing/corrupt per model; emit the same `DiarizationModelStatus` payload shape to keep the frontend working)
- [x] 2.3 Rework `download_diarization_models` to call `registry.ensure_for_profile(Profile::Balanced)`, emitting `diarization-model-download-progress` per model (name + overall percentage 0–100) and the final `diarization-model-download-complete` event; delete `extract_segmentation_model` and `download_with_progress` if unused elsewhere
- [x] 2.4 Add stale-model cleanup: on `check_diarization_models`, delete legacy `model.int8.onnx` and `3dspeaker_*.onnx` files from the models dir (spec scenario "Legacy model files cleaned up")

## 3. Offline diarization engine swap (audio/diarization.rs)

- [x] 3.1 Replace `create_diarizer` (sherpa `OfflineSpeakerDiarization`) with a `create_polyvoice_diarizer` building: `PowersetSegmenter` (config from `PowersetConfig::default()` + `with_model_meta`), a `ResNet34Adapter::new(embedder_path, pool_size, ExecutionProvider::Cpu)` embedder, and `AhcClusterer::new(max_speakers)`; keep the existing "loaded once, reused per channel" behavior
- [x] 3.2 Replace `run_sherpa_diarization` with a polyvoice run: `segmenter.segment(&samples)` → embed each `RawSegment`'s audio slice (`embedder.embed`) → `clusterer.cluster(&embeddings)` → emit `DiarizationSegment { start, end, speaker }` (handle overlap segments as two entries for the same range; sort by start time; return empty vec for silent channels without failing the run)
- [x] 3.3 Wire the new diarizer into `run_diarization_blocking` (per-channel mic/sys + mono fallback flow unchanged) and delete the old `create_diarizer`/`run_sherpa_diarization` code
- [x] 3.4 Verify: `cargo build` + `cargo test` pass; manual offline diarization run on a recorded stereo meeting assigns `MIC_SPEAKER_NN`/`SPEAKER_NN` labels with progress events (`diarization-progress`) and `diarization_status` transitions (processing → complete)

## 4. Online diarization embedder swap (audio/online_diarization.rs)

- [x] 4.1 Delete `SherpaEmbedder` (struct + `Embedder` impl) and its `sherpa_onnx::SpeakerEmbeddingExtractor` usage; construct the polyvoice `ResNet34Adapter` (same model file as offline) in `OnlineDiarizationProcessor::new` and `create_fast_channel`
- [x] 4.2 Update the embedding model path resolution (replace the hardcoded `3dspeaker_speech_eres2net_...onnx` name with the registry-resolved `resnet34_int8` path) and drop sherpa imports
- [x] 4.3 Add/repoint an ignored spike test (`spike_polyvoice_short_window_embedding`) verifying `resnet34_int8` produces stable embeddings from ~0.25–1.5 s windows (Fast-mode risk from design Decision 4); if quality is poor, switch to `cam_pp_int8` (512-d) — one constructor call change
- [x] 4.4 Verify: `cargo build` + `cargo test` pass; recording with Fast and Efficient modes completes and assigns speaker labels at stop via `recording-stopped` payload (`online_diarization_used: true`, `speaker_assignments` populated)

## 5. Cleanup and final verification

- [x] 5.1 Delete sherpa spike tests in `src/audio/diarization.rs` (`spike_offline_diarizer_with_int8_models`, `spike_embedding_extractor_stream_api`, `spike_embedding_ready_threshold`) and sherpa-referencing polyvoice spikes (L826–920); keep only the polyvoice-native spike
- [x] 5.2 Grep the repo for remaining `sherpa` references (rust src, Cargo files, docs, frontend) and remove/update stale comments (e.g. Cargo.toml comments about the ort conflict and BYO mode, `tauri.conf.json` if referenced)
- [x] 5.3 Full verification: `cargo build` (debug), `cargo test`, frontend `npm run build` (or the project's frontend build command), manual smoke: download models → offline re-analyze speakers → new recording with online mode → labels render in transcript view
- [x] 5.4 Confirm `openspec validate --change switch-to-polyvoice-diarization` passes and update `openspec/` artifacts if the spec delta needs refinement
