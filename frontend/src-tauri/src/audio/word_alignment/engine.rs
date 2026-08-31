//! ONNX CTC posterior engine + constrained-Viterbi refinement (task 4.1/4.2,
//! design D2).
//!
//! Loads the catalogued wav2vec2 CTC export with the existing `ort` runtime,
//! runs posteriors over a 16 kHz f32 span, and refines word timestamps via
//! `viterbi`. Sessions are pooled lazily up to the diarization-style bound
//! `min(8, ceil(0.75 × cores))` (weights are re-loaded per session, so the
//! pool grows only with actual concurrency — the live consumer and repair
//! paths use one session at a time).

use super::viterbi::{align_word_spans, build_plan};
use anyhow::{anyhow, Result};
use ndarray::Array2;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

/// Samples per encoder frame for wav2vec2-large (conv strides 5·2⁶ = 320 →
/// 20 ms at 16 kHz). Recomputed from config.json at load time.
const DEFAULT_HOP_SAMPLES: usize = 320;

/// Pool bound mirroring the diarization embedder (`fixed_pool_size`).
pub fn fixed_pool_size() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (((cores as f64) * 0.75).ceil() as usize).clamp(1, 8)
}

/// CTC posterior engine over one alignment model.
pub struct AlignmentEngine {
    model_path: PathBuf,
    id_of: Arc<HashMap<char, usize>>,
    blank_id: usize,
    hop_samples: usize,
    frame_secs: f32,
    sessions: Arc<(Mutex<Vec<Session>>, Condvar)>,
    live_sessions: Arc<Mutex<usize>>,
    pool_cap: usize,
}

