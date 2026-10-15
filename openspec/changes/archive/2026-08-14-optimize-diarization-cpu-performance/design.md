## Context

Meetily's offline speaker diarization runs in `frontend/src-tauri/src/audio/diarization.rs`. It uses polyvoice 0.17.0 with a `PowersetSegmenter` (ONNX segmentation) and a `ResNet34Adapter` (ONNX speaker embedding), followed by AHC clustering.

Current observations from the codebase:
- `PowersetSegmenter` already defaults to a session pool of `clamp(available_parallelism, 1, 4)` and micro-batch size 8, so segmentation is multicore.
- `ResNet34Adapter` is constructed with `pool_size = 1` and `ExecutionProvider::Cpu`.
- The embedding stage calls `embed()` serially for every detected segment.
- Microphone and system channels run sequentially.
- The entire decoded audio file is held in memory for the duration of diarization.
- polyvoice 0.17.0 does not wire `ExecutionProvider::Cuda` or DirectML, so GPU acceleration is out of scope for this change.

The result is that diarization is effectively single-core during embedding and memory usage grows linearly with recording length.

## Goals / Non-Goals

**Goals:**
- Use all available CPU cores during offline diarization embedding extraction.
- Run microphone and system-channel diarization in parallel on stereo recordings.
- Add user-tunable concurrency settings and an automatic low-memory mode.
- Cap memory growth for long recordings via chunked audio processing.
- Preserve existing diarization quality (no change to models, clustering threshold, or speaker-ID format).
- Add stage-level timing and memory telemetry so regressions are visible in logs.

**Non-Goals:**
- GPU acceleration (CUDA, DirectML, CoreML, XNNPACK) — out of scope.
- Changing the diarization model, embedding dimension, or clustering algorithm.
- Online diarization performance during recording.
- Database schema changes.

## Decisions

### 1. Use `embed_batch()` with a tunable session pool for embedding

**Decision:** Replace the per-segment `embed()` loop with a single `embed_batch()` call over all segment audio slices. Increase the embedder `pool_size` from 1 to a value derived from CPU cores and available memory.

**Rationale:** polyvoice's `Embedder::embed_batch` already implements `parallel_embed_batch`, which fans out across the internal ONNX session pool. Using it requires no new dependency and keeps the same model and preprocessing path.

**Alternative considered:** Manually spawn threads each calling `embed()`. Rejected because it duplicates the pool logic already present in polyvoice and would compete with the session pool for cores.

### 2. Parallelize microphone and system channels

**Decision:** Run the two channel diarization passes concurrently using `rayon::join` (or `std::thread::scope`) over a shared `PolyvoiceDiarizer`.

**Rationale:** The channels are independent after de-interleaving. `PolyvoiceDiarizer` contains `RuntimeSession` pools that are `Send + Sync` via mutex-backed object pools, so sharing one instance across two threads is safe.

**Alternative considered:** Clone the diarizer per channel. Rejected because it doubles model-session memory for no throughput gain; sharing uses the existing pool more efficiently.

### 3. Add concurrency and memory-mode settings

**Decision:** Add two new settings stored in the Tauri settings store:
- `diarization_max_sessions`: an integer (1–16, default auto = `min(num_cpus, 4)`).
- `diarization_memory_mode`: `"auto"`, `"fast"`, or `"low_memory"`.

In `"auto"` the app picks `min(num_cpus, 4)` embedder sessions and enables chunking only when audio exceeds a duration threshold. In `"fast"` it maximizes concurrency. In `"low_memory"` it limits sessions to 2 and always chunks.

**Rationale:** Gives users control without exposing raw pool sizes. The defaults keep current behavior conservative on low-end machines while unlocking performance elsewhere.

### 4. Chunked processing for long recordings

**Decision:** When audio duration exceeds a threshold (default 10 minutes in `"auto"`, always in `"low_memory"`), split each channel into overlapping chunks, run segmentation + embedding per chunk, then cluster all accumulated embeddings together.

**Rationale:** This keeps the working set bounded: only one chunk's audio, segmenter windows, and embeddings are live at a time. The global clustering step preserves speaker consistency across chunks because the same voice appears in embeddings from multiple chunks.

**Alternative considered:** Cluster per chunk and merge centroids. Rejected because a single global clustering over all embeddings is simpler and avoids heuristic merge errors; embeddings are small enough to keep in memory even for two-hour recordings.

### 5. Keep AHC clustering and the 0.45 threshold

**Decision:** Do not change the clustering algorithm or threshold.

**Rationale:** The goal is faster execution with identical output. Changing clustering would require re-evaluating DER and is out of scope.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| Higher session counts increase peak memory | Cap default pool at 4; provide low-memory mode; add telemetry to catch regressions |
| Parallel channels temporarily double peak CPU/memory | Channels still share one diarizer; peak is higher but duration is shorter, so total memory-time may improve |
| Chunk boundaries could split a single speaker turn | Use generous overlap (e.g., 5 seconds) and global clustering over all embeddings |
| Cancellation becomes more complex with parallel work | Use scoped threads and check `DIARIZATION_CANCELLED` between chunks; cooperative cancellation already exists |
| polyvoice session pool may not be thread-safe under heavy contention | Verify via tests; polyvoice's `ObjectPool` uses a `Mutex<Vec<T>>` and is documented as `Send + Sync` |
| Long AHC clustering time for many embeddings | For very long recordings, clustering can itself be parallelized later if profiling shows it matters |

## Migration Plan

- No database migration is required.
- New settings receive sensible defaults (`auto` mode), so existing users are unaffected until they opt into faster/low-memory modes.
- Rollback: revert the code changes; settings keys are harmless if ignored.
- Validation: run diarization on representative recordings (30 min, 60 min, 2 h) before and after, measuring wall time, peak RSS, and speaker count.

## Open Questions

1. What is the exact default chunk duration and overlap? 10-minute chunks with 5-second overlap is a starting guess; it should be validated against real recordings.
2. Should the max-speakers setting influence chunking? (Probably not, but clustering memory is O(n²) in segment count.)
3. Do we need a progress message for the chunking stage, or can it reuse existing `diarizing`/`matching` statuses?
4. Should telemetry be sent to PostHog, or is local logging sufficient for the first iteration?
