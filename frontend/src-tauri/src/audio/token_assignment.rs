//! Token-level speaker assignment (diarization-accuracy-upgrade 4.x).
//!
//! When Whisper provides token timestamps, diarization refines per-segment
//! ownership to token granularity: each token is attributed to the speaker of
//! its covering turn (max overlap), falling back to nearest turn ≤30s. A
//! segment whose tokens span a speaker change is split into N rows (one per
//! contiguous speaker block, each boundary validated by ≥2 contiguous tokens
//! of the new speaker to avoid drift-induced splits), with contiguous
//! gap-free timestamps.

use serde::{Deserialize, Serialize};

/// One Whisper token with start/end timestamps (seconds, recording-relative).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Token {
    pub text: String,
    /// Start time in seconds from recording start.
    pub start: f32,
    /// End time in seconds from recording start.
    pub end: f32,
    /// True when `start`/`end` came from CTC forced alignment rather than the
    /// transcription engine's own timestamps (word-level-diarization-alignment
    /// D4). Refinement call sites skip tokens already flagged refined.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refined: bool,
}

/// A diarization turn (speaker-active interval) used for assignment.
#[derive(Debug, Clone)]
pub struct SpeakerTurn {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
}

/// Contiguous speaker block after token assignment (≥2 tokens per block except
/// possibly single-token noise that was merged).
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerBlock {
    pub speaker: i32,
    /// Inclusive token indices for this block.
    pub start_idx: usize,
    pub end_idx: usize,
    /// Time span (contiguous, gap-free with neighbors).
    pub start: f32,
    pub end: f32,
}

/// Result of token attribution: per-token speaker plus split decision (N-way).
#[derive(Debug, Clone)]
pub struct TokenAssignment {
    /// Token index -> speaker id (None when no turn within 30s or ambiguous).
    pub per_token: Vec<Option<i32>>,
    /// Whether any boundary validated (blocks.len() > 1).
    pub should_split: bool,
    /// Index of first token of the second block (for legacy single-split callers).
    pub split_token_idx: Option<usize>,
    /// N-way blocks (1 block = no split). Each block has ≥2 tokens except
    /// merged noise blocks.
    pub blocks: Vec<SpeakerBlock>,
}

/// Attribute each token to a speaker by max turn overlap, with nearest-turn
/// fallback ≤30s. Group into N blocks with ≥2 contiguous tokens per boundary.
pub fn assign_tokens_to_speakers(tokens: &[Token], turns: &[SpeakerTurn]) -> TokenAssignment {
    if tokens.is_empty() {
        return TokenAssignment {
            per_token: Vec::new(),
            should_split: false,
            split_token_idx: None,
            blocks: Vec::new(),
        };
    }
    if turns.is_empty() {
        return TokenAssignment {
            per_token: vec![None; tokens.len()],
            should_split: false,
            split_token_idx: None,
            blocks: Vec::new(),
        };
    }

    let per_token: Vec<Option<i32>> = tokens
        .iter()
        .map(|tok| best_speaker_for_token(tok, turns))
        .collect();
    let blocks = group_into_blocks(&per_token, tokens);
    let should_split = blocks.len() > 1;
    let split_token_idx = if should_split {
        Some(blocks[1].start_idx)
    } else {
        None
    };

    TokenAssignment {
        per_token,
        should_split,
        split_token_idx,
        blocks,
    }
}

