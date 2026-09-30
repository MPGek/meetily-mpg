//! Turn assembly: the pure global stage that maps clustered unit labels onto
//! per-segment speaker turns, splits at overlap spans, applies the min-speech
//! filter and bridges same-speaker gaps.

use super::cluster::SpeakerSegment;
use super::segment::ChunkRecord;
use super::super::{
    ClusteredEmbedding, DiarizationConfig, DiarizationSegment, SpeakerTurn, TimeRange,
};

/// Pure global stage (unit-testable): per-chunk Hungarian local→global mapping
/// over the clustered unit labels, per-segment primary turns (majority label
/// of the segment's dense windows) with splitting at mapped overlap spans,
/// overlap-aware two-speaker resegmentation, min-speech filter, and gap-fill.
pub(crate) fn assemble_channel_turns(
    chunks: &[ChunkRecord],
    unit_embeddings: &[Vec<f32>],
    unit_labels: &[usize],
    resegmenter: &polyvoice::resegmentation::OverlapResegmenter,
    config: &DiarizationConfig,
) -> Result<(Vec<DiarizationSegment>, Vec<ClusteredEmbedding>), String> {
    use polyvoice::clusterer::{build_cooccurrence, hungarian_local_to_global};
    use polyvoice::resegmentation::{
        compute_centroids, OverlapRegionInput, ResegmentInputs, Resegmenter as _,
    };

    let mut global_unit = 0usize;
    let mut primary_turns: Vec<SpeakerTurn> = Vec::new();
    let mut clustered: Vec<ClusteredEmbedding> = Vec::new();
    let mut overlap_inputs: Vec<OverlapRegionInput> = Vec::new();

    // Overlap spans whose both local speakers mapped to global clusters
    // (global coords + region primary). Primary turns of the region-primary
    // speaker are split at these spans below: the resegmenter re-emits the
    // primary+secondary pair over the span (vendored aggregator semantics),
    // so leaving the primary covering it would double-cover the span.
    let mut mapped_spans: Vec<(TimeRange, polyvoice::types::SpeakerId)> = Vec::new();

    for chunk in chunks {
        let n = chunk.units.len();
        let chunk_labels = &unit_labels[global_unit..global_unit + n];
        global_unit += n;
        if n == 0 {
            continue;
        }

        let local_idx: Vec<u8> = chunk.units.iter().map(|u| u.local_idx).collect();
        let durations: Vec<f64> = chunk
            .units
            .iter()
            .map(|u| u.time.end - u.time.start)
            .collect();
        let cooc = build_cooccurrence(&local_idx, chunk_labels, &durations);
        let cannot_link: Vec<(u8, u8)> =
            chunk.overlaps.iter().map(|(_, lo, hi)| (*lo, *hi)).collect();
        let local_to_global = hungarian_local_to_global(&cooc, &cannot_link);

        // Per source segment (D5 contract: per-segment semantics for turns,
        // caches, and enrollment are preserved): label = duration-weighted
        // majority of the segment's unit cluster labels; vector =
        // L2-normalized mean of its unit embeddings. Raw dense windows are
        // embedding units only — emitting one turn per window would tile long
        // segments with overlapping duplicates that only gap-fill can re-glue
        // (breaks gap=0 and double-covers time when labels alternate).
        // Powerset local indices are concurrent-speaker slots reused across
        // time, so segment identity is resolved through the segment's own
        // unit majority, not the per-chunk local→global map (the map serves
        // the overlap regions only).
        let mut by_segment: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (ui, unit) in chunk.units.iter().enumerate() {
            by_segment.entry(unit.segment_idx).or_default().push(ui);
        }
        for (segment_idx, unit_ids) in by_segment {
            let seg = &chunk.primary[segment_idx];
            let mut label_secs: std::collections::BTreeMap<usize, f64> =
                std::collections::BTreeMap::new();
            for &ui in &unit_ids {
                *label_secs.entry(chunk_labels[ui]).or_insert(0.0) +=
                    chunk.units[ui].time.end - chunk.units[ui].time.start;
            }
            let Some((&majority, _)) = label_secs
                .iter()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            else {
                continue;
            };
            primary_turns.push(SpeakerTurn {
                speaker: polyvoice::types::SpeakerId(majority as u32),
                time: TimeRange {
                    start: seg.time.start + chunk.start_secs as f64,
                    end: seg.time.end + chunk.start_secs as f64,
                },
                text: None,
                stable: true,
            });
            let mut mean = vec![0.0f32; unit_embeddings[unit_ids[0]].len()];
            for &ui in &unit_ids {
                for (i, x) in unit_embeddings[ui].iter().enumerate() {
                    mean[i] += x;
                }
            }
            let n = unit_ids.len().max(1) as f32;
            for m in mean.iter_mut() {
                *m /= n;
            }
            polyvoice::utils::l2_normalize(&mut mean);
            clustered.push(ClusteredEmbedding {
                speaker: majority as i32,
                embedding: mean,
                duration_secs: (seg.time.end - seg.time.start).max(0.0) as f32,
                start_secs: Some((seg.time.start + chunk.start_secs as f64) as f32),
                end_secs: Some((seg.time.end + chunk.start_secs as f64) as f32),
            });
        }

        // Overlap regions → two-speaker assignment.
        for (time, lo, hi) in &chunk.overlaps {
            let g_lo = local_to_global.get(lo).copied();
            let g_hi = local_to_global.get(hi).copied();
            let global_time = TimeRange {
                start: time.start + chunk.start_secs as f64,
                end: time.end + chunk.start_secs as f64,
            };
            if let (Some(a), Some(b)) = (g_lo, g_hi) {
                overlap_inputs.push(OverlapRegionInput {
                    time: global_time,
                    primary_speaker: a,
                    secondary_speaker: Some(b),
                    embedding: Vec::new(),
                });
                mapped_spans.push((global_time, a));
                continue;
            }
            let mixed = chunk
                .mixed_overlaps
                .iter()
                .find(|(t, _)| (t.start - time.start).abs() < 1e-6 && (t.end - time.end).abs() < 1e-6)
                .map(|(_, e)| e.clone());
            let Some(mixed) = mixed else { continue };
            let primary = g_lo.or(g_hi).unwrap_or_else(|| {
                let mid = (global_time.start + global_time.end) / 2.0;
                let tmid = |t: &SpeakerTurn| (t.time.start + t.time.end) / 2.0;
                primary_turns
                    .iter()
                    .min_by(|a, b| (tmid(a) - mid).abs().total_cmp(&(tmid(b) - mid).abs()))
                    .map(|t| t.speaker)
                    .unwrap_or(polyvoice::types::SpeakerId(0))
            });
            overlap_inputs.push(OverlapRegionInput {
                time: global_time,
                primary_speaker: primary,
                secondary_speaker: None,
                embedding: mixed,
            });
        }
    }

    // Split region-primary turns at mapped overlap spans (vendored
    // aggregator semantics: primaries must not cover overlap spans because
    // the resegmenter re-emits the primary+secondary pair there). Only the
    // region-primary speaker's turns are split — other speakers' coverage is
    // never destroyed. Sub-min-speech slivers are dropped by the filter below.
    let mut split_turns: Vec<SpeakerTurn> = Vec::with_capacity(
        primary_turns.len() + 2 * mapped_spans.len(),
    );
    for turn in &primary_turns {
        let mut pieces = vec![turn.clone()];
        for (span, primary_spk) in &mapped_spans {
            if turn.speaker != *primary_spk {
                continue;
            }
            let mut next: Vec<SpeakerTurn> = Vec::with_capacity(pieces.len() + 1);
            for piece in pieces {
                next.extend(subtract_span(&piece, span));
            }
            pieces = next;
        }
        split_turns.extend(pieces);
    }
    let primary_turns = split_turns;

    let centroids = compute_centroids(unit_embeddings, unit_labels);

    let mut all_turns = if centroids.len() >= 2 && !overlap_inputs.is_empty() {
        resegmenter
            .resegment(ResegmentInputs {
                primary_turns: &primary_turns,
                speaker_centroids: &centroids,
                overlap_regions: &overlap_inputs,
            })
            .map_err(|e| format!("Overlap resegmentation failed: {}", e))?
    } else {
        let mut base = primary_turns.clone();
        // No resegmentation (single cluster or no overlap spans): overlap
        // spans have no primary coverage (primaries exclude overlap-flagged
        // segments), so emit them with their resolved primary speaker instead
        // of leaving holes (which score as Miss).
        for region in &overlap_inputs {
            base.push(SpeakerTurn {
                speaker: region.primary_speaker,
                time: region.time,
                text: None,
                stable: true,
            });
        }
        base
    };
    all_turns.sort_by(|a, b| a.time.start.total_cmp(&b.time.start));

    let min_secs = config.min_speech_secs as f64;
    all_turns.retain(|t| t.time.duration() >= min_secs);

    let all_turns = if config.gap_merge_secs > 0.0 {
        gap_fill_turns(all_turns, config.gap_merge_secs)
    } else {
        all_turns
    };

    let segments = all_turns
        .into_iter()
        .map(|t| DiarizationSegment {
            start: t.time.start as f32,
            end: t.time.end as f32,
            speaker: t.speaker.0 as i32,
        })
        .collect();
    Ok((segments, clustered))
}

