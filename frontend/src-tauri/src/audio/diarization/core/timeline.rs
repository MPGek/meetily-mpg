//! Speaker attribution over a diarized timeline (05 D5): the one
//! implementation both the batch pass and the live stop-time path use to map
//! a transcript span onto a speaker.

use log::info;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Runtime};

use super::super::telemetry::emit_progress;
use super::super::batch::guard::DIARIZATION_CANCELLED;
use super::super::DiarizationSegment;
use super::cluster::SpeakerSegment;
use crate::audio::token_assignment::{
    assign_tokens_to_speakers, SpeakerTurn as TokenTurn, Token,
};

pub(crate) fn compute_speaker_matches<R: Runtime>(
    mic_segments: &[DiarizationSegment],
    sys_segments: &[DiarizationSegment],
    is_stereo: bool,
    transcripts: &[crate::database::models::Transcript],
    app: &AppHandle<R>,
    meeting_id: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut updates: Vec<(String, String)> = Vec::new();
    let total = transcripts.len();
    let mut skipped_no_match = 0usize;

    for (idx, transcript) in transcripts.iter().enumerate() {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let progress = 70 + ((idx as f32 / total as f32) * 25.0) as u32;
        if idx % 10 == 0 {
            emit_progress(
                app,
                meeting_id,
                "matching",
                progress,
                &format!("Matching segment {}/{}", idx + 1, total),
            );
        }

        let t_start = transcript.audio_start_time.unwrap_or(0.0) as f32;
        let t_end = transcript.audio_end_time.unwrap_or(0.0) as f32;

        // Stereo: system-source transcripts match system-channel segments
        // (SPEAKER_NN); all others match mic-channel segments (MIC_SPEAKER_NN).
        // Mono fallback: everything matches the single run as remote (SPEAKER_NN).
        let (segments, prefix) = if is_stereo {
            if transcript.source_device.as_deref() == Some("System") {
                (sys_segments, "SPEAKER")
            } else {
                (mic_segments, "MIC_SPEAKER")
            }
        } else {
            (mic_segments, "SPEAKER")
        };

        let speaker_id = match find_best_speaker(segments, t_start, t_end) {
            Some(spk) => format!("{}_{:02}", prefix, spk),
            None => {
                skipped_no_match += 1;
                continue;
            }
        };

        updates.push((transcript.id.clone(), speaker_id));
    }

    info!(
        "Speaker matching: {} total, {} matched, {} no-match skipped",
        total,
        updates.len(),
        skipped_no_match,
    );

    emit_progress(app, meeting_id, "matching", 95, "Speaker matching complete");
    Ok(updates)
}

/// A labeled span on one channel's timeline. The batch pass and a live session
/// carry different segment types with differently typed speaker labels, so the
/// attribution rule below is written once against this seam (05 D5).
pub(crate) trait LabeledSpan {
    type Label: Copy + PartialEq;

    fn start(&self) -> f32;
    fn end(&self) -> f32;
    fn label(&self) -> Self::Label;
    /// The label as the `i32` the token-assignment layer speaks.
    fn label_index(&self) -> i32;
}

impl LabeledSpan for DiarizationSegment {
    type Label = i32;

    fn start(&self) -> f32 {
        self.start
    }
    fn end(&self) -> f32 {
        self.end
    }
    fn label(&self) -> i32 {
        self.speaker
    }
    fn label_index(&self) -> i32 {
        self.speaker
    }
}

impl LabeledSpan for SpeakerSegment {
    type Label = usize;

    fn start(&self) -> f32 {
        self.start
    }
    fn end(&self) -> f32 {
        self.end
    }
    fn label(&self) -> usize {
        self.speaker
    }
    fn label_index(&self) -> i32 {
        self.speaker as i32
    }
}

/// One token block of a transcript row: the tokens that belong to a single
/// speaker, with the block's own text.
pub(crate) struct TokenBlock {
    pub(crate) speaker: i32,
    pub(crate) start: f32,
    pub(crate) end: f32,
    pub(crate) start_idx: usize,
    pub(crate) end_idx: usize,
    /// Joined and trimmed text of the block's tokens. Empty when they carry no
    /// visible text; each caller decides what that means for its output.
    pub(crate) text: String,
}

/// Split one transcript row's tokens across the speakers of its channel. This
/// is the only call site of `assign_tokens_to_speakers` (05 task 2.4): the
/// offline row split, the stop-time split and the live display split all come
/// through here and render their own output shape from the returned blocks.
pub(crate) fn split_tokens_by_speaker<S: LabeledSpan>(
    tokens: &[Token],
    spans: &[S],
) -> Vec<TokenBlock> {
    if tokens.is_empty() || spans.is_empty() {
        return Vec::new();
    }
    let turns: Vec<TokenTurn> = spans
        .iter()
        .map(|s| TokenTurn {
            start: s.start(),
            end: s.end(),
            speaker: s.label_index(),
        })
        .collect();

    assign_tokens_to_speakers(tokens, &turns)
        .blocks
        .into_iter()
        .map(|block| {
            let slice = &tokens[block.start_idx..=block.end_idx];
            let parts: Vec<&str> = slice.iter().map(|tok| tok.text.as_str()).collect();
            // Tokenizers that carry their own spacing join cleanly; those that
            // do not would collapse into whitespace, so fall back to spaces.
            let joined = parts.concat();
            let text = if joined.trim().is_empty() {
                parts.join(" ")
            } else {
                joined
            };
            TokenBlock {
                speaker: block.speaker,
                start: block.start,
                end: block.end,
                start_idx: block.start_idx,
                end_idx: block.end_idx,
                text: text.trim().to_string(),
            }
        })
        .collect()
}

