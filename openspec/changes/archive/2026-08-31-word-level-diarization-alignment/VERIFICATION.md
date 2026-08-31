# Verification: word-level-diarization-alignment

Automated verification performed during implementation (2026-08-28). The
runtime matrix requiring an interactive two-speaker recording session is
listed at the end as pending manual QA.

## Build / lint / test gate (task 7.2)

| Check | Command | Result |
| --- | --- | --- |
| Rust compile | `cargo check` | clean (only pre-existing warnings) |
| Rust lints (my modules) | `cargo clippy` | `word_alignment/*` clean; 0 warnings |
| Rust tests | `cargo test --lib` | 335 passed, 1 failed, 8 ignored |
| Real-model integration | `cargo test aligns_real_model -- --ignored` | passed |
| Frontend typecheck | `npx tsc --noEmit` | 0 new errors (only pre-existing `bun:test` in 2 test files) |
| Frontend build | `npm run build` | ✓ Compiled successfully, lint + types valid, 11 static pages |

**The 1 failing test** is `audio::playback_monitor::tests::test_get_output_device`
("Should be able to get output device"). It is pre-existing and
environment-dependent: it requires an active audio output device, which is
absent in this headless session. `playback_monitor.rs` is not in this change's
diff. CI does not run `cargo clippy`, and the one clippy **error**
(`audio::capture::backend_config.rs:55` `inherent_to_string_shadow_display`) is
pre-existing (commit `17a39fd`, not in this diff).

## Spike / engine assumptions (tasks 3.1, 4.1)

- **Model**: `NewComer00/wav2vec2-xlsr-multilingual-56-ONNX` (56-language
  wav2vec2-large CTC), default file `onnx/model_fp16.onnx` (652 MB).
- **Raw-waveform input CONFIRMED against the real download**: the engine feeds
  a 1 s 16 kHz f32 span as `input_values` and receives a `[frames × 9913]`
  `logits` matrix. Measured frames ≈ 50 for 1 s (20 ms hop = conv strides
  5·2⁶), and each row's linear probabilities sum to ≈ 1.0 (log-softmax).
- **Dtype finding (corrected the spike note)**: despite the `_fp16` filename
  the graph emits **f32** logits; the engine accepts f32 first, f16 fallback.
- **Download URLs** HEAD-verified: config.json 2280 B, vocab.json 146914 B,
  model_fp16.onnx 651760843 B — all HTTP 200, sizes match the catalog.

## Unit/integration coverage by task

| Task | Test(s) | Status |
| --- | --- | --- |
| 1.1 token forwarding | `recording_commands::tests::transcript_update_tokens_survive_event_payload_roundtrip`, `...without_tokens_stays_none` | pass |
| 1.2 json round-trip | `recording_saver::tests::test_tokens_roundtrip_through_transcripts_json` | pass |
| 2.1 Parakeet tokens | `parakeet_engine::model::tests::{word_tokens_merge_subwords_and_chain_boundaries, same_frame_words_share_start_and_last_word_ends_one_frame_after_start, single_word_and_empty_inputs, timestamps_are_never_decreasing}` | pass |
| 3.2 catalog readiness | `word_alignment::catalog::tests::{missing_dir_reports_missing, fake_complete_dir_reports_available_and_short_file_corrupted, list_models_reports_per_model_status}` | pass |
| 4.1 posteriors | `word_alignment::engine::tests::aligns_real_model_one_second_clip` (ignored; run manually) + `pool_size_matches_diarization_formula` | pass |
| 4.2 Viterbi | `word_alignment::viterbi::tests::{plan_inserts_word_and_repeat_blanks, out_of_alphabet_word_returns_none_not_error, viterbi_recovers_known_word_boundaries, viterbi_single_word_short_span, viterbi_fails_when_sequence_longer_than_frames}` | pass |
| 4.3 span source | `word_alignment::refine::tests::{file_span_source_extracts_requested_window, memory_span_source_maps_channels}` | pass |
| 5.1 refine entry | `word_alignment::refine::tests::{disabled_settings_is_noop, missing_model_is_noop, already_refined_segments_are_skipped, per_segment_span_failure_keeps_baseline}` | pass |
| 5.2 queue | `word_alignment::queue::tests::{push_pop_preserves_order, overflow_drops_oldest_not_newest, push_after_close_is_refused, drain_completes_after_close}` | pass |

## Fallback / degradation matrix (task 7.1) — logic verified by tests

| Condition | Behavior | Evidence |
| --- | --- | --- |
| Alignment OFF | no inference; ASR tokens used verbatim | `disabled_settings_is_noop`, worker skips queue creation when `is_enabled()==false` |
| Model MISSING | repair/queue no-op; recording unaffected | `missing_model_is_noop`; `settings.engine()` returns None |
| Queue OVERFLOW | oldest block dropped, keeps ASR tokens, worker never blocks | `overflow_drops_oldest_not_newest` |
| Per-segment FAIL (span/alphabet/timeout) | that segment keeps baseline; run continues | `per_segment_span_failure_keeps_baseline`, `out_of_alphabet_word_returns_none_not_error` |
| Already `refined` | skipped by all three call sites | `already_refined_segments_are_skipped`; consumer + `refine_tokens_with_source` guard |
| Partials | never queued | worker gates push on `!is_partial` |
| Tokenless legacy rows | segment-level overlap only, no split | `assign_tokens_to_speakers` unchanged; loops require `tokens.len() >= 2` |

## Pending manual runtime QA (requires interactive recording)

These steps need a live app session and cannot be exercised headlessly:

1. **Live Whisper/Parakeet recording** with alignment ON: confirm
   `transcripts.json` segments gain `refined: true` tokens during recording and
   `SELECT tokens FROM transcripts` is a non-NULL JSON array after save.
2. **Two-speaker stop**: confirm a cross-speaker chunk splits into separate
   rows with per-block boundaries (stop-time repair path).
3. **Offline re-diarization** of a meeting recorded with alignment OFF: confirm
   split rows use aligned boundaries (offline repair path logs "refined N
   offline transcript row(s)").
4. **Settings UI**: toggle persistence across restart; model download shows
   Missing → progress → Ready; cancel mid-download removes partials → Missing.
5. **Queue drain on stop** with a deep backlog: finalize completes with
   refined-or-baseline tokens and never hangs (120 s overall bound).