/// Group per_token into N blocks validated by ≥2 contiguous tokens per new speaker.
fn group_into_blocks(per_token: &[Option<i32>], tokens: &[Token]) -> Vec<SpeakerBlock> {
    if per_token.is_empty() || tokens.is_empty() {
        return Vec::new();
    }
    // If no valid speaker at all, no blocks.
    if per_token.iter().all(|v| v.is_none()) {
        return Vec::new();
    }

    let mut blocks: Vec<SpeakerBlock> = Vec::new();
    let mut block_start = 0usize;
    let mut block_speaker = per_token[0];

    // Helper to flush block [block_start, end_idx]
    let flush = |blocks: &mut Vec<SpeakerBlock>, s: Option<i32>, start: usize, end: usize| {
        if let Some(spk) = s {
            let start_t = tokens[start].start;
            let end_t = tokens[end].end;
            // Ensure contiguous gap-free: start of block beyond first is previous block's end
            let (adj_start, adj_end) = if blocks.is_empty() {
                (start_t, end_t)
            } else {
                // Gap-free: previous block ended at tokens[prev_end].end which == tokens[start].start
                // if tokens are contiguous; we preserve that. Use tokens[start].start directly
                // but ensure no overlap gap: if tokens are contiguous, this is already gap-free.
                (tokens[start].start, end_t)
            };
            blocks.push(SpeakerBlock {
                speaker: spk,
                start_idx: start,
                end_idx: end,
                start: adj_start,
                end: adj_end,
            });
        }
    };

    let mut i = 1usize;
    while i < per_token.len() {
        let cur = per_token[i];
        if cur != block_speaker {
            // Potential boundary at i: check run length of cur speaker
            if let Some(spk) = cur {
                // Count forward run of this speaker
                let mut run = 1usize;
                for j in (i + 1)..per_token.len() {
                    if per_token[j] == Some(spk) {
                        run += 1;
                    } else {
                        break;
                    }
                }
                if run >= 2 {
                    // Validated boundary: close previous block up to i-1
                    flush(&mut blocks, block_speaker, block_start, i - 1);
                    block_start = i;
                    block_speaker = cur;
                    // Skip the validated run quickly? Keep normal walk
                    i += 1;
                    continue;
                } else {
                    // Spurious single-token excursion: ignore (treat as noise, do not split)
                    // We keep current block speaker, but mark this token as noise? For grouping,
                    // we effectively ignore this token's speaker and keep it in current block
                    // by not splitting. To avoid orphan None blocks, we just skip.
                    // To preserve time contiguity, we keep block open and will include this token
                    // in the current block's span (its time still counts). But per_token noise would
                    // create hole; instead we treat noise tokens as belonging to current block for
                    // block counting (they are drift). We do not change block_speaker.
                    i += 1;
                    continue;
                }
            } else {
                // cur is None (no turn within 30s) – do not split on None, keep block open
                i += 1;
                continue;
            }
        } else {
            i += 1;
        }
    }
    // Flush final block
    flush(&mut blocks, block_speaker, block_start, per_token.len() - 1);

    // Post-process: if we had noise singletons merged, the last block may have absorbed them.
    // Ensure blocks' timestamps are contiguous gap-free by stitching boundaries at token edges.
    for idx in 1..blocks.len() {
        let boundary = tokens[blocks[idx].start_idx].start;
        blocks[idx - 1].end = boundary;
        blocks[idx].start = boundary;
    }

    blocks
}

fn best_speaker_for_token(tok: &Token, turns: &[SpeakerTurn]) -> Option<i32> {
    let t_start = tok.start;
    let t_end = tok.end;
    let mut best: Option<i32> = None;
    let mut best_overlap: f32 = 0.0;
    for turn in turns {
        let ov_s = t_start.max(turn.start);
        let ov_e = t_end.min(turn.end);
        if ov_s < ov_e {
            let ov = ov_e - ov_s;
            if ov > best_overlap {
                best_overlap = ov;
                best = Some(turn.speaker);
            }
        }
    }
    if best.is_some() {
        return best;
    }
    if turns.is_empty() {
        return None;
    }
    let first = turns[0].speaker;
    if turns.iter().all(|t| t.speaker == first) {
        return Some(first);
    }
    const MAX_GAP: f32 = 30.0;
    let mut nearest: Option<(f32, i32)> = None;
    for turn in turns {
        let gap = if turn.end < t_start {
            t_start - turn.end
        } else if turn.start > t_end {
            turn.start - t_end
        } else {
            0.0
        };
        if gap <= MAX_GAP && nearest.map_or(true, |(g, _)| gap < g) {
            nearest = Some((gap, turn.speaker));
        }
    }
    nearest.map(|(_, s)| s)
}