pub(crate) fn find_best_speaker<S: LabeledSpan>(
    segments: &[S],
    t_start: f32,
    t_end: f32,
) -> Option<S::Label> {
    let mut best_speaker: Option<S::Label> = None;
    let mut best_overlap: f32 = 0.0;

    for seg in segments {
        let overlap_start = t_start.max(seg.start());
        let overlap_end = t_end.min(seg.end());
        if overlap_start < overlap_end {
            let overlap = overlap_end - overlap_start;
            if overlap > best_overlap {
                best_overlap = overlap;
                best_speaker = Some(seg.label());
            }
        }
    }

    if best_speaker.is_some() {
        return best_speaker;
    }
    if segments.is_empty() {
        return None;
    }

    // Gap-fill: short utterances the segmenter missed get the nearest speaker.
    // A single-speaker channel can be filled unconditionally; a multi-speaker
    // channel is bounded so we never assign across long silences.
    let first = segments[0].label();
    if segments.iter().all(|s| s.label() == first) {
        return Some(first);
    }

    const MAX_GAP_SECS: f32 = 30.0;
    let mut nearest: Option<(f32, S::Label)> = None;
    for seg in segments {
        let gap = if seg.end() < t_start {
            t_start - seg.end()
        } else if seg.start() > t_end {
            seg.start() - t_end
        } else {
            0.0
        };
        if gap <= MAX_GAP_SECS && nearest.map_or(true, |(g, _)| gap < g) {
            nearest = Some((gap, seg.label()));
        }
    }
    nearest.map(|(_, spk)| spk)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same four-case fixture through both label types: the batch
    /// `DiarizationSegment` (i32 labels) and the live `SpeakerSegment` (usize).
    /// Task 2.1 deleted the live copy of this rule, so this is what guarantees
    /// the surviving one still decides identically for either caller.
    #[test]
    fn shared_attribution_agrees_for_both_label_types() {
        // Two speakers, a gap between 12.0 and 50.0, nothing after 51.0.
        let batch = [
            DiarizationSegment {
                start: 0.0,
                end: 10.0,
                speaker: 0,
            },
            DiarizationSegment {
                start: 10.5,
                end: 12.0,
                speaker: 1,
            },
            DiarizationSegment {
                start: 50.0,
                end: 51.0,
                speaker: 1,
            },
        ];
        let live = [
            SpeakerSegment {
                start: 0.0,
                end: 10.0,
                speaker: 0,
            },
            SpeakerSegment {
                start: 10.5,
                end: 12.0,
                speaker: 1,
            },
            SpeakerSegment {
                start: 50.0,
                end: 51.0,
                speaker: 1,
            },
        ];

        // 1. Overlap wins, and the largest overlap wins: 9.0 s with speaker 0
        //    against 1.5 s with speaker 1.
        assert_eq!(find_best_speaker(&batch, 1.0, 11.0), Some(0));
        assert_eq!(find_best_speaker(&live, 1.0, 11.0), Some(0));

        // 2. No overlap, multi-speaker channel, inside the 30 s bound: the
        //    nearest segment's speaker (speaker 1 ends at 12.0, 8 s away).
        assert_eq!(find_best_speaker(&batch, 20.0, 21.0), Some(1));
        assert_eq!(find_best_speaker(&live, 20.0, 21.0), Some(1));

        // 3. Past the 30 s bound with no segment on the far side: unassigned.
        //    (Dropping the 50 s segment leaves the nearest end at 12.0, so a
        //    span at 50 s is 38 s away -- outside MAX_GAP_SECS.)
        let early_batch = &batch[..2];
        let early_live = &live[..2];
        assert_eq!(find_best_speaker(early_batch, 50.0, 51.0), None);
        assert_eq!(find_best_speaker(early_live, 50.0, 51.0), None);
        // Just inside the bound, the same fixture assigns the nearest speaker.
        assert_eq!(find_best_speaker(early_batch, 41.0, 42.0), Some(1));
        assert_eq!(find_best_speaker(early_live, 41.0, 42.0), Some(1));

        // 4. Single-speaker channel: filled unconditionally, even 10 minutes
        //    past the last segment, and an empty channel stays unassigned.
        let one_batch = [batch[0].clone()];
        let one_live = [live[0].clone()];
        assert_eq!(find_best_speaker(&one_batch, 600.0, 601.0), Some(0));
        assert_eq!(find_best_speaker(&one_live, 600.0, 601.0), Some(0));
        assert_eq!(
            find_best_speaker(&[] as &[DiarizationSegment], 1.0, 2.0),
            None
        );
        assert_eq!(find_best_speaker(&[] as &[SpeakerSegment], 1.0, 2.0), None);
    }
}
