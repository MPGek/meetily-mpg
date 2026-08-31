//! Character-sequence builder + CTC blank-aware constrained Viterbi
//! (task 4.2, design D2).
//!
//! Builds a label sequence from a segment's word list — character labels per
//! word with forced `Blank` markers between words (and between repeated
//! characters, per CTC's no-repeat rule) — then runs a monotonic interval
//! Viterbi over a `[frames × vocab]` log-posterior matrix to recover the
//! frame range of every word. Fully testable with synthetic posteriors.

use ndarray::Array2;
use std::collections::HashMap;

/// One CTC label position: a character id or a forced blank (word boundary /
/// repeat separator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    Char(usize),
    Blank,
}

/// A prepared per-segment alignment plan.
#[derive(Debug, Clone)]
pub struct LabelPlan {
    pub labels: Vec<Label>,
    /// `word_ranges[i]` = (first, last) inclusive indices into `labels` of
    /// word `i`'s character labels; `(a, b)` with `a > b` marks a
    /// punctuation-only word (zero-length span).
    pub word_ranges: Vec<(usize, usize)>,
    /// CTC blank id in the posterior matrix (set by the engine).
    pub blank: usize,
}

/// Characters kept at word edges (stripped before alphabet lookup).
fn is_strippable_edge(c: char) -> bool {
    !(c.is_alphanumeric() || c.is_whitespace())
}

/// Build the label sequence for one segment's words.
///
/// Returns `None` when any word contains an out-of-alphabet character after
/// edge-punctuation stripping — the caller keeps the pre-alignment tokens
/// (per-segment fallback, never an error).
pub fn build_plan(
    words: &[String],
    id_of: &HashMap<char, usize>,
) -> Option<LabelPlan> {
    let mut labels: Vec<Label> = Vec::new();
    let mut word_ranges: Vec<(usize, usize)> = Vec::new();

    for (w_idx, raw) in words.iter().enumerate() {
        let word = raw.trim();
        // Strip edge punctuation/symbols (kept in the transcript text; only
        // excluded from the acoustic label sequence).
        let stripped: String = word
            .trim_matches(|c: char| is_strippable_edge(c))
            .chars()
            .collect();

        if w_idx > 0 {
            labels.push(Label::Blank); // word-boundary marker
        }

        if stripped.is_empty() {
            // Punctuation-only word: zero-length span at the current boundary.
            let at = labels.len().saturating_sub(1);
            word_ranges.push((at + 1, at)); // empty range (first > last)
            continue;
        }

        let first = labels.len();
        let mut prev: Option<char> = None;
        for c in stripped.chars() {
            let lower = c.to_lowercase().next().unwrap_or(c);
            let id = *id_of.get(&lower).or_else(|| id_of.get(&c))?;
            // CTC forbids the same label twice in a row: separate repeats by a blank.
            if prev == Some(lower) {
                labels.push(Label::Blank);
            }
            labels.push(Label::Char(id));
            prev = Some(lower);
        }
        word_ranges.push((first, labels.len() - 1));
    }

    if labels.is_empty() {
        return None;
    }
    Some(LabelPlan {
        labels,
        word_ranges,
        blank: 0,
    })
}

