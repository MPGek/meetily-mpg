## Context

Offline diarization in `frontend/src-tauri/src/audio/diarization.rs` currently decodes the whole recording into a `Vec<f32>` (`decode_audio_file`), then `extract_channels()` de-interleaves into two full-length buffers, and `channel_chunks()` materializes every chunk eagerly before any segmentation runs. A 50-minute 48 kHz stereo recording peaks around 5–7 GB because of this copy chain plus the per-chunk resampler's input clone (`audio_processing.rs:624`) and the concurrent `rayon::join` of both channels.

The `DiarizationMemoryMode` (`auto`/`fast`/`low_memory`) and the `max_sessions` override only tune the ONNX session pool and whether chunking runs — they do not bound the decode/copy memory, and they surface a tradeoff users should not have to make.

ffmpeg is already a first-class dependency (`audio/ffmpeg.rs`, `ffmpeg_sidecar`, used by `decoder.rs` and `audio_file.rs`), so streaming decode is available with no new dependency.

## Goals / Non-Goals

**Goals:**
- Make peak diarization memory flat: hold only one in-memory audio chunk per channel at a time, plus the ONNX session pools.
- Decode, resample, and channel-split via ffmpeg stdout so Rust never materializes the full decoded buffer.
- Remove the `memory_mode` and `max_sessions` settings; use one fixed profile.
- Fix the ONNX session pool size to `min(8, ceil(0.75 × cores))`.
- Keep the existing diarization quality: same models, clustering threshold, speaker-ID format, and chunk overlap.

**Non-Goals:**
- GPU acceleration (unchanged, out of scope).
- Changing the segmentation/embedding models or clustering algorithm.
- Online diarization during recording.
- Removing `max_speakers` (cluster-count ceiling) — it is unrelated to memory mode.
- Temp-file-based chunking (audio chunks stay in memory; no audio temp files).

## Decisions

### 1. Stream decode via ffmpeg stdout (f32le pipe), not temp files

**Decision:** Spawn ffmpeg with `-ar 16000 -f f32le pipe:1` and read raw little-endian f32 samples from stdout into bounded windows. Use `tempfile` only for the existing mkv/webm/wma pre-conversion case, never for chunked audio.

**Rationale:** A pipe avoids disk I/O and temp-file lifecycle management, and lets ffmpeg's native resampler/downmix run in-process with Rust holding only the current window. `f32le` matches the downstream f32 pipeline with no integer→float conversion.

**Alternative considered:** ffmpeg `-f segment` to write per-chunk WAV files, then Symphonia-decode each. Rejected: extra disk I/O and temp-file cleanup, no memory benefit over the pipe.

### 2. One ffmpeg process per channel (mono-aware)

**Decision:** Probe the source's channel count once (Symphonia header read, no full decode — the same probe step already at `decoder.rs:487-510`). Then:
- Stereo: spawn two ffmpeg processes, `-af "pan=mono|c0=c0"` (left/mic) and `-af "pan=mono|c0=c1"` (right/sys), each `-ar 16000 -f f32le pipe:1`.
- Mono: spawn one process, `-ac 1 -ar 16000 -f f32le pipe:1`.

Each process is read by its own worker (the existing `rayon::join` for the two channels).

**Rationale:** Two processes keep each channel an independent, simple stream and avoid cross-platform named-pipe complexity (there is no reliable portable `pipe:2`). Decode runs twice, but ffmpeg decode/resample is cheap relative to ONNX segmentation + embedding.

**Alternative considered:** Single ffmpeg process emitting interleaved stereo, de-interleaving in Rust. Rejected: requires two channel chunk buffers with independent overlap boundaries, adding state-machine complexity for little gain. Can be revisited if decode proves to be a hot path.

### 3. In-memory chunking with streaming overlap carry

**Decision:** Replace the eager `channel_chunks()` (which builds a `Vec` of all chunks) with a streaming window reader: read samples from the pipe until a fixed 600-second (16 kHz) window is full, run segmentation + embedding on that window, drop it, then carry the trailing 5-second overlap forward into the next window.

**Rationale:** Bounds the working set to one chunk (~38 MB at 16 kHz) plus the overlap carry. The 600s / 5s values are the already-shipped `low_memory` defaults and preserve the global-clustering quality guarantees from the prior change.

**Alternative considered:** Smaller chunks (e.g. 60s) for lower peak. Rejected: 38 MB is already negligible, and smaller chunks add more boundary seams with no quality or memory benefit.

### 4. Drop the Rust resampler from the diarization path

**Decision:** With ffmpeg emitting 16 kHz, the diarization path no longer calls `resample()`; its per-chunk input `to_vec()` clone (`audio_processing.rs:624`) disappears from this path. `resample()` remains unchanged for transcription and other callers.

**Rationale:** Removes a copy and lets ffmpeg's swresample handle the anti-aliasing; the segmenter/embedder are robust to minor resampler differences (the VAD-sensitivity concern in `decoder.rs` does not apply to diarization).