impl AlignmentEngine {
    /// Load model metadata (vocab, blank id, frame hop) and prepare the pool.
    /// ONNX sessions are created lazily on first use.
    pub fn load(model_dir: &Path) -> Result<Self> {
        let model_path = model_dir.join("model_fp16.onnx");
        if !model_path.exists() {
            return Err(anyhow!("alignment model file missing: {}", model_path.display()));
        }

        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
            model_dir.join("config.json"),
        )?)
        .map_err(|e| anyhow!("alignment config.json: {}", e))?;
        let vocab: HashMap<String, usize> = serde_json::from_str(&std::fs::read_to_string(
            model_dir.join("vocab.json"),
        )?)
        .map_err(|e| anyhow!("alignment vocab.json: {}", e))?;

        let blank_id = config
            .get("pad_token_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("alignment config.json missing pad_token_id"))?
            as usize;

        let hop_samples = config
            .get("conv_stride")
            .and_then(|v| v.as_array())
            .map(|strides| {
                strides
                    .iter()
                    .filter_map(|s| s.as_u64())
                    .product::<u64>() as usize
            })
            .filter(|h| *h > 0)
            .unwrap_or(DEFAULT_HOP_SAMPLES);

        // Single-character, non-special vocab entries form the alphabet.
        let mut id_of: HashMap<char, usize> = HashMap::new();
        for (token, &id) in &vocab {
            let mut chars = token.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if token.starts_with('<') || token.starts_with('[') {
                    continue; // special token
                }
                // Lowercase keys win; keep original-case lookups working.
                for key in c.to_lowercase() {
                    id_of.entry(key).or_insert(id);
                }
                id_of.entry(c).or_insert(id);
            }
        }
        if id_of.is_empty() {
            return Err(anyhow!("alignment vocab.json has no single-char entries"));
        }

        let pool_cap = fixed_pool_size();
        log::info!(
            "AlignmentEngine loaded: blank={}, hop={} samples ({:.1} ms), alphabet={} chars, pool_cap={}",
            blank_id,
            hop_samples,
            hop_samples as f64 / 16.0,
            id_of.len(),
            pool_cap
        );

        Ok(Self {
            model_path,
            id_of: Arc::new(id_of),
            blank_id,
            hop_samples,
            frame_secs: hop_samples as f32 / 16000.0,
            sessions: Arc::new((Mutex::new(Vec::new()), Condvar::new())),
            live_sessions: Arc::new(Mutex::new(0)),
            pool_cap,
        })
    }

    fn create_session(&self) -> Result<Session> {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let intra = (cores / self.pool_cap).max(1);
        Session::builder()
            .map_err(|e| anyhow!("ORT builder: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("ORT optimization: {}", e))?
            .with_intra_threads(intra)
            .map_err(|e| anyhow!("ORT threads: {}", e))?
            .commit_from_file(&self.model_path)
            .map_err(|e| anyhow!("ORT load {}: {}", self.model_path.display(), e))
    }

    /// Borrow a pooled session (blocks when the pool is saturated).
    fn acquire_session(&self) -> Result<Session> {
        let (lock, cvar) = &*self.sessions;
        let mut pool = lock.lock().unwrap();
        loop {
            if let Some(session) = pool.pop() {
                return Ok(session);
            }
            let mut live = self.live_sessions.lock().unwrap();
            if *live < self.pool_cap {
                *live += 1;
                drop(live);
                drop(pool);
                match self.create_session() {
                    Ok(s) => return Ok(s),
                    Err(e) => {
                        *self.live_sessions.lock().unwrap() -= 1;
                        return Err(e);
                    }
                }
            }
            pool = cvar.wait(pool).unwrap();
        }
    }

    fn release_session(&self, session: Session) {
        let (lock, cvar) = &*self.sessions;
        lock.lock().unwrap().push(session);
        cvar.notify_one();
    }

    /// Run CTC inference over a 16 kHz mono f32 span, returning the
    /// `[frames × vocab]` matrix of log-probabilities.
    pub fn log_posteriors(&self, samples: &[f32]) -> Result<Array2<f32>> {
        if samples.len() < self.hop_samples {
            return Err(anyhow!(
                "span too short for alignment: {} samples < {} (1 frame)",
                samples.len(),
                self.hop_samples
            ));
        }

        // External feature extraction for wav2vec2: per-utterance
        // zero-mean/unit-variance normalization (preprocessor_config.json
        // `do_normalize`, eps 1e-7).
        let n = samples.len() as f64;
        let mean = samples.iter().map(|&x| x as f64).sum::<f64>() / n;
        let var = samples.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / n;
        let inv_std = 1.0 / (var + 1e-7).sqrt();
        let normalized: Vec<f32> = samples
            .iter()
            .map(|&x| ((x as f64 - mean) * inv_std) as f32)
            .collect();

        let mut session = self.acquire_session()?;
        let run = Self::run_posteriors(&mut session, normalized);
        let out = match run {
            Ok(out) => out,
            Err(e) => {
                self.release_session(session);
                return Err(e);
            }
        };
        self.release_session(session);
        Ok(out)
    }

    /// Run one inference on a borrowed session and return the owned
    /// `[frames × vocab]` log-posterior matrix (no borrows outlive the call).
    fn run_posteriors(session: &mut Session, normalized: Vec<f32>) -> Result<Array2<f32>> {
        let input = ndarray::ArrayD::<f32>::from_shape_vec(
            ndarray::IxDyn(&[1, normalized.len()]),
            normalized,
        )
        .map_err(|e| anyhow!("input shape: {}", e))?;
        let outputs = session
            .run(ort::inputs![
                "input_values" => ort::value::TensorRef::from_array_view(input.view())
                    .map_err(|e| anyhow!("input tensor: {}", e))?
            ])
            .map_err(|e| anyhow!("ORT inference: {}", e))?;
        let logits = outputs
            .get("logits")
            .ok_or_else(|| anyhow!("alignment logits output missing"))?;

        // The fp16 export emits f32 `logits` (the graph casts back to f32 at the
        // output); accept either dtype so other catalog entries also work.
        if let Ok(view) = logits.try_extract_array::<f32>() {
            let shape = view.shape();
            if shape.len() != 3 {
                return Err(anyhow!("unexpected logits shape {:?}", shape));
            }
            let (frames, vocab) = (shape[1], shape[2]);
            let flat = view.as_slice().ok_or_else(|| anyhow!("non-contiguous"))?;
            return Ok(Self::log_softmax_matrix(flat, frames, vocab));
        }
        let view = logits
            .try_extract_array::<half::f16>()
            .map_err(|e| anyhow!("logits extract: {}", e))?;
        let shape = view.shape();
        if shape.len() != 3 {
            return Err(anyhow!("unexpected logits shape {:?}", shape));
        }
        let (frames, vocab) = (shape[1], shape[2]);
        let flat: Vec<f32> = view.iter().map(|v| v.to_f32()).collect();
        Ok(Self::log_softmax_matrix(&flat, frames, vocab))
    }

    /// Row-wise log-softmax over a `[frames × vocab]` flat f32 buffer.
    fn log_softmax_matrix(flat: &[f32], frames: usize, vocab: usize) -> Array2<f32> {
        let mut out = Array2::<f32>::zeros((frames, vocab));
        for t in 0..frames {
            let row = &flat[t * vocab..(t + 1) * vocab];
            let max = row.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
            let sum: f32 = row.iter().map(|&v| (v - max).exp()).sum();
            let denom = max + sum.ln();
            for (v, &val) in row.iter().enumerate() {
                out[(t, v)] = val - denom;
            }
        }
        out
    }

    /// Refine `tokens` in place against `samples` (the segment's own audio,
    /// starting at `span_start` recording-relative). Returns true when every
    /// token was updated and flagged refined. Any structural failure leaves
    /// the tokens untouched (per-segment fallback).
    pub fn align_tokens(
        &self,
        tokens: &mut [crate::audio::token_assignment::Token],
        samples: &[f32],
        span_start: f32,
        span_end: f32,
    ) -> Result<bool> {
        if tokens.is_empty() {
            return Ok(false);
        }
        let words: Vec<String> = tokens.iter().map(|t| t.text.clone()).collect();
        let mut plan = match build_plan(&words, &self.id_of) {
            Some(p) => p,
            None => return Ok(false), // out-of-alphabet -> keep ASR tokens
        };
        plan.blank = self.blank_id;

        let logprobs = self.log_posteriors(samples)?;
        let Some(spans) = align_word_spans(&logprobs, &plan, self.frame_secs, span_start) else {
            return Ok(false);
        };
        if spans.len() != tokens.len() {
            return Ok(false);
        }

        // Bound refined times within the supplied span, non-decreasing.
        let mut prev_start = span_start;
        for (token, &(start, end)) in tokens.iter_mut().zip(spans.iter()) {
            let s = start.clamp(span_start, span_end).max(prev_start);
            let e = end.clamp(s, span_end);
            token.start = s;
            token.end = e;
            token.refined = true;
            prev_start = s;
        }
        Ok(true)
    }
}

/// Align with a wall-clock timeout: run on a helper thread and abandon (not
/// cancel) on expiry — the helper finishes into a dead channel. A timed-out
/// alignment keeps ASR tokens (per-segment fallback).
pub fn align_tokens_with_timeout(
    engine: Arc<AlignmentEngine>,
    tokens: &mut [crate::audio::token_assignment::Token],
    samples: Vec<f32>,
    span_start: f32,
    span_end: f32,
    timeout: std::time::Duration,
) -> bool {
    let (tx, rx) = mpsc::channel();
    let mut working = tokens.to_vec();
    std::thread::spawn(move || {
        let ok = engine
            .align_tokens(&mut working, &samples, span_start, span_end)
            .unwrap_or(false);
        let _ = tx.send((ok, working));
    });
    match rx.recv_timeout(timeout) {
        Ok((true, updated)) => {
            tokens.clone_from_slice(&updated);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_size_matches_diarization_formula() {
        let cores = std::thread::available_parallelism().unwrap().get();
        let expected = (((cores as f64) * 0.75).ceil() as usize).min(8).max(1);
        assert_eq!(fixed_pool_size(), expected);
    }

    /// Task 4.1 integration: run a 1 s clip through the real model and assert
    /// the posterior matrix is `[frames × vocab]` with sane probabilities.
    /// Ignored by default (needs the ~650 MB downloaded model); run with
    /// `cargo test -- --ignored aligns_real_model` once the model is present.
    #[test]
    #[ignore = "requires the downloaded wav2vec2-xlsr-56 alignment model"]
    fn aligns_real_model_one_second_clip() {
        use crate::audio::word_alignment::catalog::{model_dir, resolve_status, spec_by_id, AlignmentModelStatus};

        let models_root = dirs::config_dir()
            .expect("config dir")
            .join("com.meetily.ai")
            .join("models");
        let spec = spec_by_id(super::super::catalog::DEFAULT_ALIGNMENT_MODEL_ID).unwrap();
        let dir = model_dir(&models_root, spec.id);
        if resolve_status(&dir, spec) != AlignmentModelStatus::Available {
            eprintln!("alignment model not downloaded at {}; skipping", dir.display());
            return;
        }

        let engine = AlignmentEngine::load(&dir).expect("load engine");
        // 1 s of 16 kHz sine-ish audio.
        let samples: Vec<f32> = (0..16000)
            .map(|i| (i as f32 * 0.05).sin() * 0.3)
            .collect();
        let post = engine.log_posteriors(&samples).expect("posteriors");
        let (frames, vocab) = (post.nrows(), post.ncols());
        assert!(frames >= 40 && frames <= 60, "frames={frames} (expect ~50 for 1s)");
        assert!(vocab > 100, "vocab={vocab}");
        // Log-probs: each row sums (in linear space) to ~1.
        for t in (0..frames).step_by(frames / 5).take(5) {
            let row = post.row(t);
            let sum: f32 = row.iter().map(|&lp| lp.exp()).sum();
            assert!((sum - 1.0).abs() < 0.05, "row {t} prob sum {sum}");
        }
    }
}
