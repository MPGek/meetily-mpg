//! Blocking orchestration of the offline pass: resolve the recording's channel
//! layout, run the core over each channel (in memory or streamed from ffmpeg),
//! and report per-stage timings.

use log::{info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Runtime};

use super::super::core::factory::create_polyvoice_diarizer_for_app;
use super::super::core::timeline::compute_speaker_matches;
use super::super::core::units::{count_unique_speakers, StageTimings};
use super::super::persist::clusters::persist_and_recognize_session;
use super::super::telemetry::{emit_progress, MemorySampler};
use super::super::core::segment::V2Core;
use super::super::{
    ChannelClusters, ClusteredEmbedding, DiarizationConfig, DiarizationProgress,
    DiarizationResult, DiarizationSegment, PolyvoiceDiarizer, DIARIZATION_SAMPLE_RATE,
};
use super::chunking::run_chunked_polyvoice_diarization;
use super::guard::{DiarizationGuard, DIARIZATION_CANCELLED};
use super::pcm::{spawn_ffmpeg_pcm, PcmStream, StreamWindows};
use crate::audio::audio_file::find_audio_file;
use crate::audio::token_assignment::{assign_tokens_to_speakers, SpeakerTurn as TokenTurn};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::state::AppState;
use crate::audio::decoder::{
    convert_to_wav_with_ffmpeg, decode_audio_file, detect_channel_layout, needs_ffmpeg_conversion,
    ChannelLayout,
};
use crate::audio::ffmpeg::find_ffmpeg_path;

// ===== Blocking diarization orchestration =====

#[allow(dead_code)]
pub(crate) fn run_diarization_blocking<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder_path: &str,
    _models_dir: &PathBuf,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
    transcripts: &[crate::database::models::Transcript],
) -> Result<
    (
        DiarizationResult,
        Vec<(String, String)>,
        ChannelClusters,
        ChannelClusters,
        bool,
    ),
    String,
> {
    run_diarization_blocking_with_app(
        app,
        meeting_id,
        folder_path,
        max_speakers,
        config,
        transcripts,
    )
}

pub(crate) fn run_diarization_blocking_with_app<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    folder_path: &str,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
    transcripts: &[crate::database::models::Transcript],
) -> Result<
    (
        DiarizationResult,
        Vec<(String, String)>,
        ChannelClusters,
        ChannelClusters,
        bool,
    ),
    String,