/// Subtract an overlap span from a primary turn, returning the surviving
/// piece(s). Zero-length pieces are dropped; disjoint inputs return the turn
/// unchanged. Used to keep primaries off mapped overlap spans that the
/// resegmenter re-emits as primary+secondary pairs.
fn subtract_span(turn: &SpeakerTurn, span: &TimeRange) -> Vec<SpeakerTurn> {
    if span.end <= turn.time.start || span.start >= turn.time.end {
        return vec![turn.clone()];
    }
    let mut out = Vec::with_capacity(2);
    if span.start > turn.time.start {
        out.push(SpeakerTurn {
            speaker: turn.speaker,
            time: TimeRange {
                start: turn.time.start,
                end: span.start,
            },
            text: None,
            stable: true,
        });
    }
    if span.end < turn.time.end {
        out.push(SpeakerTurn {
            speaker: turn.speaker,
            time: TimeRange {
                start: span.end,
                end: turn.time.end,
            },
            text: None,
            stable: true,
        });
    }
    out
}

/// Pipeline gap-fill: bridge consecutive same-speaker turns separated by at
/// most `max_gap_secs` (v2 `merge_segments` semantics; replaces the old
/// app-side post-clustering merge pass). Overlapping same-speaker pairs merge
/// (negative gap); cross-speaker boundaries and different-speaker overlaps are
/// left untouched.
fn gap_fill_turns(turns: Vec<SpeakerTurn>, max_gap_secs: f32) -> Vec<SpeakerTurn> {
    let segments: Vec<polyvoice::types::Segment> = turns
        .into_iter()
        .map(|t| polyvoice::types::Segment {
            time: t.time,
            speaker: Some(t.speaker),
            confidence: None,
        })
        .collect();
    polyvoice::utils::merge_segments(segments, max_gap_secs as f64)
        .into_iter()
        .filter_map(|s| {
            s.speaker.map(|spk| SpeakerTurn {
                speaker: spk,
                time: s.time,
                text: None,
                stable: true,
            })
        })
        .collect()
}

