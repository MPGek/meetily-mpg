//! Post-ASR word-level CTC forced alignment (word-level-diarization-alignment).
//!
//! Refines per-word start/end timestamps of transcript segments against the
//! segment's own audio using a wav2vec2 CTC model + constrained Viterbi.
//! Runs live at block finalization (bounded queue, `queue.rs`) and as a repair
//! path in offline re-diarization and stop-time finalize (`refine.rs`). Every
//! failure mode degrades silently to the ASR-provided tokens.
//!
//! ## Spike (task 3.1): chosen ONNX export
//!
//! `NewComer00/wav2vec2-xlsr-multilingual-56-ONNX` (Apache-2.0) — an
//! optimum/transformers.js export of `voidful/wav2vec2-xlsr-multilingual-56`
//! (wav2vec2-large XLS-R fine-tuned CTC on 56 Common Voice languages).
//!
//! * Input: `input_values` — raw 16 kHz mono f32 waveform. The CNN feature
//!   extractor is inside the graph, so no companion front-end export is
//!   needed; only the external `do_normalize` step (zero mean / unit variance
//!   over the span, per `preprocessor_config.json`) is applied in Rust.
//! * Output: `logits` — `[1, frames, 9913]` **f32** CTC posteriors (verified
//!   against the real download: despite the `model_fp16.onnx` filename the
//!   graph casts back to f32 at the output; the engine accepts either dtype);
//!   frame hop is 320 samples (20 ms) from the conv strides (5·2⁶).
//! * Blank id is read from `vocab.json` (`[PAD]`), not hardcoded.
//! * Default file: `onnx/model_fp16.onnx` (652 MB). fp16 runs on the ORT
//!   1.23 CPU EP; the repo's q4/bnb4 variants need `com.microsoft` contrib
//!   ops, which conflict with the process-wide ort init used by VAD/Parakeet/
//!   diarization — rejected. English-only int8 exports exist
//!   (`Xenova/wav2vec2-large-xlsr-53-english`) but would silently disable
//!   alignment for non-Latin transcripts — rejected for the default entry.
//!
//! ## Module structure
//!
//! * `catalog` — model catalog, readiness resolution under
//!   `app_data_dir/models/alignment/<id>/`.
//! * `download` — HF multi-file download with progress, Range resume, cancel.
//! * `engine` — ONNX session pool + CTC posterior inference over a span.
//! * `viterbi` — character-sequence builder + constrained Viterbi aligner.
//! * `refine` — `refine_segment_tokens` + `AudioSpanSource` (repair paths).
//! * `queue` — bounded live-alignment queue + re-emit consumer.
//! * `commands` — Tauri commands for settings UI.

pub mod catalog;
pub mod commands;
pub mod download;
pub mod engine;
pub mod queue;
pub mod refine;
pub mod settings;
pub mod viterbi;

pub use catalog::{alignment_models_root, model_dir, AlignmentModelInfo, AlignmentModelStatus};
pub use engine::AlignmentEngine;
pub use refine::{refine_segment_tokens, AlignmentSettings, AudioSpanSource};
pub use settings::{current as current_settings, is_enabled as is_alignment_enabled};