> {
    let overall_start = Instant::now();
    let mut timings = StageTimings::default();
    let memory_sampler = MemorySampler::start();

    emit_progress(app, meeting_id, "loading", 10, "Finding audio file...");

    let decode_start = Instant::now();
    let audio_path = find_audio_file(std::path::Path::new(folder_path))?;

    // Resolve a streamable source path: mkv/webm/wma are pre-converted to a
    // temporary WAV that ffmpeg (and the Symphonia fallback) can read.
    let (_temp_wav_guard, source_path): (Option<tempfile::TempPath>, PathBuf) =
        if needs_ffmpeg_conversion(&audio_path) {
            let temp_path = convert_to_wav_with_ffmpeg(&audio_path, None)
                .map_err(|e| format!("Failed to convert audio for streaming: {}", e))?;
            let wav_path = temp_path.to_path_buf();
            (Some(temp_path), wav_path)
        } else {
            (None, audio_path.clone())
        };

    // Resolve the channel layout from the decoded audio (metadata fast path,
    // first-packet decode when the container omits the count) — never default
    // a missing count to mono, which silently downmixed stereo recordings.
    emit_progress(app, meeting_id, "decoding", 15, "Streaming audio...");
    let layout = detect_channel_layout(&source_path)
        .map_err(|e| format!("Failed to probe audio metadata: {}", e))?;
    let channel_split = channel_split_for_layout(layout);
    let mut is_stereo = layout.is_stereo();
    if channel_split == ChannelSplit::NativeDecoded {
        warn!(
            "Channel layout unknown for {}; passing the native stream without downmixing",
            source_path.display()
        );
    }
    timings.decode_secs = decode_start.elapsed().as_secs_f64();

    emit_progress(
        app,
        meeting_id,
        "diarizing",
        20,
        "Running speaker diarization...",
    );

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    // Load the diarizer via the 3-location fallback (app_data → resource → manifest).
    let diarizer = create_polyvoice_diarizer_for_app(app, max_speakers, config)
        .map_err(|e| format!("Diarization failed: {}", e))?;

    let channel_start = Instant::now();
    let (mic_result, sys_result): (ChannelRunResult, ChannelRunResult) =
        match (find_ffmpeg_path(), channel_split) {
            // Unknown layout: never downmix. An unknown stereo file must at
            // worst be split, not collapsed, so decode the native stream and
            // split it — this also re-derives the real layout from the audio.
            (_, ChannelSplit::NativeDecoded) => {
                let (mic, sys, stereo) =
                    diarize_decoded_channels(&diarizer, &source_path, config)?;
                is_stereo = stereo;
                (mic, sys)
            }
            (Some(ffmpeg), ChannelSplit::Stereo) => {
                let left = spawn_ffmpeg_pcm(&ffmpeg, &source_path, Some(0))?;
                let right = spawn_ffmpeg_pcm(&ffmpeg, &source_path, Some(1))?;
                rayon::join(
                    || run_channel_diarization_stream(&diarizer, left, config),
                    || run_channel_diarization_stream(&diarizer, right, config),
                )
            }
            (Some(ffmpeg), ChannelSplit::Mono) => {
                let mono = spawn_ffmpeg_pcm(&ffmpeg, &source_path, None)?;
                let mic = run_channel_diarization_stream(&diarizer, mono, config);
                (mic, Ok((Vec::new(), Vec::new(), StageTimings::default())))
            }
            (None, _) => {
                // ffmpeg unavailable: fall back to full Symphonia decode (higher peak memory).
                warn!(
                    "ffmpeg not found; falling back to in-memory Symphonia decode for diarization"
                );
                let (mic, sys, stereo) =
                    diarize_decoded_channels(&diarizer, &source_path, config)?;
                is_stereo = stereo;
                (mic, sys)
            }
        };

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let (mic_segments, mic_embeddings, mic_timings) =
        mic_result.map_err(|e| format!("Microphone channel failed: {}", e))?;
    let (sys_segments, sys_embeddings, sys_timings) =
        sys_result.map_err(|e| format!("System channel failed: {}", e))?;

    timings.segmentation_secs = mic_timings.segmentation_secs + sys_timings.segmentation_secs;
    timings.embedding_secs = mic_timings.embedding_secs + sys_timings.embedding_secs;
    timings.clustering_secs = mic_timings.clustering_secs + sys_timings.clustering_secs;
    timings.resegmentation_secs =
        mic_timings.resegmentation_secs + sys_timings.resegmentation_secs;
    let channel_elapsed = channel_start.elapsed().as_secs_f64();

    emit_progress(
        app,
        meeting_id,
        "matching",
        70,
        "Matching speakers to transcripts...",
    );

    let matching_start = Instant::now();
    let speakers_found =
        count_unique_speakers(&mic_segments) + count_unique_speakers(&sys_segments);
    let speaker_updates = compute_speaker_matches(
        &mic_segments,
        &sys_segments,
        is_stereo,
        transcripts,
        app,
        meeting_id,
    )?;
    timings.matching_secs = matching_start.elapsed().as_secs_f64();

    let peak_mb = memory_sampler.finish();
    let overall_secs = overall_start.elapsed().as_secs_f64();

    info!(
        "Diarization timing for {}: decode={:.2}s, segmentation={:.2}s, embedding={:.2}s, clustering={:.2}s, resegmentation={:.2}s, matching={:.2}s, channel_total={:.2}s, overall={:.2}s, peak_rss={}MB, segments={}, speakers={}",
        meeting_id,
        timings.decode_secs,
        timings.segmentation_secs,
        timings.embedding_secs,
        timings.clustering_secs,
        timings.resegmentation_secs,
        timings.matching_secs,
        channel_elapsed,
        overall_secs,
        peak_mb,
        speaker_updates.len(),
        speakers_found
    );

    const TIME_WARNING_SECS: f64 = 600.0;
    const MEMORY_WARNING_MB: u64 = 4096;
    if overall_secs > TIME_WARNING_SECS || peak_mb > MEMORY_WARNING_MB {
        warn!(
            "Diarization regression warning: overall={:.2}s (threshold {}s), peak_rss={}MB (threshold {}MB)",
            overall_secs, TIME_WARNING_SECS, peak_mb, MEMORY_WARNING_MB
        );
    }

    Ok((
        DiarizationResult {
            meeting_id: meeting_id.to_string(),
            segments_labeled: speaker_updates.len(),
            speakers_found,
        },
        speaker_updates,
        ChannelClusters {
            segments: mic_segments,
            embeddings: mic_embeddings,
        },
        ChannelClusters {
            segments: sys_segments,
            embeddings: sys_embeddings,
        },
        is_stereo,
    ))
}