/// Interval Viterbi over the plan: returns the inclusive frame range of every
/// label, or `None` when the span is too short to host the sequence.
///
/// `logprobs` is `[frames × vocab]` of log P(token | frame).
pub fn viterbi_ranges(logprobs: &Array2<f32>, plan: &LabelPlan) -> Option<Vec<(usize, usize)>> {
    let frames = logprobs.nrows();
    let n = plan.labels.len();
    if frames == 0 || n == 0 || n > frames {
        return None;
    }

    let emit = |i: usize, t: usize| -> f32 {
        match plan.labels[i] {
            Label::Char(id) => logprobs[(t, id)],
            Label::Blank => logprobs[(t, plan.blank)],
        }
    };

    // dp[i][t]: best score aligning labels[0..=i] with label i ending at t.
    // ptr[i][t]: true => label i also occupies t-1 (repeat); false => label i
    // starts at t (advance from label i-1, or a fresh start for label 0).
    let mut dp = vec![vec![f32::NEG_INFINITY; frames]; n];
    let mut ptr = vec![vec![false; frames]; n];

    let mut acc = 0.0f32;
    for t in 0..frames {
        let e = emit(0, t);
        if t == 0 {
            acc = e;
        } else {
            let extended = acc + e;
            ptr[0][t] = extended >= e;
            acc = extended.max(e);
        }
        dp[0][t] = acc;
    }

    for i in 1..n {
        let mut best_prev = f32::NEG_INFINITY; // max dp[i-1][u] for u <= t-1
        for t in 0..frames {
            let from_advance = if t > 0 { best_prev } else { f32::NEG_INFINITY };
            let from_repeat = if t > 0 { dp[i][t - 1] } else { f32::NEG_INFINITY };
            let prev_best = if from_repeat >= from_advance {
                ptr[i][t] = true;
                from_repeat
            } else {
                ptr[i][t] = false;
                from_advance
            };
            if prev_best.is_finite() {
                dp[i][t] = prev_best + emit(i, t);
            }
            if dp[i - 1][t] > best_prev {
                best_prev = dp[i - 1][t];
            }
        }
    }

    // The sequence ends at any frame; trailing frames stay unassigned (blank).
    // `>=` tie-breaks toward the later end so the last word absorbs trailing
    // frames of its own emission.
    let mut end_t = 0usize;
    let mut best = f32::NEG_INFINITY;
    for (t, &score) in dp[n - 1].iter().enumerate() {
        if score >= best {
            best = score;
            end_t = t;
        }
    }
    if !best.is_finite() {
        return None;
    }

    // Backtrack label occupancy ranges.
    let mut ranges = vec![(0usize, 0usize); n];
    let mut i = n - 1;
    let mut t = end_t;
    loop {
        ranges[i].1 = t;
        while t > 0 && ptr[i][t] {
            t -= 1;
        }
        ranges[i].0 = t;
        if i == 0 {
            break;
        }
        // Label i started at t via advance: label i-1 ended at the best u < t.
        let target = dp[i - 1][..t].iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        let mut u = t - 1;
        while u > 0 && dp[i - 1][u] != target {
            u -= 1;
        }
        t = u;
        i -= 1;
    }
    Some(ranges)
}

