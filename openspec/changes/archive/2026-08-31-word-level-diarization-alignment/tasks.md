# Tasks: word-level-diarization-alignment

## 1. Token plumbing fixes (live path → finalize → DB)

- [x] 1.1 Forward `update.tokens` in both `transcript-update` listeners in recording_commands.rs (replace hardcoded `tokens: None` at ~line 701 and the meeting-name path); verify `cargo check` passes and a live transcript-update event lands tokens in `SHARED_SEGMENTS` (add/extend a unit test or observe event payload in a manual recording run)
- [x] 1.2 Verify recording_saver's `TranscriptSegment.tokens` round-trips through `transcripts.json` into the segments handed to stop-time finalize; verify with a serialization round-trip test (tokens present before and after save/load)
- [x] 1.3 Add `tokens` to the frontend transcript type and save payload (api.rs already accepts `Option<serde_json::Value>`; DB insert already binds the column); verify: record + save a meeting, then query `SELECT tokens FROM transcripts` and see a non-NULL JSON array

## 2. Parakeet word tokens (native frames, no interpolation)

- [x] 2.1 Add `ParakeetEngine::transcribe_audio_with_tokens()` building `Vec<Token>` from `TimestampedResult` frame indices (8 ms → seconds, chunk-relative, non-decreasing, same-frame words share start, last word ends one frame after its start); keep `transcribe_audio` as text-only wrapper; verify: unit tests on synthetic frame sequences cover same-frame and last-word cases
- [x] 2.2 Populate the Parakeet branch in transcription worker (worker.rs:591) so `TranscriptUpdate.tokens` is offset by `chunk.start_time` exactly like the Whisper branch (worker.rs:550-587), and empty text yields no tokens; verify: `cargo test` passes and a live Parakeet recording emits transcript-updates carrying tokens

## 3. Alignment model spike + management module

- [x] 3.1 Spike: pick a wav2vec2 CTC forced-alignment ONNX export and confirm the graph accepts raw 16 kHz f32 waveform input (if not, export the conv front-end as a companion ONNX graph); deliverable: chosen model id + file list recorded in the module docs and catalog
- [x] 3.2 Create `frontend/src-tauri/src/audio/word_alignment/` module skeleton: model catalog (id, HF repo, files, size, language coverage), readiness resolver under `app_data_dir/models/alignment/<id>/` (existence + min-size validation); verify: `cargo check` passes and the status command reports Missing/Available correctly for a fake dir
- [x] 3.3 Implement HF multi-file download with weighted progress, Range resume, cancel (partial cleanup), and delete, mirroring the Parakeet model-management pattern; verify: download the default model end-to-end, then cancel a second download mid-flight and confirm partial files are removed and status resets to Missing
- [x] 3.4 Expose Tauri commands (`list_alignment_models`, `download_alignment_model`, `delete_alignment_model`, `check_alignment_models`) and type the frontend API stubs; verify: commands invokable from devtools console and return expected shapes

## 4. Alignment engine (CTC posteriors + constrained Viterbi)

- [x] 4.1 Implement ONNX session loading via existing `ort` with a bounded session pool `min(8, ceil(0.75 × cores))` and posterior inference over a 16 kHz f32 span; verify: unit/integration test on a 1 s clip returns a `[frames × vocab]` matrix with sane probabilities
- [x] 4.2 Implement the character-sequence builder (word-boundary markers, CTC blank-aware constrained Viterbi) producing per-word frame boundaries and refined `Token` start/end in seconds; verify: unit tests with synthetic posterior matrices recover known word boundaries, and a word containing an out-of-alphabet character makes the builder return per-segment fallback (false) instead of erroring
- [x] 4.3 Implement `AudioSpanSource` (repair paths only — the live path aligns the worker's in-memory block): ffmpeg `-ss/-to` seek extraction to bounded 16 kHz f32 mono windows, batching adjacent segments per channel (mic=left, system=right); verify: extracted window duration matches the requested span within one frame on a test recording, and mono files map the whole file to one channel

## 5. Live alignment queue + integration hooks

- [x] 5.1 Implement shared `refine_segment_tokens(segments, audio_source, settings)` entry point: no-op when disabled/model missing, per-segment try + timeout, in-place token mutation, pre-alignment tokens kept on any failure, segments already flagged `refined` skipped; verify: unit tests cover disabled, missing-model, already-refined, and per-segment-failure fallback paths
- [x] 5.2 Implement the bounded alignment queue + worker hand-off: on a final `transcribe_chunk_with_provider` result carrying tokens, move `Arc` samples + metadata (sequence_id, recording-relative span, channel, ASR tokens) into the queue (capacity 128 MB, drop-oldest on overflow); partial results are never queued; verify: unit tests cover final-queued, partial-skipped, and overflow-drops-oldest-without-blocking-the-worker
- [x] 5.3 Implement the queue consumer: run the alignment engine per block on the shared session pool, mark tokens `refined`, re-emit `transcript-update` for the same `sequence_id`; verify: a simulated recording run shows the buffered segment's tokens tightened in `SHARED_SEGMENTS`/`transcripts.json` after the second update
- [x] 5.4 Drain on stop: close the queue at recording stop and wait for in-flight blocks with a per-block timeout before finalize runs; verify: stopping with a deep backlog finalizes with refined-or-baseline tokens and never hangs
- [x] 5.5 Wire the repair hook into offline diarization (diarization.rs) immediately before `assign_tokens_to_speakers`, per-channel, from file spans; verify: re-diarize a saved meeting recorded with alignment off and confirm split rows use aligned boundaries (log token spans before/after)
- [x] 5.6 Wire the repair hook into stop-time finalize (online_diarization.rs) before the N-way expansion loop (line ~951) for segments left unrefined after drain, reading the saved post-flush meeting file; verify: stop a two-speaker recording and confirm the cross-speaker chunk is stored as separate transcript rows with per-block boundaries

## 6. Settings & UX

- [x] 6.1 Persist `wordAlignmentEnabled` (default on) and `alignmentModelId` in the settings store; verify: values survive app restart and are read by `refine_segment_tokens`
- [x] 6.2 Add a "Word alignment" section to the diarization settings panel (enable toggle + model status/download/cancel/delete mirroring the engine-selector pattern); verify: UI transitions Missing → download progress → Ready and the toggle persists

## 7. End-to-end verification & docs

- [x] 7.1 Run the verification matrix {Whisper, Parakeet, OS partials} × {live finalization, stop-time repair, offline repair} × {alignment on, off, model missing}: final blocks are refined during recording, partials are never aligned, already-refined segments are never re-aligned, stop-time and offline split on refined-or-baseline tokens whenever tokens exist, and tokenless legacy rows still use segment-level matching with no row splits; record results in the change folder
- [x] 7.2 Run `cargo clippy`, `cargo test`, and the frontend typecheck/build for the touched packages; verify all pass clean
- [x] 7.3 Update `docs/CODEBASE_MAP*` (new `word_alignment` module + changed audio/diarization flow) and run `graphify update .`; verify the map files reference the new module and dirty graph files are committed-worthy
