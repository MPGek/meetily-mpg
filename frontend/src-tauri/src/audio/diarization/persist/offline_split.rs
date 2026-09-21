//! The offline N-way row split: when a stored transcript row's tokens span
//! more than one speaker, the row is rewritten as contiguous per-speaker rows
//! before speakers are persisted. Lifted out of the batch orchestrator by 05
//! task 2.5; the two SQL statements are unchanged.

use std::collections::HashMap;
use std::path::Path;

use log::{info, warn};
use sqlx::SqlitePool;

use super::super::core::timeline::split_tokens_by_speaker;
use crate::audio::audio_file::find_audio_file;
use super::super::DiarizationSegment;
use crate::database::models::Transcript;

/// Split the rows whose tokens cross a speaker boundary, persist the rewritten
/// rows, and return the speaker updates to apply: token-derived for every row
/// that split, and the caller's cluster-derived ones for the rest. When no row
/// splits, `speaker_updates` is returned untouched.
pub(crate) async fn split_rows_by_speaker(
    pool: &SqlitePool,
    transcripts: &[Transcript],
    folder_path_for_repair: &str,
    is_stereo: bool,
    mic_segments: &[DiarizationSegment],
    sys_segments: &[DiarizationSegment],
    mut speaker_updates: Vec<(String, String)>,
) -> Vec<(String, String)> {
    // Token-level refinement for offline path (task 4.3/4.4): if a transcript
    // carries token JSON and its tokens span ≥2 speakers, split the row
    // into N contiguous, gap-free blocks before persisting speakers.
    // This mirrors online_diarization.rs finalize logic but runs on the
    // post-clustering segments for offline re-analysis.
    let mut expanded_inserts: Vec<(
        String,
        String,
        String,
        Option<String>,
        f64,
        f64,
        f64,
        String,
        String,
    )> = Vec::new(); // (id, meeting_id, timestamp, source_device, start, end, duration, text, tokens_json)
    let mut original_row_updates: Vec<(String, f64, f64, f64, String, String)> = Vec::new(); // (id, start, end, duration, text, tokens_json) for first block
    let mut token_based_updates: Vec<(String, String)> = Vec::new();
    let mut saw_token_split = false;

    // Repair hook (word-level-diarization-alignment 5.5): refine the
    // word tokens of rows that lack refined timestamps, per-channel
    // from the meeting audio, immediately before the N-way split.
    // Rows already carrying refined tokens (live alignment during
    // recording) are skipped by the engine. Disabled/missing model ->
    // empty map -> split uses stored (baseline) tokens.
    let refined_tokens: HashMap<String, Vec<crate::audio::token_assignment::Token>> = {
        let align_settings = crate::audio::word_alignment::settings::current();
        if align_settings.enabled {
            let folder = folder_path_for_repair.to_string();
            let stereo = is_stereo;
            let rows: Vec<(String, String, Option<String>, Option<f64>, Option<f64>)> =
                transcripts
                    .iter()
                    .filter_map(|t| {
                        t.tokens.clone().map(|j| {
                            (
                                t.id.clone(),
                                j,
                                t.source_device.clone(),
                                t.audio_start_time,
                                t.audio_end_time,
                            )
                        })
                    })
                    .collect();
            if rows.is_empty() {
                HashMap::new()
            } else {
                tokio::task::spawn_blocking(move || {
                    refine_offline_rows(&folder, stereo, rows, &align_settings)
                })
                .await
                .unwrap_or_default()
            }
        } else {
            HashMap::new()
        }
    };

    for t in transcripts {
        if let Some(tokens_json) = &t.tokens {
            let tokens: Vec<crate::audio::token_assignment::Token> = refined_tokens
                .get(&t.id)
                .cloned()
                .or_else(|| {
                    serde_json::from_str::<Vec<crate::audio::token_assignment::Token>>(
                        tokens_json,
                    )
                    .ok()
                })
                .unwrap_or_default();
            if tokens.len() >= 2 {
                    let segs_ref =
                        if is_stereo && t.source_device.as_deref() == Some("System") {
                            sys_segments
                        } else {
                            mic_segments
                        };
                    if !segs_ref.is_empty() {
                        let blocks = split_tokens_by_speaker(&tokens, segs_ref);
                        if blocks.len() > 1 {
                            saw_token_split = true;
                            for (idx, block) in blocks.iter().enumerate() {
                                let slice = &tokens[block.start_idx..=block.end_idx];
                                let text = block.text.clone();
                                let block_tokens_json = serde_json::to_string(slice)
                                    .unwrap_or_else(|_| "[]".to_string());
                                let start = block.start as f64;
                                let end = block.end as f64;
                                let dur = (end - start).max(0.0);
                                if idx == 0 {
                                    original_row_updates.push((
                                        t.id.clone(),
                                        start,
                                        end,
                                        dur,
                                        text.clone(),
                                        block_tokens_json.clone(),
                                    ));
                                    let prefix = if is_stereo
                                        && t.source_device.as_deref() == Some("System")
                                    {
                                        "SPEAKER"
                                    } else if is_stereo {
                                        "MIC_SPEAKER"
                                    } else {
                                        "SPEAKER"
                                    };
                                    token_based_updates.push((
                                        t.id.clone(),
                                        format!("{}_{:02}", prefix, block.speaker),
                                    ));
                                } else {
                                    let new_id = format!("{}_split{}", t.id, idx);
                                    expanded_inserts.push((
                                        new_id.clone(),
                                        t.meeting_id.clone(),
                                        t.timestamp.clone(),
                                        t.source_device.clone(),
                                        start,
                                        end,
                                        dur,
                                        text.clone(),
                                        block_tokens_json.clone(),
                                    ));
                                    let prefix = if is_stereo
                                        && t.source_device.as_deref() == Some("System")
                                    {
                                        "SPEAKER"
                                    } else if is_stereo {
                                        "MIC_SPEAKER"
                                    } else {
                                        "SPEAKER"
                                    };
                                    token_based_updates.push((
                                        new_id,
                                        format!("{}_{:02}", prefix, block.speaker),
                                    ));
                                }
                            }
                            continue;
                        }
                    }
            }
        }
    }
    if saw_token_split {
        // Apply timing/text updates to original rows that split
        for (id, start, end, dur, text, tokens_json) in &original_row_updates {
            let _ = sqlx::query("UPDATE transcripts SET audio_start_time = ?, audio_end_time = ?, duration = ?, transcript = ?, tokens = ? WHERE id = ?")
                .bind(*start).bind(*end).bind(*dur).bind(text).bind(tokens_json).bind(id)
                .execute(pool).await;
        }
        // Insert remaining blocks as new rows
        for (new_id, meeting_id_ins, ts, src, start, end, dur, text, tokens_json) in
            &expanded_inserts
        {
            let _ = sqlx::query("INSERT OR IGNORE INTO transcripts (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration, source_device, tokens) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(new_id).bind(meeting_id_ins).bind(text).bind(ts).bind(*start).bind(*end).bind(*dur).bind(src).bind(tokens_json)
                .execute(pool).await;
        }
        // Replace speaker_updates with token-derived ones plus non-split fallback entries
        let split_ids: Vec<String> = original_row_updates
            .iter()
            .map(|(oid, _, _, _, _, _)| oid.clone())
            .collect();
        let mut non_split_updates = Vec::new();
        for (tid, spk) in &speaker_updates {
            if !split_ids.contains(tid) {
                non_split_updates.push((tid.clone(), spk.clone()));
            }
        }
        speaker_updates = [token_based_updates, non_split_updates].concat();
    }
    speaker_updates
}

/// Offline repair (word-level-diarization-alignment 5.5): refine the word
/// tokens of stored transcript rows that lack refined timestamps, per-channel
/// from the meeting audio file. Returns `row_id -> refined tokens` for rows
/// that were successfully refined; the caller overlays these before the N-way
/// split. Blocking (ffmpeg seek extraction) — call from `spawn_blocking`.
fn refine_offline_rows(
    folder: &str,
    stereo: bool,
    rows: Vec<(String, String, Option<String>, Option<f64>, Option<f64>)>,
    settings: &crate::audio::word_alignment::refine::AlignmentSettings,
) -> HashMap<String, Vec<crate::audio::token_assignment::Token>> {
    use crate::audio::word_alignment::refine::{
        refine_tokens_with_source, FileSpanSource,
    };
    let Some(engine) = settings.engine() else {
        return HashMap::new();
    };
    let audio_path = match find_audio_file(Path::new(folder)) {
        Ok(p) => p,
        Err(e) => {
            warn!("Alignment repair: no audio file in {}: {}", folder, e);
            return HashMap::new();
        }
    };
    let source = match FileSpanSource::new(audio_path, stereo) {
        Ok(s) => s,
        Err(e) => {
            warn!("Alignment repair: span source init failed: {}", e);
            return HashMap::new();
        }
    };
    let mut out = HashMap::new();
    let mut refined_count = 0;
    for (id, tokens_json, channel, start, end) in rows {
        let Ok(mut tokens) =
            serde_json::from_str::<Vec<crate::audio::token_assignment::Token>>(&tokens_json)
        else {
            continue;
        };
        let ch = channel.as_deref().unwrap_or("Microphone");
        let s = start.unwrap_or(0.0);
        let e = end.unwrap_or(0.0);
        if refine_tokens_with_source(&mut tokens, &source, ch, s, e, &engine) {
            refined_count += 1;
            out.insert(id, tokens);
        }
    }
    if refined_count > 0 {
        info!("Alignment repair: refined {} offline transcript row(s)", refined_count);
    }
    out
}