### 5. Fixed concurrency profile: `min(8, ceil(0.75 × cores))`

**Decision:** Both the segmenter and embedder session pools are sized to `min(8, (0.75 × logical_cpu_count).ceil())` with a floor of 1. `DiarizationConfig` retains only this derived value plus the fixed chunk constants; `DiarizationMemoryMode`, `memory_mode`, and `max_sessions` are deleted.

**Rationale:** 75% of cores (capped at 8) is the "fast enough, memory-safe" balance the three modes approximated, and it leaves headroom for ffmpeg processes and the main thread.

**Alternative considered:** Hardcode 2 sessions (the old `low_memory` value). Rejected as unnecessarily slow on multi-core machines; the streaming fix already removes the main memory driver, so the pool cap is the only remaining knob and can afford to be moderate.

### 6. Cancellation kills the ffmpeg child

**Decision:** Hold each ffmpeg `Child` handle and call `.kill()` (and read remaining stderr to avoid pipe deadlock) when `DIARIZATION_CANCELLED` is set. Check the flag between windows and within the read loop.

**Rationale:** A long-running ffmpeg process must not outlive a cancelled job. Existing cooperative cancellation is preserved.

### 7. Symphonia fallback path is retained

**Decision:** When `find_ffmpeg_path()` returns `None`, keep the existing `decode_audio_file` + `extract_channels` + chunked path as a fallback (with the fixed pool size), so diarization still works on systems without ffmpeg.

**Rationale:** ffmpeg is bundled/auto-downloaded in production but may be absent in some environments; graceful degradation beats a hard failure.

## Data flow

```
probe (Symphonia header) ──► channels/sample_rate
                                    │
                 ┌──────────────────┴───────────────────┐
                 ▼ stereo                                ▼ mono
   ┌──────────────────────────────┐          ┌──────────────────────────────┐
   │ ffmpeg -i in -vn             │          │ ffmpeg -i in -vn -ac 1        │
   │  -af pan=mono|c0=c0          │          │  -ar 16000 -f f32le pipe:1    │
   │  -ar 16000 -f f32le pipe:1   │          └──────────────┬───────────────┘
   └──────────────┬───────────────┘                         │
                  │  (left)                                 │
   ┌──────────────┴───────────────┐                         │
   │ ffmpeg -af pan=mono|c0=c1    │                         │
   │  (right, same args)          │                         │
   └──────────────┬───────────────┘                         │
                  │                                         │
        ┌─────────┴─────────┐  rayon::join                  │
        ▼                   ▼                               ▼
   read window (600s)   read window (600s)             read window (600s)
   segment + embed      segment + embed                segment + embed
   drop audio           drop audio                     drop audio
        └─────────┬─────────┘                               │
                  ▼                                         ▼
        accumulate embeddings ──► global AHC cluster ──► speaker labels
```

Peak memory ≈ one 600s window (~38 MB) × (active channels) + overlap carry + ONNX pools, instead of the full decoded buffer and its copies.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| ffmpeg swresample output differs from rubato (non-bit-identical labels) | Segmenter/embedder are robust to minor sample differences; validate speaker counts/labels on 30/60-min recordings |
| Two ffmpeg processes double decode CPU/I/O | Decode is cheap vs ONNX; revisit single interleaved process only if profiling shows it matters |
| Reading a pipe can deadlock if stderr fills | Drain stderr in a thread or use `-nostats -loglevel error`; kill child on cancel |
| `pan=mono|c0=c1` fails on mono input | Probe channel count first; never spawn the right-channel process for mono |
| Pool of 8 sessions could raise model RAM on low-RAM machines | Cap at 8 and scale by 75% of cores; floor 1; memory telemetry already logs peak RSS |
| Streamed chunk boundaries may split a speaker turn | Preserve 5s overlap and global clustering (unchanged from prior change) |
| Removing settings changes the public command payloads | `start_diarization`/`get_diarization_settings`/`set_diarization_settings` signatures change; update all frontend callers in the same change |

## Migration Plan

- No database migration.
- Remove `diarizationMemoryMode`/`diarizationMaxSessions` (localStorage) and `memory_mode`/`max_sessions` (`diarization-settings.json`). Stale values are ignored.
- Rollback: revert code; re-introducing the settings is trivial since only config plumbing is removed.
- Validation: run offline diarization on 30-min, 60-min, and 2-hour stereo recordings before/after, comparing wall time, peak RSS (already logged by `MemorySampler`), speaker count, and label assignment.

## Open Questions

1. Is a single interleaved ffmpeg process (decode once) worth the extra de-interleaving state machine, or is two-process acceptable? (Default: two-process.)
2. Should the ffmpeg stdout buffer use `s16le` (half the pipe bandwidth) with int→float conversion in Rust, or stay `f32le` for simplicity? (Default: `f32le`.)
3. Should the fixed 600s chunk duration be reduced once streaming makes it cheap to experiment? (Default: keep 600s.)
