//! Speaker attribution over a diarized timeline (05 D5): the one
//! implementation both the batch pass and the live stop-time path use to map
//! a transcript span onto a speaker.

use log::info;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Runtime};

use super::super::telemetry::emit_progress;
use super::super::batch::guard::DIARIZATION_CANCELLED;
use super::super::DiarizationSegment;

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

fn find_best_speaker(segments: &[DiarizationSegment], t_start: f32, t_end: f32) -> Option<i32> {
    let mut best_speaker: Option<i32> = None;
    let mut best_overlap: f32 = 0.0;

    for seg in segments {
        let overlap_start = t_start.max(seg.start);
        let overlap_end = t_end.min(seg.end);
        if overlap_start < overlap_end {
            let overlap = overlap_end - overlap_start;
            if overlap > best_overlap {
                best_overlap = overlap;
                best_speaker = Some(seg.speaker);
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
    let first = segments[0].speaker;
    if segments.iter().all(|s| s.speaker == first) {
        return Some(first);
    }

    const MAX_GAP_SECS: f32 = 30.0;
    let mut nearest: Option<(f32, i32)> = None;
    for seg in segments {
        let gap = if seg.end < t_start {
            t_start - seg.end
        } else if seg.start > t_end {
            seg.start - t_end
        } else {
            0.0
        };
        if gap <= MAX_GAP_SECS && nearest.map_or(true, |(g, _)| gap < g) {
            nearest = Some((gap, seg.speaker));
        }
    }
    nearest.map(|(_, spk)| spk)
}
