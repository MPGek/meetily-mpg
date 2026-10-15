# Design: word-level-diarization-alignment

## Context

Motivation is in proposal.md. Current state that shapes the approach:

- The word-split engine is complete but starved: `assign_tokens_to_speakers` / `split_into_blocks` (token_assignment.rs) already run in both the offline pass (diarization.rs) and the stop-time N-way expansion (online_diarization.rs:951+), but they receive tokens only when `TranscriptSegment.tokens` is populated — which never happens live because recording_commands.rs:701 hardcodes `tokens: None`, and never for Parakeet because its wrapper drops timestamps.
- Whisper already emits interpolated word tokens through `transcribe_audio_with_tokens` (worker.rs:550); Parakeet's model layer already produces `TimestampedResult { text, timestamps, tokens }` at 8 ms frame granularity (parakeet_engine/model.rs) — only the `transcribe_audio` wrapper (parakeet_engine.rs:489) discards them.
- At block-finalization time the transcription worker already owns the exact PCM the segment was transcribed from: per-channel 16 kHz mono `AudioChunk` (`device_type: Microphone | System`, recording_state.rs:19-26), resampled if needed (worker.rs:512-519), VAD-merged to ≤ 25 s (pipeline.rs:828) → ≤ 1.6 MB per block at 64 KB/s. The result tuple distinguishes final from partial (`is_partial`, worker.rs:195), so "completed block" is an existing, provider-correct boundary (OS partials are excluded).
- The transcript-update listener upserts buffered segments by `sequence_id` and rewrites `transcripts.json` on every update (recording_commands.rs:708-735) — a second update for the same block is persisted through the existing path with no new persistence code.
- Stop-time `finalize` runs **post-save** (online_diarization.rs:921-922), so the meeting's audio file is flushed and available at finalize time — the repair path can read real recorded audio.
- `ort = 2.0.0-rc.12` is already a dependency (Silero VAD, Parakeet, diarization ONNX paths); ffmpeg streaming decode (16 kHz f32 mono per channel) already exists for offline diarization.
- The DB layer already persists `transcripts.tokens` when provided (repositories/transcript.rs binds the column with a pre-migration fallback); the frontend save payload is the only persistence gap.
- whisper.cpp DTW timestamps are incompatible with flash attention in the vendored 1.8.3 (DTW path is disabled when FA is on), and the user has decided FA stays on.

## Goals / Non-Goals

**Goals:**
- Word-level tokens flow live → shared segments → finalize → DB save, for both providers.
- A post-ASR CTC forced-alignment refinement that runs **live at block finalization**: every completed (non-partial) segment's tokens are refined against the block's in-memory audio during recording, so word-true timestamps are persisted as the meeting progresses and stop-time / offline flows never re-align refined data.
- Stop-time finalize and offline re-diarization act as a **repair path**: they refine only segments that lack refined tokens (alignment off/missing during recording, legacy meetings, overflow-dropped blocks).
- Zero-regression fallback: every alignment failure mode (model missing, per-segment failure, queue overflow) leaves the pipeline exactly at the ASR-provided tokens.

**Non-Goals:**
- No whisper.cpp DTW / flash-attention changes (explicitly rejected).
- No model retraining, no new ASR providers.
- No synchronous alignment inside the transcription worker's emit path — the worker never blocks on CTC inference; live alignment is decoupled via a bounded queue (D4).
- No rolling ring buffer of recent audio for the live path — the block's samples already exist as a discrete buffer at finalization (D4 alternative).
- No changes to speaker embedding/clustering/recognition logic; the token split algorithm itself is unchanged.
- No live per-word speaker labels in the UI during recording — blocks are still labeled per turn live, and the N-way row split still happens at finalize; this change only guarantees the finalize split operates on word-true timestamps that are already in the buffer.
- No DB schema migration (column already exists; repository already tolerates its absence).

## Decisions

### D1: Parakeet tokens come from native frames, not interpolation
Add `ParakeetEngine::transcribe_audio_with_tokens()` returning `Vec<Token>` built from `TimestampedResult`'s per-token frame indices (frame × 8 ms → seconds, chunk-relative), mirroring the Whisper token contract so worker.rs can offset by `chunk.start_time` identically (worker.rs:264). Words landing in the same frame share a start; a word's end is the next word's start (last word ends one frame later). Keep the old `transcribe_audio` as a text-only wrapper for existing callers.
*Alternative:* linear interpolation like Whisper — rejected: it throws away data the model already produces.