/// How offline diarization should obtain the microphone/system streams for a
/// resolved channel layout. `NativeDecoded` is the no-downmix path taken when
/// the layout cannot be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelSplit {
    /// Two decoded channels: spawn independent left/right streams.
    Stereo,
    /// A genuinely single-channel recording: one downmixed mono stream.
    Mono,
    /// Layout unknown: decode the native stream and split it, never downmix.
    NativeDecoded,
}

/// Map a detected layout onto the channel-split strategy. Only a genuinely
/// single-channel layout selects `Mono` (the `ffmpeg -ac 1` path); `Unknown`
/// selects the native split instead of a silent downmix.
fn channel_split_for_layout(layout: ChannelLayout) -> ChannelSplit {
    if layout.is_stereo() {
        ChannelSplit::Stereo
    } else if layout.is_mono() {
        ChannelSplit::Mono
    } else {
        ChannelSplit::NativeDecoded
    }
}

/// Per-channel diarization run output, or the channel's failure.
type ChannelRunResult = Result<
    (
        Vec<DiarizationSegment>,
        Vec<ClusteredEmbedding>,
        StageTimings,
    ),
    String,
>;

pub(crate) fn run_channel_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
    channel_name: &str,
) -> ChannelRunResult {
    info!(
        "Running diarization on {} channel ({} samples, {}Hz)",
        channel_name,
        samples.len(),
        sample_rate
    );
    // Fallback path: diarization always processes recordings in chunks.
    run_chunked_polyvoice_diarization(diarizer, samples, sample_rate, config)
}

/// Decode a recording's native stream with Symphonia and diarize each channel
/// independently, returning the mic/system runs plus whether the decoded
/// layout is stereo. Used when ffmpeg is unavailable and when the layout could
/// not be resolved from the container/first packet: the native stream is split
/// rather than downmixed, so a hidden stereo recording is never collapsed.
fn diarize_decoded_channels(
    diarizer: &PolyvoiceDiarizer,
    source_path: &Path,
    config: &DiarizationConfig,
) -> Result<(ChannelRunResult, ChannelRunResult, bool), String> {
    let decoded = decode_audio_file(source_path).map_err(|e| {
        format!(
            "Failed to decode audio for channel splitting: {} — the recording's channel layout could not be determined. Re-export the audio or install ffmpeg so it can be split without downmixing.",
            e
        )
    })?;
    let sample_rate = decoded.sample_rate;
    let (left, right) = decoded.extract_channels();
    let mic_stream = left.unwrap_or_default();
    match right {
        Some(sys_stream) => {
            let (mic, sys) = rayon::join(
                || run_channel_diarization(diarizer, &mic_stream, sample_rate, config, "mic"),
                || run_channel_diarization(diarizer, &sys_stream, sample_rate, config, "sys"),
            );
            Ok((mic, sys, true))
        }
        None => {
            let mic = run_channel_diarization(diarizer, &mic_stream, sample_rate, config, "mic");
            Ok((
                mic,
                Ok((Vec::new(), Vec::new(), StageTimings::default())),
                false,
            ))
        }
    }
}

/// Diarize a channel streamed from ffmpeg (already 16 kHz), reading overlapping
/// in-memory windows through the v2 core and clustering all accumulated
/// embeddings globally.
fn run_channel_diarization_stream(
    diarizer: &PolyvoiceDiarizer,
    mut pcm: PcmStream,
    config: &DiarizationConfig,
) -> Result<
    (
        Vec<DiarizationSegment>,
        Vec<ClusteredEmbedding>,
        StageTimings,
    ),
    String,