/// Align a segment's words: per-word (start, end) in seconds relative to the
/// recording, given the span's start time and frame duration.
/// Returns `None` on any structural failure (caller keeps ASR tokens).
pub fn align_word_spans(
    logprobs: &Array2<f32>,
    plan: &LabelPlan,
    frame_secs: f32,
    span_start: f32,
) -> Option<Vec<(f32, f32)>> {
    let ranges = viterbi_ranges(logprobs, plan)?;
    let frames = logprobs.nrows();
    let mut out = Vec::with_capacity(plan.word_ranges.len());
    for &(first, last) in &plan.word_ranges {
        if first > last {
            // Punctuation-only word: zero-length at the previous word's end.
            let at = out.last().map(|&(_, e)| e).unwrap_or(span_start);
            out.push((at, at));
            continue;
        }
        let start_frame = ranges[first].0;
        let end_frame = ranges[last].1.min(frames - 1);
        let start = span_start + start_frame as f32 * frame_secs;
        let end = span_start + (end_frame as f32 + 1.0) * frame_secs;
        out.push((start, end));
    }
    // Enforce non-decreasing timestamps across words.
    for w in 1..out.len() {
        if out[w].0 < out[w - 1].0 {
            out[w].0 = out[w - 1].0;
        }
        if out[w].1 < out[w].0 {
            out[w].1 = out[w].0;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_map(spec: &[(&str, usize)]) -> HashMap<char, usize> {
        spec.iter()
            .map(|(c, i)| (c.chars().next().unwrap(), *i))
            .collect()
    }

    /// Build a synthetic posterior matrix where each frame strongly prefers
    /// `path[t]` (label id), with small mass elsewhere.
    fn synth(frames: usize, vocab: usize, path: &[usize], strong: f32) -> Array2<f32> {
        let mut m = Array2::<f32>::from_elem((frames, vocab), -strong / (vocab as f32));
        for (t, &id) in path.iter().enumerate() {
            m[(t, id)] = 0.0; // log-prob max
            for v in 0..vocab {
                if v != id {
                    m[(t, v)] = -strong;
                }
            }
        }
        m
    }

    fn plan_with_blank(labels: Vec<Label>, word_ranges: Vec<(usize, usize)>, blank: usize) -> LabelPlan {
        LabelPlan {
            labels,
            word_ranges,
            blank,
        }
    }

    #[test]
    fn plan_inserts_word_and_repeat_blanks() {
        let ids = id_map(&[("a", 1), ("b", 2)]);
        let words = vec!["ab".to_string(), "ba".to_string()];
        let plan = build_plan(&words, &ids).unwrap();
        // a b <blank word> b a  (no identical adjacency, one inter-word blank)
        assert_eq!(
            plan.labels,
            vec![
                Label::Char(1),
                Label::Char(2),
                Label::Blank,
                Label::Char(2),
                Label::Char(1),
            ]
        );
        assert_eq!(plan.word_ranges, vec![(0, 1), (3, 4)]);

        let words = vec!["aa".to_string()];
        let plan = build_plan(&words, &ids).unwrap();
        assert_eq!(
            plan.labels,
            vec![Label::Char(1), Label::Blank, Label::Char(1)]
        );
    }

    #[test]
    fn out_of_alphabet_word_returns_none_not_error() {
        let ids = id_map(&[("a", 1)]);
        // '日' not in alphabet, not strippable punctuation.
        let words = vec!["a日".to_string()];
        assert!(build_plan(&words, &ids).is_none());
        // Edge punctuation is stripped, not a failure.
        let words = vec!["a.".to_string()];
        assert!(build_plan(&words, &ids).is_some());
        // Interior symbol is a failure.
        let words = vec!["a日b".to_string()];
        assert!(build_plan(&words, &ids).is_none());
    }

    #[test]
    fn viterbi_recovers_known_word_boundaries() {
        // vocab: 0=blank, 1='a', 2='b'. Words: "ab" then "a".
        // True frame layout: a(0) b(1) blank(2) a(3) a(4)
        let m = synth(5, 3, &[1, 2, 0, 1, 1], 10.0);
        let plan = plan_with_blank(
            vec![Label::Char(1), Label::Char(2), Label::Blank, Label::Char(1)],
            vec![(0, 1), (3, 3)],
            0,
        );
        let spans = align_word_spans(&m, &plan, 0.02, 10.0).unwrap();
        assert_eq!(spans.len(), 2);
        // word "ab": frames 0..=1 -> [10.00, 10.04]
        assert!((spans[0].0 - 10.00).abs() < 1e-4);
        assert!((spans[0].1 - 10.04).abs() < 1e-4);
        // word "a": frames 3..=4 -> [10.06, 10.10]
        assert!((spans[1].0 - 10.06).abs() < 1e-4);
        assert!((spans[1].1 - 10.10).abs() < 1e-4);
    }

    #[test]
    fn viterbi_single_word_short_span() {
        let m = synth(3, 3, &[0, 1, 1], 10.0);
        let plan = plan_with_blank(vec![Label::Char(1)], vec![(0, 0)], 0);
        let spans = align_word_spans(&m, &plan, 0.02, 0.0).unwrap();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].0 >= 0.0 && spans[0].1 <= 0.06 + 1e-4);
    }

    #[test]
    fn viterbi_fails_when_sequence_longer_than_frames() {
        let m = synth(2, 3, &[1, 1], 10.0);
        let plan = plan_with_blank(
            vec![Label::Char(1), Label::Blank, Label::Char(2)],
            vec![(0, 0), (2, 2)],
            0,
        );
        assert!(viterbi_ranges(&m, &plan).is_none());
    }
}
