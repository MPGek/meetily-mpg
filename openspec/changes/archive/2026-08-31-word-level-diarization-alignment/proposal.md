# Proposal: word-level-diarization-alignment

## Why

Word-level diarization is implemented but inert: the live recording path hardcodes `tokens: None` when converting `TranscriptUpdate` events into shared transcript segments (recording_commands.rs:701), the Parakeet provider never emits tokens at all (its native per-token frame timestamps are discarded in the engine wrapper), and saved meetings persist `tokens = NULL` into the DB. As a result the N-way token split in both the offline (`start_diarization`) and online (stop-time `finalize`) paths never fires, and speaker ownership remains at segment-level granularity.

Separately, word-level timing precision is currently limited: Whisper word timestamps are a linear interpolation within each segment, and enabling real whisper.cpp DTW timestamps would force disabling flash attention (a ~1.2–1.5× encoder slowdown on GPU tiers that use it). The user wants flash attention kept and a dedicated post-ASR CTC (wav2vec2) forced-alignment pass to obtain word accuracy after transcription. That pass must run **live as each transcription block completes** (final results only, never pending partials), so word/token-level diarization input is available without waiting for the end of the whole meeting.

## What Changes

- **Fix the token break (offline + online)**: transcript-update listeners propagate `update.tokens` into the shared `TranscriptSegment`, so word-level tokens reach `SHARED_SEGMENTS`, `transcripts.json`, the online stop-time `finalize`, and DB persistence.
- **Parakeet word tokens**: the Parakeet engine exposes its existing native per-token encoder-frame alignment (8 ms granularity, `TimestampedResult`) through a new `transcribe_audio_with_tokens`-style path; the transcription worker populates `TranscriptUpdate.tokens` for Parakeet chunks (no interpolation needed).
- **Whisper word tokens**: keep the existing linear-interpolated word timestamps as the baseline (explicitly acceptable) and keep flash attention enabled (no DTW context flag).
- **NEW capability — CTC forced alignment**: a post-ASR word-alignment engine (wav2vec2 CTC encoder + constrained Viterbi over the transcribed text) that refines word start/end times, run **live at block finalization** against the completed block's in-memory audio (final results only; partials never aligned), and as a **repair path** over file-extracted spans in offline re-diarization and stop-time finalize for segments lacking refined tokens. ONNX Runtime is reused via the existing `ort` dependency; a small alignment-model catalog/download follows the established Parakeet model-management pattern.
- **Graceful degradation**: if the alignment model is missing, the audio span is unavailable, the alignment queue overflows (oldest pending block dropped), or the feature is disabled, word tokens fall back to the interpolated (Whisper) or frame-aligned (Parakeet) timestamps — word-level diarization keeps working.
- **Persistence fix**: the frontend save payload carries per-segment `tokens` (DB insert already binds the `tokens` column), so offline re-diarization of already-saved meetings has tokens to split on.

## Capabilities

### New Capabilities

- `ctc-word-alignment`: post-ASR forced alignment producing word-level timestamps (model catalog + download, alignment inference over a text/audio span, enable/disable, fallback to non-aligned tokens), integrated into the live block-finalization flow with the offline and stop-time diarization flows as repair paths.

### Modified Capabilities

- `parakeet-engine`: "Audio transcription via Parakeet" — transcription SHALL additionally return per-word timestamps derived from native per-token encoder-frame alignment (not just text).
- `speaker-diarization`: adds a requirement that offline diarization operates on segments carrying word-level tokens (fixed token supply + alignment refinement before the N-way split).
- `online-speaker-diarization`: adds a requirement that the stop-time finalize operates on segments carrying word-level tokens (fixed token supply + alignment refinement before the N-way expansion).

## Impact

- `frontend/src-tauri/src/audio/recording_commands.rs` — `transcript-update` listeners (≈line 701 and the meeting-name path) must forward `update.tokens`.
- `frontend/src-tauri/src/audio/transcription/worker.rs` — Parakeet branch must populate `TranscriptUpdate.tokens` from the engine's `TimestampedResult`; on final results carrying tokens, hand the block's in-memory samples to the alignment queue (the worker itself never blocks on inference).
- `frontend/src-tauri/src/parakeet_engine/` — expose per-token frame timestamps through the engine (`transcribe_audio` wrapper currently drops `TimestampedResult.tokens/timestamps`).
- New module (e.g. `frontend/src-tauri/src/audio/word_alignment/`) — alignment engine, bounded queue + re-emit consumer, model management, commands.
- `frontend/src-tauri/src/audio/diarization.rs` and `frontend/src-tauri/src/audio/online_diarization.rs` — repair-path refinement before token N-way split/expansion (skip already-refined segments); channel-audio extraction for file spans.
- `frontend/src-tauri/src/api/api.rs` + `frontend/src` types — save payload carries `tokens`.
- `frontend/src-tauri/Cargo.toml` — no new heavy dependencies planned (reuse `ort = 2.0.0-rc.12`); a small forced-alignment implementation (Viterbi) is hand-rolled.
- Settings/store — alignment enable flag and selected alignment model id.
- UX — model download/selection following the existing engine-selector pattern; no change to live UI behavior (alignment runs silently during recording; refined tokens tighten buffered segments via the existing transcript-update path).