> {
    let chunk_samples = (config.chunk_duration_secs() * DIARIZATION_SAMPLE_RATE as f32) as usize;
    let overlap_samples = (config.chunk_overlap_secs * DIARIZATION_SAMPLE_RATE as f32) as usize;

    let mut core = V2Core::new(diarizer, config);
    let mut windows = StreamWindows::new(chunk_samples, overlap_samples);

    loop {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            pcm.kill();
            return Err("Diarization cancelled".to_string());
        }

        let (window_start_seconds, window) = match windows.next_from(&mut pcm.stdout) {
            Ok(Some(w)) => w,
            Ok(None) => break,
            Err(e) => {
                pcm.kill();
                return Err(e);
            }
        };

        if window.is_empty() {
            continue;
        }
        if let Err(e) = core.process_chunk(window_start_seconds, &window) {
            pcm.kill();
            return Err(e);
        }
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        pcm.kill();
        return Err("Diarization cancelled".to_string());
    }

    pcm.finish()?;
    core.finish()
}

pub(crate) async fn run_offline_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    max_speakers: Option<i32>,
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    // Never start while retranscription is replacing transcript rows: the two
    // jobs write the same rows and cluster mappings (D4).
    if crate::audio::retranscription::is_retranscription_in_progress() {
        return Err(
            "Retranscription is in progress; wait for it to finish before running speaker analysis"
                .to_string(),
        );
    }
    let _guard = DiarizationGuard::acquire()?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

    let config = DiarizationConfig::resolved();

    let pool = state.db_manager.pool();

    MeetingsRepository::update_diarization_status(pool, &meeting_id, "processing")
        .await
        .map_err(|e| format!("Failed to update diarization status: {}", e))?;

    let transcripts = MeetingsRepository::get_transcripts_for_diarization(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load transcripts: {}", e))?;

    if transcripts.is_empty() {
        MeetingsRepository::update_diarization_status(pool, &meeting_id, "failed")
            .await
            .ok();
        return Err("No transcripts found for this meeting".to_string());
    }

    let meeting = MeetingsRepository::get_meeting_metadata(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| "Meeting not found".to_string())?;

    let folder_path = meeting
        .folder_path
        .ok_or_else(|| "Meeting has no folder path — cannot find audio file".to_string())?;

    let app_clone = app.clone();
    let meeting_id_clone = meeting_id.clone();
    let transcripts_for_block = transcripts.clone();
    let folder_path_for_repair = folder_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        run_diarization_blocking_with_app(
            &app_clone,
            &meeting_id_clone,
            &folder_path,
            max_speakers,
            &config,
            &transcripts_for_block,
        )
    })
    .await
    .map_err(|e| format!("Diarization task panicked: {}", e))?;

    match result {
        Ok((diar_result, mut speaker_updates, mic_clusters, sys_clusters, is_stereo)) => {
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
                    let folder = folder_path_for_repair.clone();
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

            for t in &transcripts {
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
                                    &sys_clusters.segments
                                } else {
                                    &mic_clusters.segments
                                };
                            if !segs_ref.is_empty() {
                                let turns: Vec<TokenTurn> = segs_ref
                                    .iter()
                                    .map(|s| TokenTurn {
                                        start: s.start,
                                        end: s.end,
                                        speaker: s.speaker,
                                    })
                                    .collect();
                                let assign = assign_tokens_to_speakers(&tokens, &turns);
                                if assign.blocks.len() > 1 {
                                    saw_token_split = true;
                                    for (idx, block) in assign.blocks.iter().enumerate() {
                                        let slice = &tokens[block.start_idx..=block.end_idx];
                                        let text_parts: Vec<String> =
                                            slice.iter().map(|tk| tk.text.clone()).collect();
                                        let joined = text_parts.join("");
                                        let text = if joined.trim().is_empty() {
                                            text_parts.join(" ")
                                        } else {
                                            joined
                                        };
                                        let text = text.trim().to_string();
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

            for (transcript_id, speaker_id) in &speaker_updates {
                MeetingsRepository::update_transcript_speaker(pool, transcript_id, speaker_id)
                    .await
                    .map_err(|e| format!("Failed to update speaker: {}", e))?;
            }

            // Persist per-cluster centroid + exemplar caches, then auto-assign
            // recognized speakers (change: speaker-identity-registry). Clusters
            // without candidates / below threshold stay anonymous.
            persist_and_recognize_session(
                pool,
                &meeting_id,
                &mic_clusters.embeddings,
                &sys_clusters.embeddings,
                is_stereo,
            )
            .await?;

            MeetingsRepository::update_diarization_status(pool, &meeting_id, "complete")
                .await
                .map_err(|e| format!("Failed to update diarization status: {}", e))?;

            let _ = app.emit(
                "diarization-progress",
                DiarizationProgress {
                    meeting_id: meeting_id.clone(),
                    status: "complete".to_string(),
                    progress: 100,
                    message: format!(
                        "Labeled {} segments from {} speakers",
                        diar_result.segments_labeled, diar_result.speakers_found
                    ),
                },
            );

            Ok(diar_result)
        }
        Err(e) => {
            MeetingsRepository::update_diarization_status(pool, &meeting_id, "failed")
                .await
                .ok();
            Err(e)
        }
    }
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

#[cfg(test)]
mod tests {
    use polyvoice::clusterer::Clusterer as _;
    use polyvoice::embedder::Embedder as _;

    use crate::audio::diarization::config::{DEFAULT_CLUSTER_CEILING, DEFAULT_GAP_MERGE_SECS};
    use crate::audio::diarization::core::factory::{
        create_polyvoice_diarizer, diarize_wav_samples, resolve_models_dir_standalone,
    };
    use super::*;

    #[test]
    fn unknown_layout_never_selects_the_downmix_path() {
        use crate::audio::decoder::ChannelLayout;
        // An unknown layout must decode the native stream and split it; it must
        // never take the `ffmpeg -ac 1` mono path.
        assert_eq!(
            channel_split_for_layout(ChannelLayout::Unknown),
            ChannelSplit::NativeDecoded
        );
        assert_ne!(
            channel_split_for_layout(ChannelLayout::Unknown),
            ChannelSplit::Mono
        );
        // Only genuinely single-channel decoded audio may downmix.
        assert_eq!(
            channel_split_for_layout(ChannelLayout::Known(1)),
            ChannelSplit::Mono
        );
        assert_eq!(
            channel_split_for_layout(ChannelLayout::Known(2)),
            ChannelSplit::Stereo
        );
        // Any multi-channel layout is treated as stereo (left/right split).
        assert_eq!(
            channel_split_for_layout(ChannelLayout::Known(6)),
            ChannelSplit::Stereo
        );
    }

    fn default_config() -> DiarizationConfig {
        DiarizationConfig::default()
    }

    fn find_models_dir() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("MEETILY_MODELS_DIR") {
            let p = PathBuf::from(dir);
            if crate::audio::embedder::is_enhanced_installed(&p) {
                return Some(p);
            }
            // MEETILY_MODELS_DIR override wins even if only raw existence; keep fallback for tests that create tiny dummies
            if p.join("segmentation-3.0.onnx").exists() && p.join("titanet_large.onnx").exists() {
                return Some(p);
            }
        }
        let mut candidates: Vec<PathBuf> = [
            std::env::var("APPDATA")
                .ok()
                .map(|d| PathBuf::from(d).join("com.meetily.ai").join("models")),
            std::env::var("HOME").ok().map(|d| {
                PathBuf::from(d)
                    .join("Library")
                    .join("Application Support")
                    .join("com.meetily.ai")
                    .join("models")
            }),
            std::env::var("XDG_DATA_HOME")
                .ok()
                .map(|d| PathBuf::from(d).join("com.meetily.ai").join("models")),
            std::env::var("HOME").ok().map(|d| {
                PathBuf::from(d)
                    .join(".local")
                    .join("share")
                    .join("com.meetily.ai")
                    .join("models")
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        // Dev manifest fallback (cargo tauri dev) and resource dir fallback (bundled)
        candidates.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models"));
        // Resource dir near executable (best-effort for spike tests on installed builds)
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(parent.join("resources").join("models"));
                candidates.push(parent.join("models"));
            }
        }
        // Use shared resolver helper (first verified location wins, size >1KB gate)
        if let Some(dir) =
            crate::audio::embedder::resolve_enhanced_models_dir_from_paths(&candidates)
        {
            return Some(dir);
        }
        // Fallback to raw existence check for spike tests with tiny dummies
        candidates.into_iter().find(|p| {
            p.join("segmentation-3.0.onnx").exists() && p.join("titanet_large.onnx").exists()
        })
    }

    fn synthetic_speech_16k() -> Vec<f32> {
        // 4 seconds of 16 kHz tone bursts (amplitude-modulated) as stand-in audio.
        let mut samples = Vec::with_capacity(16000 * 4);
        for i in 0..16000 * 4 {
            let t = i as f32 / 16000.0;
            let tone = (2.0 * std::f32::consts::PI * 220.0 * t).sin();
            let burst = if (t % 1.0) < 0.6 { 1.0 } else { 0.0 };
            samples.push(tone * 0.3 * burst);
        }
        samples
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_offline_pipeline() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let diarizer = create_polyvoice_diarizer(&models_dir, None, &default_config())
            .expect("polyvoice diarizer should initialize with the enhanced models");
        let samples = synthetic_speech_16k();
        let (segments, _, _) =
            run_chunked_polyvoice_diarization(&diarizer, &samples, 16000, &default_config())
                .expect("offline diarization should return a result");
        info!(
            "spike: polyvoice offline diarization produced {} segments on synthetic audio",
            segments.len()
        );
        for pair in segments.windows(2) {
            assert!(
                pair[0].start <= pair[1].start,
                "segments must be sorted by start time"
            );
        }
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_short_window_embedding() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = crate::audio::embedder::enhanced_model_paths(&models_dir);
        let embedder = polyvoice::fbank_onnx::FbankOnnxExtractor::new(
            &emb_model,
            192,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("TitaNet extractor should initialize with the enhanced model");
        assert_eq!(embedder.dim(), 192, "titanet_large embeds to 192 dims");

        // Probe embeddings from short windows — the Fast-mode streaming geometry.
        let samples = synthetic_speech_16k();
        for secs in [0.25f32, 0.5, 1.0, 1.5] {
            let n = (16000.0 * secs) as usize;
            let emb = embedder.embed(&samples[..n]);
            info!(
                "spike: embed at {:.2}s -> {:?}",
                secs,
                emb.as_ref()
                    .map(|e| format!("{} dims", e.len()))
                    .unwrap_or_else(|e| format!("error: {e}"))
            );
            if let Ok(e) = emb {
                assert_eq!(e.len(), 192);
                let norm: f32 = e.iter().map(|x| x * x).sum::<f32>().sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-2,
                    "embedding must be L2-normalized (got {norm})"
                );
            }
        }
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_streaming_pipeline() {
        use polyvoice::streaming::{LatencyPreset, StreamingPipeline};
        use polyvoice::vad::{EnergyVad, VadConfig};

        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = crate::audio::embedder::enhanced_model_paths(&models_dir);
        let extractor = polyvoice::fbank_onnx::FbankOnnxExtractor::new(
            &emb_model,
            192,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("TitaNet extractor should initialize");

        let vad = EnergyVad::new(-100.0, 16000, 512);
        let mut pipeline = StreamingPipeline::with_latency_preset(
            vad,
            extractor,
            LatencyPreset::Balanced,
            VadConfig::default(),
        )
        .expect("StreamingPipeline should build with balanced preset");

        let samples = synthetic_speech_16k();
        for chunk in samples.chunks(16000) {
            let turns = pipeline
                .feed(chunk)
                .expect("feed should accept arbitrary 16 kHz chunks");
            info!("spike: streaming feed produced {} turns", turns.len());
        }
        let flushed = pipeline
            .flush()
            .expect("flush should return remaining turns");
        info!(
            "spike: streaming flush produced {} turns, {} total buffered, {} speakers",
            flushed.len(),
            pipeline.turns().len(),
            pipeline.num_speakers()
        );
    }

    #[test]
    #[ignore = "spike: requires polyvoice diarization models (see standalone probe)"]
    fn spike_polyvoice_efficient_path() {
        let Some(models_dir) = find_models_dir() else {
            eprintln!("SKIP: diarization models not found on this machine");
            return;
        };
        let (_, emb_model) = crate::audio::embedder::enhanced_model_paths(&models_dir);
        let extractor = polyvoice::fbank_onnx::FbankOnnxExtractor::new(
            &emb_model,
            192,
            default_config().embedder_pool_size(),
            polyvoice::onnx::ExecutionProvider::Cpu,
        )
        .expect("TitaNet extractor should initialize");

        // Two distinct tone-burst signals simulate two speakers; embed per segment.
        let samples_a = synthetic_speech_16k();
        let samples_b: Vec<f32> = samples_a
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let t = i as f32 / 16000.0;
                s + (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.2
            })
            .collect();

        let mut embeddings: Vec<(f32, f32, Vec<f32>)> = Vec::new();
        for (start, seg) in [(0.0, &samples_a[..]), (2.0, &samples_b[..])] {
            let emb = extractor
                .embed(seg)
                .expect("embed should return a 192-dim embedding");
            assert_eq!(emb.len(), 192);
            embeddings.push((start, start + seg.len() as f32 / 16000.0, emb));
        }

        let clusterer = polyvoice::clusterer::AhcClusterer::new(8);
        let labels = clusterer
            .cluster(&embeddings.iter().map(|e| e.2.clone()).collect::<Vec<_>>())
            .expect("AhcClusterer should cluster buffered embeddings");
        assert_eq!(labels.len(), embeddings.len());
        info!(
            "spike: efficient-path clustering produced labels {:?}",
            labels
        );
        assert!(
            labels.iter().all(|&l| l < 8),
            "labels must respect the max-speakers ceiling"
        );
    }

    /// Parity check (add-diarization-eval-harness 2.3): the app's offline
    /// ffmpeg-streaming path and the harness's in-memory chunked core must
    /// produce identical speaker turns for the same audio. Requires
    /// `MEETILY_TEST_AUDIO` pointing at a real recording plus the enhanced
    /// models and ffmpeg on this machine.
    #[test]
    #[ignore = "parity: requires MEETILY_TEST_AUDIO, models, and ffmpeg"]
    fn parity_stream_vs_in_memory_core() {
        let Ok(path) = std::env::var("MEETILY_TEST_AUDIO") else {
            eprintln!("SKIP: MEETILY_TEST_AUDIO not set");
            return;
        };
        let source = PathBuf::from(&path);
        // Flag-free harness config == DiarizationConfig::default(): the bin
        // starts from default() and only overlays explicit CLI flags
        // (diarize_eval.rs), so a no-flag run must equal this config exactly.
        let config = DiarizationConfig::default();
        assert_eq!(
            (
                config.cluster_threshold,
                config.cluster_ceiling,
                config.gap_merge_secs
            ),
            (
                crate::audio::embedder::TITANET_CLUSTER_THRESHOLD,
                DEFAULT_CLUSTER_CEILING,
                DEFAULT_GAP_MERGE_SECS
            ),
            "flag-free harness parity requires default() to equal the built-in constants"
        );
        let is_stereo = detect_channel_layout(&source)
            .expect("probe audio")
            .is_stereo();
        let ffmpeg = find_ffmpeg_path().expect("ffmpeg required for parity check");

        // App path: streaming windows over the ffmpeg PCM pipe.
        let diarizer_app = create_polyvoice_diarizer(
            &resolve_models_dir_standalone(None).expect("models"),
            None,
            &config,
        )
        .expect("app-path diarizer");
        let mut app_turns: Vec<(f32, f32, i32)> = Vec::new();
        let chans: Vec<Option<u32>> = if is_stereo {
            vec![Some(0), Some(1)]
        } else {
            vec![None]
        };
        for ch in chans {
            let pcm = spawn_ffmpeg_pcm(&ffmpeg, &source, ch).expect("spawn ffmpeg");
            let (segments, _, _) =
                run_channel_diarization_stream(&diarizer_app, pcm, &config).expect("stream run");
            app_turns.extend(segments.iter().map(|s| (s.start, s.end, s.speaker)));
        }

        // Harness path: symphonia decode + in-memory chunked core.
        let decoded = decode_audio_file(&source).expect("decode audio");
        let (left, right) = decoded.extract_channels();
        let mut eval_turns: Vec<(f32, f32, i32)> = Vec::new();
        for samples in [left, right].into_iter().flatten() {
            let clusters = diarize_wav_samples(&samples, decoded.sample_rate, None, &config, None)
                .expect("harness run");
            eval_turns.extend(
                clusters
                    .segments
                    .iter()
                    .map(|s| (s.start, s.end, s.speaker)),
            );
        }

        assert!(
            !app_turns.is_empty(),
            "app path produced no segments (audio too quiet?)"
        );
        assert_eq!(
            app_turns.len(),
            eval_turns.len(),
            "turn count mismatch: app={} eval={}",
            app_turns.len(),
            eval_turns.len()
        );
        let mut max_dt = 0.0f32;
        for (a, e) in app_turns.iter().zip(eval_turns.iter()) {
            assert_eq!(a.2, e.2, "speaker label mismatch at app turn {:?}", a);
            max_dt = max_dt.max((a.0 - e.0).abs()).max((a.1 - e.1).abs());
        }
        assert!(
            max_dt < 0.01,
            "turn boundary drift {}s exceeds deterministic-identical tolerance",
            max_dt
        );
        info!("parity: {} turns identical (max drift {:.4}s)", app_turns.len(), max_dt);
    }
}
