## Context

The app diarizes recorded meetings with a `polyvoice` 0.17 crate (pinned) that bundles powerset segmentation + WeSpeaker ResNet34 (256-d) ONNX models, loaded via a repo-owned builder (`create_resnet34_embedder`, batched `embed_batch`) in `audio/diarization.rs` and `audio/online_diarization.rs`. AHC threshold is fixed at 0.45; transcript labels are assigned by whole-segment overlap (`find_best_speaker`) with 30s gap-filling. Whisper transcription already runs `set_token_timestamps(true)` (see whisper_engine.rs) but the timestamps are not used for assignment. Voiceprints in `speaker_embeddings` are already tagged by a `model` column, and `meeting_speakers` caches centroids. GPU/CPU-EP plumbing already exists via `ort` (see proposal.md — Why/What/Impact).

## Goals / Non-Goals

**Goals:**
- Add a repo-owned, model-aware diarization layer that runs the *same* pipeline skeleton (chunked processing, batched embedding, per-channel parallelism, AHC, recognition/enrollment) against a stronger model set — pyannote `segmentation-3.0` + TitaNet-Large (192-d) — whenever it is installed, with zero behavioral change otherwise.
- Use Whisper token timestamps to refine speaker ownership to sub-segment granularity, splitting transcript rows at detected speaker changes.
- Keep Fast (streaming) mode on the untouched polyvoice `StreamingPipeline`; only the repo's own live-embedding path (recognition/enrollment) becomes model-aware.

**Non-Goals:**
- No new ASR/alignment models (no forced-phoneme MMS alignment), no Canary/NeMo engines, no pre-splitting audio at diarization boundaries before transcription.
- No changes to capture, VAD, or transcription chunking.
- No change to polyvoice internals (do not fork or patch the pinned crate).
- No multichannel/overlap-specific handling beyond today's mic/sys split.

## Decisions

### D1. Repo-owned model-aware embedder abstraction
Introduce a trait (e.g. `SpeakerEmbedder`) with `embed_batch`, `input_dim`, `model_tag`, and `family_threshold`:
- **Legacy impl** wraps the exact code path used today (`create_resnet34_embedder` / ResNet34 ONNX, 256-d, τ=0.45).
- **Enhanced impl** loads TitaNet-Large ONNX via `ort` (same acceleration helpers as existing CUDA/CPU selection), 192-d, L2-normalized outputs, batch inference padding all segments in a batch to the longest with a length mask, threshold set by a calibrated constant.
Selection: enhanced impl is constructed when both enhanced models verify on disk; otherwise the legacy impl; the chosen `model_tag` accompanies embeddings, cache rows, centroids, and the recognition query so a single run never mixes families.

*Alternatives considered:* (a) swapping model files inside polyvoice's own `ModelRegistry` — rejected, adapters (ResNet34-specific 256-d) are not swappable without forking the crate; (b) adopting `pyannote-rs`/Oolong crates — rejected, their APIs differ and online streaming mode would need re-implementation.

### D2. Enhanced segmentation plugs into the same turn pipeline
When enhanced models are installed, `audio/diarization.rs` runs `segmentation-3.0` ONNX directly (repo-owned ort session, windowed inference over the existing 600s/5s chunks) producing the same kind of speech segments/powerset-style speaker-activity masks that feed the existing embedding + AHC stages. The legacy `PowersetSegmenter` remains the fallback. Turn structs and transcript-matching code are unchanged.

### D3. Word/token-level assignment is a refinement pass, not a replacement
Token timestamps are already computed by whisper; plumb them (token text, start/end ms) out of the transcription worker alongside the segment text. In the diarization finalize path:
1. If a segment has token timestamps and diarization turns exist → assign each token to the speaker of the covering turn (max overlap; nearest-turn fallback within 30s).
2. Group consecutive tokens by speaker with the ≥2 contiguous-token rule per boundary; if a single speaker spans the segment, keep one labeled row.
3. Otherwise split the stored transcript row into `N` rows (one per contiguous speaker block) at each validated boundary (first token of each new block), adjusting `audio_start_time`/`audio_end_time` per block to its token span, preserving `source_device`/`confidence`/`sequence_id`-derived ordering; resulting blocks are contiguous and gap-free (e.g. A `0.0–1.2`, B `1.2–2.8`, A `2.8–4.0`).
4. Ambiguous or timestamp-less cases fall back to the existing segment-overlap matching. This keeps the change additive and reverts cleanly if token timestamps regress. `duration` and `display_time` are recomputed per split row.