### D2: Alignment engine = ONNX wav2vec2 CTC + hand-rolled constrained Viterbi
New module `frontend/src-tauri/src/audio/word_alignment/` with: model catalog + HF download (D5), an `AlignmentEngine` (load ONNX session via existing `ort`, run CTC posteriors over a 16 kHz f32 span), and a Viterbi aligner that walks a character sequence built from the segment's word list (word-boundary markers between words, blank-aware CTC transitions) to recover per-word frame boundaries. The engine consumes an in-memory f32 span plus its recording-relative start; it does not care whether the span came from the live queue (D4) or file extraction (D3). No new heavy crates.
*Alternatives:* (a) whisper.cpp DTW — requires disabling flash attention (rejected by user); (b) stable-ts / external Python — out-of-process dep, packaging cost; (c) Montforce Aligner — toolchain + dictionary installation, far heavier. Hand-rolled Viterbi is ~150 lines over a `[frames × vocab]` posterior matrix and fully testable with synthetic inputs.

### D3: Audio spans — in-memory on the live path, ffmpeg seek only for repair
The live path (D4) aligns against the chunk's own samples held by the worker at finalization — zero re-decode, zero file I/O, channel-correct by construction. File-span extraction is demoted to the repair path: offline re-diarization and stop-time leftovers use the existing ffmpeg streaming decode with `-ss/-to` seek windows (bounded memory, no full-file buffers), batching adjacent segments into one extraction pass per channel; mic (left) / system (right) split mirrors the existing per-channel diarization so alignment never sees cross-channel audio.
*Alternative:* ffmpeg seek extraction for the live path too — rejected: re-decoding audio the process already produced is pure waste; it only made sense under the old stop-time-only design.

### D4: Live alignment at block finalization via a bounded ownership queue
When the worker produces a **final** result carrying tokens, it moves the block's samples (`Arc<[f32]>` + metadata: `sequence_id`, recording-relative span, channel, ASR tokens) into a bounded mpsc alignment queue and emits the transcript-update exactly as today (ASR-timed tokens). A dedicated consumer task (session pool `min(8, ceil(0.75 × cores))`, shared with the repair path) runs the D2 engine per block and re-emits a `transcript-update` for the same `sequence_id` with refined tokens and `refined: true`; the existing listener upsert (recording_commands.rs:708-715) persists the refinement into `SHARED_SEGMENTS` / `transcripts.json` / DB with no new plumbing. Partial results are never aligned. Queue overflow drops the **oldest** pending block (that block keeps ASR tokens — one of the spec's fallback modes). At recording stop the queue is closed and drained with a bounded wait (per-block timeout); whatever is still unrefined is handled by the repair hook before the N-way split. The shared `refine_segment_tokens(segments, audio_source, settings)` entry point remains for the offline and stop-time repair flows and skips segments already carrying `refined` tokens (idempotency). The split algorithm and boundary rule (≥2 contiguous tokens) stay untouched.
*Emit policy:* emit-then-refine (above) rather than align-before-emit — the transcript appears at ASR speed and only token timestamps silently tighten on the second update (text is unchanged, so no UI jitter); the worker's latency is isolated from CTC inference.
*Alternatives:* (a) synchronous alignment in the worker — rejected: delays every final transcript by inference time and couples ASR throughput to the aligner; (b) rolling ring buffer of recent audio per channel, copied out per completed block — rejected for the live path: the block's audio already exists as a discrete in-memory buffer at finalization, so a ring adds an overwrite race and a second copy for no benefit; recorded sizing if a timestamp-only trigger is ever needed: lookback ≈ 3 min/channel (≈ 11 MB each, 22 MB stereo) covering in-flight chunks (3 workers × 25 s) plus CPU-alignment backlog.

