## 1. Model-aware embedder abstraction

- [x] 1.1 Introduce `SpeakerEmbedder` trait (`embed_batch`, `input_dim`, `model_tag`, `family_threshold`) in a new `audio/embedder.rs` and refactor `create_resnet34_embedder` usage in `audio/diarization.rs` and `audio/online_diarization.rs` through it (legacy impl = today's ResNet34 path, tag `resnet34_int8`, τ=0.45)
- [x] 1.2 Implement `TitanetEmbedder` (enhanced impl): load the TitaNet-Large ONNX model via `ort` with the existing CUDA/CPU acceleration selection, 192-d L2-normalized output, batched inference over padded-to-longest segments with a length mask; `model_tag`/`input_dim`/`family_threshold` set correctly
- [x] 1.3 Add a selection helper that builds the enhanced embedder only when both enhanced model files are present and verified, else the legacy embedder; return the active `model_tag` with every embed batch result
- [x] 1.4 Unit-test both impls on synthetic audio: output dims, L2 normalization, batch ordering preserved (i-th output ↔ i-th input), and selection behavior with/without enhanced files

## 2. Enhanced segmentation (offline path)

- [x] 2.1 Add a repo-owned `segmentation-3.0` ONNX session loader (windowed inference matching the model's hop/window) in `audio/diarization.rs`, producing the same segment/output shape consumed by the embedding + clustering stages
- [x] 2.2 Wire the segmentation choice into the offline pipeline: enhanced segmenter when installed, legacy `powerset_int8` `PowersetSegmenter` otherwise; per-channel chunked processing, batched embedding, and AHC clustering unchanged
- [x] 2.3 Integration test: run offline diarization with the enhanced sets available and with only legacy files; assert both complete, produce valid `SPEAKER_NN`/`MIC_SPEAKER_NN` labels, and that enhanced-centroid dimensions match the enhanced family tag

## 3. Enhanced model bundling at build time (no runtime download) and settings

- [x] 3.1 Add build-time fetch: `build.rs` / `scripts/fetch-enhanced-models.*` downloads `onnx-community/pyannote-segmentation-3.0` (public ONNX) and `Recogment/titanet-large-onnx` (`titanet-large.onnx` ~97 MB, public) from Hugging Face, verifies SHA-256 + minisign, and copies verified files into bundled app resources (`models/`); both files required for enhanced; skip gracefully with warning when offline (build without network → legacy only). No `HF_TOKEN`, no `dotenv`. Artifact metadata (URLs, SHA-256, signatures) per design.md Open Questions remains versioned.
- [x] 3.2 Make runtime verification read-only: check bundled files at startup via `is_enhanced_installed` / `verify_enhanced_integrity` (both files required, no 401 fallback), no `download_enhanced_diarization_models` / `remove_enhanced_diarization_models` commands, no progress events, no online transactional install; “removal” is rebuild without bundled files, fallback is automatic
- [x] 3.3 Update the diarization settings panel (frontend) to show **read-only** per-set bundled availability (no download/remove controls); wire the bundled status into the runtime embedder/segmenter selection helper; keep legacy fallback transparent

## 4. Word/token-level speaker assignment

- [x] 4.1 Plumb token-level data (token text, start/end ms) out of the whisper transcription worker alongside segment text/confidence, preserving it through the live and batch transcription result types
- [x] 4.2 Implement `assign_tokens_to_speakers(tokens, turns)`: per-token speaker by max turn overlap, nearest-turn (≤30s) fallback, and boundary detection requiring ≥2 contiguous tokens of a different speaker
- [x] 4.3 Implement transcript row splitting in the diarization finalize path: split a segment into `N` rows (one per contiguous speaker block, each boundary validated by ≥2 contiguous tokens of the new speaker) with per-block `audio_start_time`/`audio_end_time` and sliced `text` preserving `source_device` and ordering; single-speaker segments stay whole; no-token or ambiguous cases fall back to existing overlap matching
- [x] 4.4 Wire token refinement into offline finalize and online stop-time assignment (same `update_transcript_speaker` write path); unit-test: cross-speaker chunk splits, single-speaker chunk untouched, no-token fallback identical to before
- [x] 4.5 Verify downstream consumers (transcript queries, summary input, export) iterate the split rows correctly in time order

## 5. Online (Efficient + Fast live-embedder) integration

- [x] 5.1 Switch Efficient-mode embedding extraction to the model-aware embedder (enhanced TitaNet when installed, ResNet34 otherwise), buffering `model_tag` with embeddings; stop-time clustering applies the family threshold
- [x] 5.2 Keep Fast mode's polyvoice `StreamingPipeline` untouched, but route the repo's own live chunk embedder (used for recognition/enrollment) through the model-aware embedder with its family tag; add stop-time token-level splitting per 4.3
- [x] 5.3 Integration test: online Efficient run with enhanced models installed produces valid labels; live Fast run still emits turns and enrolls only same-family embeddings

## 6. Speaker-identity-registry family guard

- [x] 6.1 Enforce family-aware recognition: filter candidate prototypes by the run's `model_tag` (legacy τ=0.7 stays; TitaNet family uses its calibrated threshold constant); assert cross-family prototypes are never compared
- [x] 6.2 Ensure cache/centroid persistence records the producing model family and that re-match reads only same-family cache rows; update `speaker_storage_stats`/voiceprint browser grouping if they assume a single dimension
- [x] 6.3 Test: legacy-recognized speaker not matched by an enhanced run (and vice versa); user-linked bindings (`matched_by='user'`) across families remain intact

## 7. Calibration, validation, and release polish

- [x] 7.1 Offline calibration pass on held-out meeting audio to set the TitaNet clustering and recognition thresholds; record defaults in code with the "experimental threshold" label until calibrated
- [x] 7.2 Run the Rust test suite (`cargo test`) and typecheck/lint (project conventions per AGENTS.md) for the touched crates; confirm no regressions in legacy-model tests (threshold 0.45, 256-d storage)
- [x] 7.3 Manual QA on a two-speaker meeting and a conference-call recording (enhanced installed vs not): speaker-separated transcript edges at turns, split rows render correctly, fallback behavior when enhanced files are removed, and settings status updates
- [x] 7.4 Update docs (docs/CODEBASE_MAP_*MODULE_AUDIO/PARAKEET/WHISPER as relevant) and AGENTS.md recently-added summary for the model-aware diarization layer and token-level assignment