/// Bridge same-speaker gaps in a channel timeline assembled outside the batch
/// turn stage (05b D1): the stop-time refinement labels one segment per
/// buffered embedding window, which tiles a single speaker's speech into as
/// many segments as it had windows. Runs through the same `merge_segments`
/// call `gap_fill_turns` uses, so a live session's refined timeline bridges
/// gaps exactly as the batch path does.
pub(crate) fn merge_same_speaker_segments(
    mut segments: Vec<SpeakerSegment>,
    max_gap_secs: f32,
) -> Vec<SpeakerSegment> {
    if segments.len() < 2 {
        return segments;
    }
    segments.sort_by(|a, b| a.start.total_cmp(&b.start));
    if max_gap_secs <= 0.0 {
        return segments;
    }
    let turns: Vec<SpeakerTurn> = segments
        .iter()
        .map(|seg| SpeakerTurn {
            speaker: polyvoice::types::SpeakerId(seg.speaker as u32),
            time: TimeRange {
                start: seg.start as f64,
                end: seg.end as f64,
            },
            text: None,
            stable: true,
        })
        .collect();
    gap_fill_turns(turns, max_gap_secs)
        .into_iter()
        .map(|turn| SpeakerSegment {
            start: turn.time.start as f32,
            end: turn.time.end as f32,
            speaker: turn.speaker.0 as usize,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{raw_seg, test_unit};
    use super::super::segment::DenseUnit;
    use super::*;

    fn turn(start: f64, end: f64, speaker: u32) -> polyvoice::types::SpeakerTurn {
        polyvoice::types::SpeakerTurn {
            speaker: polyvoice::types::SpeakerId(speaker),
            time: polyvoice::types::TimeRange { start, end },
            text: None,
            stable: true,
        }
    }

    fn turn_span(t: &polyvoice::types::SpeakerTurn) -> (f32, f32, i32) {
        (t.time.start as f32, t.time.end as f32, t.speaker.0 as i32)
    }

    #[test]
    fn gap_fill_bridges_short_same_speaker_gap() {
        let input = vec![turn(0.0, 1.0, 0), turn(1.2, 2.5, 0), turn(5.0, 6.0, 0)];
        let out = gap_fill_turns(input, 0.3);
        assert_eq!(out.len(), 2, "gap 0.2s bridged, gap 2.5s kept");
        assert_eq!(turn_span(&out[0]), (0.0, 2.5, 0));
        assert_eq!(turn_span(&out[1]), (5.0, 6.0, 0));
    }

    #[test]
    fn gap_fill_at_window_boundary_is_bridged() {
        let input = vec![turn(0.0, 1.0, 0), turn(1.3, 2.0, 0)];
        let out = gap_fill_turns(input, 0.3);
        assert_eq!(out.len(), 1, "gap equal to the window merges (<=)");
    }

    #[test]
    fn gap_fill_preserves_cross_speaker_boundaries() {
        // A different-speaker segment between two same-speaker segments must
        // block bridging even when both gaps fit the window.
        let input = vec![turn(0.0, 1.0, 0), turn(1.1, 2.0, 1), turn(2.1, 3.0, 0)];
        let out = gap_fill_turns(input, 0.3);
        assert_eq!(out.len(), 3);
        assert_eq!(
            out.iter().map(|s| s.speaker.0).collect::<Vec<_>>(),
            vec![0, 1, 0]
        );
    }

    #[test]
    fn gap_fill_merges_same_speaker_overlap_and_keeps_cross_speaker_overlap() {
        // Pipeline gap-fill (v2 merge_segments): negative gaps (overlaps)
        // merge only within the same speaker; distinct speakers sharing time
        // are preserved (the overlap-aware output contract).
        let same = vec![turn(0.0, 2.0, 0), turn(1.5, 3.0, 0)];
        let out = gap_fill_turns(same, 0.3);
        assert_eq!(out.len(), 1);
        assert_eq!(turn_span(&out[0]), (0.0, 3.0, 0));
        let diff = vec![turn(0.0, 2.0, 0), turn(1.5, 3.0, 1)];
        let out = gap_fill_turns(diff, 0.3);
        assert_eq!(out.len(), 2, "cross-speaker overlap untouched");
    }

    #[test]
    fn gap_fill_zero_window_is_noop() {
        // 0 disables: assemble_channel_turns skips gap_fill_turns entirely.
        let input = vec![turn(0.0, 1.0, 0), turn(1.05, 2.0, 0), turn(2.02, 3.0, 0)];
        let expected: Vec<(f32, f32, i32)> =
            input.iter().map(turn_span).collect();
        let config = DiarizationConfig {
            gap_merge_secs: 0.0,
            ..DiarizationConfig::default()
        };
        let chunk = ChunkRecord {
            start_secs: 0.0,
            primary: vec![
                raw_seg(0.0, 1.0, 0),
                raw_seg(1.05, 2.0, 0),
                raw_seg(2.02, 3.0, 0),
            ],
            overlaps: Vec::new(),
            units: vec![
                test_unit(0.0, 1.0, 0, 0),
                test_unit(1.05, 2.0, 0, 1),
                test_unit(2.02, 3.0, 0, 2),
            ],
            mixed_overlaps: Vec::new(),
        };
        let (segments, _) = assemble_channel_turns(
            &[chunk],
            &[vec![1.0, 0.0], vec![1.0, 0.0], vec![1.0, 0.0]],
            &[0, 0, 0],
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        let got: Vec<(f32, f32, i32)> = segments.iter().map(|s| (s.start, s.end, s.speaker)).collect();
        assert_eq!(got, expected, "gap 0 must not merge anything");
    }

    #[test]
    fn dense_windows_emit_single_segment_turn_without_gap_fill() {
        // One 12 s primary segment split into 4 dense windows (all one
        // cluster): the output must be a single [0,12] turn even with
        // gap-merge disabled — windows are embedding units, not turns.
        let chunk = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 12.0, 0)],
            overlaps: Vec::new(),
            units: (0..4)
                .map(|i| DenseUnit {
                    time: polyvoice::types::TimeRange {
                        start: i as f64 * 2.5,
                        end: i as f64 * 2.5 + 5.0,
                    },
                    local_idx: 0,
                    segment_idx: 0,
                    embedding: vec![1.0, 0.0],
                })
                .collect(),
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> = chunk.units.iter().map(|u| u.embedding.clone()).collect();
        let config = DiarizationConfig {
            gap_merge_secs: 0.0,
            ..DiarizationConfig::default()
        };
        let (segments, _) = assemble_channel_turns(
            &[chunk],
            &embeddings,
            &[0, 0, 0, 0],
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        assert_eq!(segments.len(), 1, "dense windows must not tile the output");
        assert_eq!((segments[0].start, segments[0].end), (0.0, 12.0));
    }

    #[test]
    fn single_cluster_overlap_span_stays_covered_without_resegmentation() {
        // One cluster (centroids < 2 → resegmenter fast path): the overlap
        // span must still be emitted with its primary speaker — otherwise it
        // scores as Miss.
        let chunk = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 10.0, 0)],
            overlaps: vec![(
                polyvoice::types::TimeRange { start: 4.0, end: 6.0 },
                0,
                1,
            )],
            units: vec![DenseUnit {
                time: polyvoice::types::TimeRange { start: 0.0, end: 10.0 },
                local_idx: 0,
                segment_idx: 0,
                embedding: vec![1.0, 0.0],
            }],
            mixed_overlaps: vec![(
                polyvoice::types::TimeRange { start: 4.0, end: 6.0 },
                vec![0.9, 0.1],
            )],
        };
        let embeddings: Vec<Vec<f32>> = chunk.units.iter().map(|u| u.embedding.clone()).collect();
        let config = DiarizationConfig {
            gap_merge_secs: 0.0,
            ..DiarizationConfig::default()
        };
        let (segments, _) = assemble_channel_turns(
            &[chunk],
            &embeddings,
            &[0],
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        for t in [2.0f32, 5.0, 8.0] {
            assert!(
                segments.iter().any(|s| s.start <= t && t <= s.end),
                "t={t} must stay covered, got {segments:?}"
            );
        }
        assert!(
            segments.iter().all(|s| s.speaker == 0),
            "single cluster keeps one label, got {segments:?}"
        );
    }

    #[test]
    fn mapped_overlap_splits_region_primary_without_losing_coverage() {
        // Primary [0,10] (label 0) fully mapped with overlap [4,6] (locals
        // 0,1 → globals 0,1): the primary is split into [0,4]+[6,10] and the
        // resegmenter re-emits [4,6] for both speakers — no triple coverage,
        // no lost coverage.
        let c0 = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 10.0, 0)],
            overlaps: vec![(
                polyvoice::types::TimeRange { start: 4.0, end: 6.0 },
                0,
                1,
            )],
            units: vec![
                DenseUnit {
                    time: polyvoice::types::TimeRange { start: 0.0, end: 5.0 },
                    local_idx: 0,
                    segment_idx: 0,
                    embedding: vec![1.0, 0.0, 0.0],
                },
                DenseUnit {
                    time: polyvoice::types::TimeRange { start: 5.0, end: 10.0 },
                    local_idx: 0,
                    segment_idx: 0,
                    embedding: vec![1.0, 0.0, 0.0],
                },
                DenseUnit {
                    time: polyvoice::types::TimeRange { start: 0.0, end: 2.0 },
                    local_idx: 1,
                    segment_idx: 0,
                    embedding: vec![0.0, 1.0, 0.0],
                },
            ],
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> = c0.units.iter().map(|u| u.embedding.clone()).collect();
        let config = DiarizationConfig {
            gap_merge_secs: 0.0,
            ..DiarizationConfig::default()
        };
        let (segments, _) = assemble_channel_turns(
            &[c0],
            &embeddings,
            &[0, 0, 1],
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        // [0,4]→0, [4,6]→0, [4,6]→1, [6,10]→0: overlap span carries exactly
        // two distinct speakers, and every instant stays covered.
        let at_overlap: Vec<&DiarizationSegment> = segments
            .iter()
            .filter(|s| s.start < 6.0 && s.end > 4.0)
            .collect();
        assert_eq!(at_overlap.len(), 2, "overlap span has exactly the pair, got {segments:?}");
        assert_ne!(at_overlap[0].speaker, at_overlap[1].speaker);
        for t in [2.0f32, 5.0, 8.0] {
            assert!(
                segments.iter().any(|s| s.start <= t && t <= s.end),
                "t={t} must stay covered, got {segments:?}"
            );
        }
    }

    #[test]
    fn overlap_pair_yields_two_speaker_turns() {
        // Chunk 0: speaker A solo [0,10] with an overlap pair [4,6] whose
        // second local never appears solo in this chunk (mixed-embedding
        // fallback). Chunk 1 (starts 9.5 s): speaker B solo [11,13] global.
        // (The overlap pair members are excluded from `primary`, matching the
        // core's `!is_overlap` filter.)
        let c0 = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 10.0, 0)],
            overlaps: vec![(
                polyvoice::types::TimeRange { start: 4.0, end: 6.0 },
                0,
                1,
            )],
            units: vec![DenseUnit {
                time: polyvoice::types::TimeRange { start: 0.0, end: 10.0 },
                local_idx: 0,
                segment_idx: 0,
                embedding: vec![1.0, 0.0, 0.0],
            }],
            mixed_overlaps: vec![(
                polyvoice::types::TimeRange { start: 4.0, end: 6.0 },
                vec![0.0, 1.0, 0.0],
            )],
        };
        let c1 = ChunkRecord {
            start_secs: 9.5,
            primary: vec![raw_seg(1.5, 3.5, 1)],
            overlaps: Vec::new(),
            units: vec![DenseUnit {
                time: polyvoice::types::TimeRange { start: 1.5, end: 3.5 },
                local_idx: 1,
                segment_idx: 0,
                embedding: vec![0.05, 0.95, 0.0],
            }],
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> =
            c0.units.iter().chain(c1.units.iter()).map(|u| u.embedding.clone()).collect();
        // Global clusters: 0 = speaker A (unit 0), 1 = speaker B (unit 1).
        let labels = vec![0usize, 1];
        let config = DiarizationConfig {
            gap_merge_secs: 0.5,
            ..DiarizationConfig::default()
        };
        let (segments, clustered) = assemble_channel_turns(
            &[c0, c1],
            &embeddings,
            &labels,
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        assert_eq!(clustered.len(), 2);
        let spk_a = clustered[0].speaker;
        let spk_b = clustered[1].speaker;
        assert_ne!(spk_a, spk_b, "distinct clusters get distinct labels");
        // Overlap [4,6] carries two distinct speakers: A's solo turn plus a
        // secondary B turn recovered from the mixed embedding.
        let at_overlap: Vec<&DiarizationSegment> = segments
            .iter()
            .filter(|s| s.start < 6.0 && s.end > 4.0)
            .collect();
        assert_eq!(at_overlap.len(), 2, "overlap region has two speaker turns");
        assert_ne!(at_overlap[0].speaker, at_overlap[1].speaker);
        assert!(
            at_overlap.iter().any(|s| s.speaker == spk_b && s.start >= 3.9 && s.end <= 6.1),
            "secondary B turn covers the overlap region, got {:?}",
            at_overlap
        );
        // Non-overlap instants stay single-labeled: no same-speaker overlap.
        for i in 0..segments.len() {
            for j in i + 1..segments.len() {
                let (a, b) = (&segments[i], &segments[j]);
                let ov = a.end.min(b.end) - a.start.max(b.start);
                if ov > 0.01 {
                    assert_ne!(a.speaker, b.speaker, "same-speaker overlap must not survive");
                }
            }
        }
    }

    #[test]
    fn chunk_boundary_split_is_bridged_by_gap_fill() {
        // Speaker A's [0,10] + [9.5,14] across the 0.5 s chunk overlap, and
        // speaker B [11,13] in between: A's pieces bridge (negative gap ≤
        // max_gap), B's boundary with A survives.
        let c0 = ChunkRecord {
            start_secs: 0.0,
            primary: vec![raw_seg(0.0, 10.0, 0)],
            overlaps: Vec::new(),
            units: vec![DenseUnit {
                time: polyvoice::types::TimeRange { start: 0.0, end: 10.0 },
                local_idx: 0,
                segment_idx: 0,
                embedding: vec![1.0, 0.0],
            }],
            mixed_overlaps: Vec::new(),
        };
        let c1 = ChunkRecord {
            start_secs: 9.5,
            primary: vec![raw_seg(0.0, 4.5, 0), raw_seg(1.5, 3.5, 1)],
            overlaps: Vec::new(),
            units: vec![
                DenseUnit {
                    time: polyvoice::types::TimeRange { start: 0.0, end: 4.5 },
                    local_idx: 0,
                    segment_idx: 0,
                    embedding: vec![0.99, 0.05],
                },
                DenseUnit {
                    time: polyvoice::types::TimeRange { start: 1.5, end: 3.5 },
                    local_idx: 1,
                    segment_idx: 1,
                    embedding: vec![0.0, 1.0],
                },
            ],
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> =
            c0.units.iter().chain(c1.units.iter()).map(|u| u.embedding.clone()).collect();
        // Units: (A c0), (A c1), (B c1) -> clusters 0,0,1.
        let labels = vec![0usize, 0, 1];
        let config = DiarizationConfig {
            gap_merge_secs: 0.5,
            ..DiarizationConfig::default()
        };
        let (segments, _) = assemble_channel_turns(
            &[c0, c1],
            &embeddings,
            &labels,
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        let a_turns: Vec<&DiarizationSegment> =
            segments.iter().filter(|s| s.speaker == 0).collect();
        assert_eq!(a_turns.len(), 1, "boundary split bridged into one turn");
        assert!(a_turns[0].start <= 0.0 && a_turns[0].end >= 13.9, "{:?}", a_turns[0]);
        let b_turns: Vec<&DiarizationSegment> =
            segments.iter().filter(|s| s.speaker == 1).collect();
        assert_eq!(b_turns.len(), 1);
        assert!((b_turns[0].start - 11.0).abs() < 0.01 && (b_turns[0].end - 13.0).abs() < 0.01);
    }
}