/// Legacy helper: split into two spans at split_idx (still used for callers expecting pair).
pub fn split_span_at_token(tokens: &[Token], split_idx: usize) -> Option<((f32, f32), (f32, f32))> {
    if split_idx == 0 || split_idx >= tokens.len() {
        return None;
    }
    let first = &tokens[0];
    let boundary = &tokens[split_idx];
    let last = tokens.last()?;
    let a_start = first.start;
    let a_end = boundary.start;
    let b_start = boundary.start;
    let b_end = last.end;
    if a_end <= a_start || b_end <= b_start {
        return None;
    }
    Some(((a_start, a_end), (b_start, b_end)))
}

/// N-way: return per-block (start,end,text) spans derived from blocks.
pub fn split_into_blocks(
    tokens: &[Token],
    assignment: &TokenAssignment,
) -> Vec<(f32, f32, String)> {
    let mut out = Vec::new();
    for b in &assignment.blocks {
        let text = tokens[b.start_idx..=b.end_idx]
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        // Preserve spacing as tokens include their own spacing? Join with empty and rely on token text containing spaces.
        // Fallback: join with space if tokens don't contain leading space.
        let text = if text.contains(' ') {
            text
        } else {
            tokens[b.start_idx..=b.end_idx]
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>()
                .join(" ")
        };
        out.push((b.start, b.end, text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, s: f32, e: f32) -> Token {
        Token {
            text: text.to_string(),
            start: s,
            end: e,
            refined: false,
        }
    }
    fn turn(s: f32, e: f32, spk: i32) -> SpeakerTurn {
        SpeakerTurn {
            start: s,
            end: e,
            speaker: spk,
        }
    }

    #[test]
    fn single_speaker_no_split() {
        let tokens = vec![
            tok("hello", 0.0, 0.5),
            tok(" world", 0.5, 1.0),
            tok(" test", 1.0, 1.5),
        ];
        let turns = vec![turn(0.0, 10.0, 0)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.per_token, vec![Some(0), Some(0), Some(0)]);
        assert!(!a.should_split);
        assert_eq!(a.blocks.len(), 1);
        assert_eq!(a.blocks[0].speaker, 0);
    }

    #[test]
    fn cross_speaker_requires_two_contiguous() {
        let tokens = vec![
            tok("a", 0.0, 1.0),
            tok("b", 1.0, 2.0),
            tok("c", 2.0, 3.0),
            tok("d", 3.0, 4.0),
        ];
        let turns = vec![turn(0.0, 2.0, 0), turn(2.0, 10.0, 1)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.per_token, vec![Some(0), Some(0), Some(1), Some(1)]);
        assert!(a.should_split);
        assert_eq!(a.split_token_idx, Some(2));
        assert_eq!(a.blocks.len(), 2);
        assert_eq!(a.blocks[0].speaker, 0);
        assert_eq!(a.blocks[1].speaker, 1);
        assert_eq!(a.blocks[0].end, 2.0);
        assert_eq!(a.blocks[1].start, 2.0);
    }

    #[test]
    fn single_token_of_new_speaker_no_split() {
        let tokens = vec![
            tok("a", 0.0, 1.0),
            tok("b", 1.0, 2.0),
            tok("c", 2.0, 3.0),
            tok("d", 3.0, 4.0),
        ];
        let turns = vec![turn(0.0, 2.0, 0), turn(2.0, 3.0, 1), turn(3.0, 10.0, 0)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.per_token[2], Some(1));
        assert!(!a.should_split, "single-token run must not split");
        assert_eq!(a.blocks.len(), 1);
    }

    #[test]
    fn three_blocks_a_b_a() {
        // 0,0,1,1,0,0 -> 3 blocks A-B-A
        let tokens = vec![
            tok("a", 0.0, 1.0),
            tok("a2", 1.0, 2.0),
            tok("b", 2.0, 3.0),
            tok("b2", 3.0, 4.0),
            tok("a3", 4.0, 5.0),
            tok("a4", 5.0, 6.0),
        ];
        let turns = vec![turn(0.0, 2.0, 0), turn(2.0, 4.0, 1), turn(4.0, 6.0, 0)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(
            a.per_token,
            vec![Some(0), Some(0), Some(1), Some(1), Some(0), Some(0)]
        );
        assert_eq!(a.blocks.len(), 3);
        assert_eq!(a.blocks[0].speaker, 0);
        assert_eq!(a.blocks[1].speaker, 1);
        assert_eq!(a.blocks[2].speaker, 0);
        // Gap-free
        assert_eq!(a.blocks[0].end, a.blocks[1].start);
        assert_eq!(a.blocks[1].end, a.blocks[2].start);
        assert_eq!(a.blocks[0].start, 0.0);
        assert_eq!(a.blocks[2].end, 6.0);
    }

    #[test]
    fn three_speakers_a_b_c() {
        let tokens = vec![
            tok("a", 0.0, 1.0),
            tok("a2", 1.0, 2.0),
            tok("b", 2.0, 3.0),
            tok("b2", 3.0, 4.0),
            tok("c", 4.0, 5.0),
            tok("c2", 5.0, 6.0),
        ];
        let turns = vec![turn(0.0, 2.0, 0), turn(2.0, 4.0, 1), turn(4.0, 6.0, 2)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.blocks.len(), 3);
        assert_eq!(a.blocks[2].speaker, 2);
    }

    #[test]
    fn fallback_nearest_within_30s() {
        let tokens = vec![tok("hi", 100.0, 101.0)];
        let turns = vec![turn(0.0, 10.0, 0), turn(120.0, 130.0, 1)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.per_token[0], Some(1));
    }

    #[test]
    fn no_turns_gives_none() {
        let tokens = vec![tok("hi", 0.0, 1.0)];
        let a = assign_tokens_to_speakers(&tokens, &[]);
        assert_eq!(a.per_token[0], None);
        assert!(a.blocks.is_empty());
    }

    #[test]
    fn split_span() {
        let tokens = vec![tok("a", 0.0, 1.0), tok("b", 1.0, 2.0), tok("c", 2.0, 3.0)];
        let s = split_span_at_token(&tokens, 2).unwrap();
        assert_eq!(s.0, (0.0, 2.0));
        assert_eq!(s.1, (2.0, 3.0));
    }

    #[test]
    fn split_into_blocks_produces_contiguous() {
        let tokens = vec![
            tok("a", 0.0, 1.0),
            tok("b", 1.0, 2.0),
            tok("c", 2.0, 3.0),
            tok("d", 3.0, 4.0),
        ];
        let turns = vec![turn(0.0, 2.0, 0), turn(2.0, 4.0, 1)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        let spans = split_into_blocks(&tokens, &a);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, 0.0);
        assert_eq!(spans[0].1, 2.0);
        assert_eq!(spans[1].0, 2.0);
        assert_eq!(spans[1].1, 4.0);
    }

    #[test]
    fn overlapping_turns_larger_covered_duration_wins() {
        // Pipeline-v2 overlap output: two turns share [1.0, 2.0]. A token fully
        // inside the overlap picks the turn covering more of the token.
        let tokens = vec![tok("x", 1.2, 1.8), tok("y", 2.5, 3.0)];
        let turns = vec![turn(0.0, 2.0, 0), turn(1.0, 4.0, 1)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        // Token x: overlap 0.6 with both -> first max wins (tie keeps first);
        // token y: only turn 1 covers it.
        assert_eq!(a.per_token[1], Some(1));
        // Shifted token covered more by turn 1 than turn 0.
        let tokens = vec![tok("z", 1.8, 2.5)];
        let a = assign_tokens_to_speakers(&tokens, &turns);
        assert_eq!(a.per_token[0], Some(1));
    }
}