*Alternatives considered:* pre-splitting audio by diarization turns before transcription — rejected (spec discussion, harms Whisper context on short turns); forced-phoneme alignment — out of scope (non-goal), heavier dependency.

### D4. Model management via build-time bundling (no runtime download)
The enhanced set (two artifacts) is fetched at **build time** by `build.rs` (or `scripts/fetch-enhanced-models.*` invoked from `build.rs`) from public Hugging Face mirrors — `onnx-community/pyannote-segmentation-3.0` (segmentation-3.0 ONNX, public) and `Recogment/titanet-large-onnx` (`titanet-large.onnx` ~97 MB, public) — verified (SHA-256 + minisign) and copied into the app resources / `models/` bundle. No `HF_TOKEN`, no `dotenv`, no auth required. The two artifacts are tracked as **bundled** only when both verify at build; at runtime the app only checks bundled files exist and re-verifies hash (no network, no progress events, no re-download). Settings panel shows **read-only** per-set bundled status. No DB migration: `speaker_embeddings.model` already exists; `meeting_speakers` centroids inherit the run family by construction.

### D5. Per-family thresholds, current values preserved
Keep 0.45 for ResNet34 and 0.7 recognition τ for legacy. TitaNet clustering and recognition thresholds are new constants (initially a conservative default), calibrated by an offline tuning pass over held-out meeting audio before the enhanced set is promoted to default-recommended.

## Risks / Trade-offs

- [TitaNet cosine threshold unknown (`D5`)] → Ship enhanced set with an explicitly labeled "experimental threshold" constant tuned offline; legacy path and threshold untouched, so a bad TitaNet threshold cannot regress the default-experience beyond the enhanced set's own quality.
- [segmentation-3.0 ONNX export differs subtly from torch reference] → One-off offline verification (compare outputs vs pyannote python reference on a sample set) at model-artifact build time; add integration test asserting segmentation runs and produces non-degenerate turns.
- [Token timestamps drift (Whisper token-level time is coarse)] → Refinement only splits on a clear boundary (≥2 contiguous new-speaker tokens); all other cases stay whole-segment, so a drift produces no worse result than today.
- [New transcript rows change downstream aggregates (summary, export)] → Splits retain all other columns and contiguous audio span; downstream consumers already iterate transcript rows in time order.
- [Enhanced models not bundled at build] → Build does not fail: `build.rs` logs warning and skips enhanced bundling, app falls back to legacy polyvoice at runtime. Both enhanced mirrors are public (no auth), so missing bundle is only due to build-time network/offline. Runtime never attempts online download, so offline/air-gapped installs remain deterministic.

## Migration Plan

- **Deploy:** additive — enhanced models are **bundled at build time** from public mirrors (`onnx-community/pyannote-segmentation-3.0` + `Recogment/titanet-large-onnx`); builds without network simply skip bundling and continue on legacy with no behavior change. No runtime download, no network required at runtime, no `HF_TOKEN`. `build.rs` verifies SHA-256 + minisign before bundling.
- **Rollback:** rebuild without the two bundled files; pipeline falls back to legacy at runtime (verification fails → legacy); re-running diarization on affected meetings restores legacy-family labels consistent with current re-analysis semantics. No settings “remove” action needed.
- **Data:** no migration; families coexist in `speaker_embeddings` under distinct `model` tags.

## Open Questions

- Exact TitaNet-Large ONNX artifact (source repo, int8 vs fp32 variant) and where its SHA-256/signature records are published — resolved at implementation time without changing specs or design.
- Calibrated numeric values for the enhanced clustering and recognition thresholds — tuned offline before promoting; specs require only per-family constants.