### D5: Model management mirrors the Parakeet pattern
Catalog of alignment models (id, repo, files, size, language coverage), multi-file HF download with progress + Range resume, integrity validation (existence + min size), delete — cloned structurally from `parakeet_engine/model.rs` conventions. Models live under `app_data_dir/models/alignment/<id>/` (user-downloadable, unlike bundled diarization models, so no resource-dir fallback chain). Readiness check command + status surfaced in settings.
*Default model:* one small multilingual wav2vec2 CTC aligner export; catalog is extensible to per-language models later.

### D6: Token plumbing fixes are minimal and additive
- recording_commands.rs:701 and the meeting-name path: `tokens: update.tokens.clone()`.
- recording_saver's `TranscriptSegment` already has a `tokens` field — verify it serializes into `transcripts.json`.
- Frontend: add `tokens` to the transcript save type and payload (api.rs `TranscriptSegment.tokens` already accepts `Option<serde_json::Value>`; the repository INSERT already binds the column with a no-column fallback).
- The refined re-emit (D4) rides the same listener upsert — no additional persistence work.
- Whisper path unchanged (interpolated tokens remain the baseline when alignment is off).

### D7: Settings and UX
`wordAlignmentEnabled` (default **on**) + `alignmentModelId` in the settings store, with a "Word alignment" section in the diarization settings panel: enable toggle, model status (missing/download with progress/delete), mirroring the transcription-engine selector pattern. No live-UI behavior change — blocks are still labeled per turn during recording; alignment silently tightens the buffered tokens so the stop-time split is word-true and pays no alignment cost, and offline re-diarization of saved meetings finds refined tokens already in the DB.

## Risks / Trade-offs

- [ONNX export of the aligner may not bundle the conv feature-extractor front-end] → Spike task first: verify the chosen export takes raw 16 kHz f32 waveform; if not, export the front-end as a companion ONNX graph and chain two sessions (still `ort`-only).
- [Alignment throughput falls behind speech rate (slow CPU, 100% duty on both channels) and the queue backs up] → Demand is bounded by ≤ 1× real-time speech rate (128 KB/s stereo); the pooled engine runs ≈ 2–20× real-time, so steady-state backlog is shallow. Hard bound: queue capacity 128 MB ≈ 80 max-size blocks; overflow drops oldest → those blocks keep ASR tokens (spec fallback), never OOM, never block the worker.
- [A refined update could arrive after the meeting is saved, persisting unrefined tokens] → Save happens after stop; stop closes + drains the queue with a bounded wait before finalize, and the repair hook refines anything left over the saved file — the DB always sees refined-or-baseline, never a torn state.
- [Words containing characters outside the model alphabet (diacritics, emoji, code-switching)] → Per-segment fallback: if the Viterbi emission sequence can't be constructed, keep ASR tokens; never drop text.
- [Alignment shifts timestamps slightly wrong (acoustic ambiguity) and moves a split boundary] → Boundary rule already requires ≥2 contiguous tokens per speaker block, absorbing single-token jitter; user per-block relabels remain authoritative.
- [Mic channel has residual no-AEC system bleed; alignment may lock onto the wrong voice's timing] → Same limitation as diarization itself; alignment only refines timestamps of text the mic ASR produced, so worst case is a boundary off by a word — unchanged from interpolated baseline.
- [Three refinement call sites (live queue, stop-time repair, offline repair) can drift] → One `AlignmentEngine`, one `refine_segment_tokens` function, one `refined` flag as the single source of truth; D3 extraction is shared by both repair flows.

## Migration Plan

1. Land plumbing fixes (D6, D1) — immediately restores word-level splitting with existing interpolated/frame tokens, no new subsystem.
2. Land `word_alignment` module + settings (D2, D5, D7) — engine, catalog, download, repair hook, all reachable but not yet wired live.
3. Land the live queue handoff (D4/D3) behind the default-on toggle; missing model degrades silently to ASR tokens.
4. Rollback: toggle off (no behavior change vs today's intent), or revert the module — plumbing fixes are independent and non-breaking.

No data migration: existing meetings keep `tokens = NULL` and continue to use segment-level matching; meetings saved while alignment was off are refined by the offline repair pass on re-diarization.

## Open Questions

- Exact default alignment model id (small multilingual vs English-only-first catalog entry) — decide during the D2 export spike; the catalog design accommodates either without changing specs or tasks.
- Final queue capacity constant (128 MB is the initial bound) — tunable at implementation without changing specs or the approach.
