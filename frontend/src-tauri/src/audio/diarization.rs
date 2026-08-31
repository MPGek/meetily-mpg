use crate::audio::audio_file::find_audio_file;
use crate::audio::decoder::{
    convert_to_wav_with_ffmpeg, decode_audio_file, needs_ffmpeg_conversion, probe_audio_metadata,
};
use crate::audio::ffmpeg::find_ffmpeg_path;
use crate::audio::speaker_recognition::{l2_normalize_in_place, Prototype};
use crate::audio::token_assignment::{assign_tokens_to_speakers, SpeakerTurn as TokenTurn};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::speaker::{Exemplar, SpeakerRepository};
use crate::state::AppState;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Maximum exemplar cache rows persisted per cluster (top by duration).
/// Enrollment later reparents the best-K=8 of these; the cache is bounded so
/// storage grows with the number of clusters, not segments.
const MAX_CLUSTER_CACHE_EXEMPLARS: usize = 32;

static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static DIARIZATION_CANCELLED: AtomicBool = AtomicBool::new(false);

struct DiarizationGuard;

impl DiarizationGuard {
    fn acquire() -> Result<Self, String> {
        if DIARIZATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Diarization already in progress".to_string());
        }
        Ok(DiarizationGuard)
    }
}

impl Drop for DiarizationGuard {
    fn drop(&mut self) {
        DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationProgress {
    pub meeting_id: String,
    pub status: String,
    pub progress: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationResult {
    pub meeting_id: String,
    pub segments_labeled: usize,
    pub speakers_found: usize,
}

pub fn is_diarization_in_progress() -> bool {
    DIARIZATION_IN_PROGRESS.load(Ordering::SeqCst)
}

pub fn cancel_diarization() {
    DIARIZATION_CANCELLED.store(true, Ordering::SeqCst);
    info!("Diarization cancellation requested");
}

#[tauri::command]
pub async fn get_diarization_status<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let pool = state.db_manager.pool();
    let meeting = MeetingsRepository::get_meeting_metadata(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting: {}", e))?
        .ok_or_else(|| "Meeting not found".to_string())?;

    Ok(serde_json::json!({
        "meeting_id": meeting.id,
        "diarization_status": meeting.diarization_status,
        "speaker_names": meeting.speaker_names,
    }))
}

#[tauri::command]
pub async fn update_speaker_label_command<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    speaker: String,
    label: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    MeetingsRepository::update_speaker_label(pool, &meeting_id, &speaker, &label)
        .await
        .map_err(|e| format!("Failed to update speaker label: {}", e))
}

/// Re-run speaker recognition for a meeting from the cached cluster centroids
/// only — no audio re-processing. User bindings (`matched_by='user'`) are
/// preserved; only unbound or auto-bound clusters are updated. The
/// expected-speaker allowlist (or all speakers when empty) constrains
/// candidates. Used after editing the expected-speaker list.
#[tauri::command]
pub async fn rematch_meeting_speakers<R: Runtime>(
    _app: AppHandle<R>,
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let pool = state.db_manager.pool();

    let centroids = SpeakerRepository::get_cluster_centroids(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load cached centroids: {}", e))?;
    if centroids.is_empty() {
        return Ok(serde_json::json!({ "meeting_id": meeting_id, "matched": 0 }));
    }

    let expected = SpeakerRepository::get_expected_speakers(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load expected speakers: {}", e))?;
    let candidates: Option<&[String]> = if expected.is_empty() {
        None
    } else {
        Some(&expected)
    };
    // Enhanced-only matching: centroids are loaded unconditionally and matched
    // against prototypes of the enhanced `titanet_large` family; legacy
    // `resnet34_int8` (256-d) rows are never candidates.
    let prototypes: Vec<Prototype> = SpeakerRepository::load_prototypes(
        pool,
        candidates,
        crate::audio::embedder::ENHANCED_MODEL_TAG,
    )
    .await
    .map_err(|e| format!("Failed to load prototypes: {}", e))?
    .into_iter()
    .map(Prototype::from)
    .collect();

    let mut matched = 0usize;
    // Current bindings so re-match only counts actual new assignments and
    // skips user-bound clusters (which are always preserved).
    let existing_rows = SpeakerRepository::get_meeting_speakers(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load meeting speakers: {}", e))?;
    let existing: HashMap<&str, (Option<&str>, Option<&str>)> = existing_rows
        .iter()
        .map(|r| {
            (
                r.cluster_label.as_str(),
                (r.speaker_id.as_deref(), r.matched_by.as_deref()),
            )
        })
        .collect();
    for c in &centroids {
        let threshold = crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD;
        if let Some(m) = crate::audio::speaker_recognition::best_match_with_threshold(
            &c.centroid,
            c.channel.as_deref(),
            &prototypes,
            threshold,
        ) {
            // Skip user-bound clusters: their binding always wins.
            if let Some((_, by)) = existing.get(c.cluster_label.as_str()) {
                if *by == Some("user") {
                    continue;
                }
            }
            // Skip clusters already bound to the same candidate.
            if let Some((sid, _)) = existing.get(c.cluster_label.as_str()) {
                if *sid == Some(m.speaker_id.as_str()) {
                    continue;
                }
            }
            SpeakerRepository::set_auto_binding_if_unbound(
                pool,
                &meeting_id,
                &c.cluster_label,
                &m.speaker_id,
                m.score as f64,
            )
            .await
            .map_err(|e| format!("Failed to auto-assign speaker: {}", e))?;
            matched += 1;
        }
    }

    Ok(serde_json::json!({ "meeting_id": meeting_id, "matched": matched }))
}

#[tauri::command]
pub async fn start_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    max_speakers: Option<i32>,
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    let _guard = DiarizationGuard::acquire()?;
    DIARIZATION_CANCELLED.store(false, Ordering::SeqCst);

    let config = DiarizationConfig::default();

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

// ===== Configuration =====

/// Fixed chunk duration for offline diarization (seconds). Diarization always
/// processes recordings in chunks to keep peak memory bounded.
const DIARIZATION_CHUNK_DURATION_SECS: f32 = 600.0;

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

/// Fixed concurrency profile: the ONNX session pool size is the smaller of 8
/// or 75% of the logical CPU core count (rounded up, minimum 1). There is no
/// user-facing memory-mode or session-count setting.
fn fixed_pool_size() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let seventy_five_percent = ((cores as f64) * 0.75).ceil() as usize;
    seventy_five_percent.min(8).max(1)
}

#[derive(Debug, Clone, Copy)]
pub struct DiarizationConfig {
    pub max_sessions: usize,
    pub chunk_overlap_secs: f32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            max_sessions: fixed_pool_size(),
            chunk_overlap_secs: 5.0,
        }
    }
}

impl DiarizationConfig {
    fn chunk_duration_secs(&self) -> f32 {
        DIARIZATION_CHUNK_DURATION_SECS
    }

    fn embedder_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }

    fn segmenter_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }
}

// ===== Blocking diarization orchestration =====

#[derive(Debug, Clone, Copy, Default)]
struct StageTimings {
    decode_secs: f64,
    segmentation_secs: f64,
    embedding_secs: f64,
    clustering_secs: f64,
    matching_secs: f64,
}

impl StageTimings {}

#[allow(dead_code)]
fn run_diarization_blocking<R: Runtime>(
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

fn run_diarization_blocking_with_app<R: Runtime>(
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

    // Probe channel count (Symphonia header read, no full decode).
    emit_progress(app, meeting_id, "decoding", 15, "Streaming audio...");
    let (_, channels) = probe_audio_metadata(&source_path)
        .map_err(|e| format!("Failed to probe audio metadata: {}", e))?;
    let is_stereo = channels == 2;
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
    let (mic_result, sys_result): (
        Result<
            (
                Vec<DiarizationSegment>,
                Vec<ClusteredEmbedding>,
                StageTimings,
            ),
            String,
        >,
        Result<
            (
                Vec<DiarizationSegment>,
                Vec<ClusteredEmbedding>,
                StageTimings,
            ),
            String,
        >,
    ) = match find_ffmpeg_path() {
        Some(ffmpeg) => {
            if is_stereo {
                let left = spawn_ffmpeg_pcm(&ffmpeg, &source_path, Some(0))?;
                let right = spawn_ffmpeg_pcm(&ffmpeg, &source_path, Some(1))?;
                rayon::join(
                    || run_channel_diarization_stream(&diarizer, left, config),
                    || run_channel_diarization_stream(&diarizer, right, config),
                )
            } else {
                let mono = spawn_ffmpeg_pcm(&ffmpeg, &source_path, None)?;
                let mic = run_channel_diarization_stream(&diarizer, mono, config);
                (mic, Ok((Vec::new(), Vec::new(), StageTimings::default())))
            }
        }
        None => {
            // ffmpeg unavailable: fall back to full Symphonia decode (higher peak memory).
            warn!("ffmpeg not found; falling back to in-memory Symphonia decode for diarization");
            let decoded = decode_audio_file(&source_path)
                .map_err(|e| format!("Failed to decode audio: {}", e))?;
            let (left, right) = decoded.extract_channels();
            let mic_stream = left.unwrap_or_default();
            if let Some(sys_stream) = right {
                rayon::join(
                    || {
                        run_channel_diarization(
                            &diarizer,
                            &mic_stream,
                            decoded.sample_rate,
                            config,
                            "mic",
                        )
                    },
                    || {
                        run_channel_diarization(
                            &diarizer,
                            &sys_stream,
                            decoded.sample_rate,
                            config,
                            "sys",
                        )
                    },
                )
            } else {
                let mic = run_channel_diarization(
                    &diarizer,
                    &mic_stream,
                    decoded.sample_rate,
                    config,
                    "mic",
                );
                (mic, Ok((Vec::new(), Vec::new(), StageTimings::default())))
            }
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

    let peak_mb = memory_sampler.stop();
    let overall_secs = overall_start.elapsed().as_secs_f64();

    info!(
        "Diarization timing for {}: decode={:.2}s, segmentation={:.2}s, embedding={:.2}s, clustering={:.2}s, matching={:.2}s, channel_total={:.2}s, overall={:.2}s, peak_rss={}MB, segments={}, speakers={}",
        meeting_id,
        timings.decode_secs,
        timings.segmentation_secs,
        timings.embedding_secs,
        timings.clustering_secs,
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

fn run_channel_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
    channel_name: &str,
) -> Result<
    (
        Vec<DiarizationSegment>,
        Vec<ClusteredEmbedding>,
        StageTimings,
    ),
    String,
> {
    info!(
        "Running diarization on {} channel ({} samples, {}Hz)",
        channel_name,
        samples.len(),
        sample_rate
    );
    // Fallback path: diarization always processes recordings in chunks.
    run_chunked_polyvoice_diarization(diarizer, samples, sample_rate, config)
}

#[derive(Debug, Clone)]
struct DiarizationSegment {
    start: f32,
    end: f32,
    speaker: i32,
}

/// An embedding tagged with its cluster id and source-segment duration,
/// produced after clustering. Used to compute per-cluster centroids and
/// exemplar caches for the speaker identity registry. Public so the online
/// diarization path can build the same shape at recording stop.
#[derive(Debug, Clone)]
pub struct ClusteredEmbedding {
    pub speaker: i32,
    pub embedding: Vec<f32>,
    pub duration_secs: f32,
    pub start_secs: Option<f32>,
    pub end_secs: Option<f32>,
}

/// Per-channel diarization output: labeled segments plus the clustered
/// embeddings aligned to them (captured before the start-time sort).
#[derive(Debug, Clone, Default)]
struct ChannelClusters {
    segments: Vec<DiarizationSegment>,
    embeddings: Vec<ClusteredEmbedding>,
}

/// Polyvoice diarization engine: enhanced segmentation-3.0 + TitaNet-Large
/// embedding + AHC clustering, loaded once per run.
struct PolyvoiceDiarizer {
    segmenter: Box<dyn crate::audio::segmentation::Segmenter>,
    embedder: Box<dyn crate::audio::embedder::SpeakerEmbedder>,
    clusterer: Box<dyn polyvoice::clusterer::Clusterer>,
}

fn create_polyvoice_diarizer(
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    // Enhanced-only engine family: segmentation and embedding construction
    // error with clear messages when the bundled enhanced models are absent
    // (no fallback to a standard/legacy model set).
    let segmenter =
        crate::audio::segmentation::create_segmenter(models_dir, config.segmenter_pool_size())?;
    let embedder =
        crate::audio::embedder::create_speaker_embedder(models_dir, config.embedder_pool_size())
            .map_err(|e| format!("Failed to create embedder: {}", e))?;
    let model_tag = embedder.model_tag();
    let family_threshold = embedder.family_threshold();
    log::info!(
        "Diarizer using enhanced family tag={} threshold={}",
        model_tag,
        family_threshold
    );

    let max_clusters = max_speakers.filter(|m| *m > 0).unwrap_or(0) as usize;
    let clusterer: Box<dyn polyvoice::clusterer::Clusterer> =
        Box::new(polyvoice::clusterer::MinClusterSizeClusterer::new(
            Box::new(polyvoice::clusterer::AhcClusterer::with_threshold(
                max_clusters,
                family_threshold,
            )),
            2,
        ));

    Ok(PolyvoiceDiarizer {
        segmenter,
        embedder,
        clusterer,
    })
}

fn create_polyvoice_diarizer_for_app<R: Runtime>(
    app: &AppHandle<R>,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    if let Some(dir) = crate::audio::embedder::resolve_enhanced_models_dir(app) {
        return create_polyvoice_diarizer(&dir, max_speakers, config);
    }
    let locations = crate::audio::embedder::format_enhanced_search_locations(app);
    Err(format!(
        "Enhanced diarization models not found. Searched: {}. The enhanced models (segmentation-3.0 + TitaNet-Large) are bundled at build time near the executable; rebuild with network or install a build that includes them.",
        locations
    ))
}

const DIARIZATION_SAMPLE_RATE: u32 = 16000;

fn embed_segments(
    embedder: &dyn crate::audio::embedder::SpeakerEmbedder,
    diar_samples: &[f32],
    raw_segments: &[crate::audio::segmentation::Segment],
    _config: &DiarizationConfig,
) -> (Vec<DiarizationSegment>, Vec<Vec<f32>>) {
    let mut segments: Vec<DiarizationSegment> = Vec::with_capacity(raw_segments.len());
    let mut slices: Vec<&[f32]> = Vec::with_capacity(raw_segments.len());

    for seg in raw_segments {
        let start = (seg.start as f64 * DIARIZATION_SAMPLE_RATE as f64) as usize;
        let end =
            ((seg.end as f64 * DIARIZATION_SAMPLE_RATE as f64) as usize).min(diar_samples.len());
        if end <= start {
            continue;
        }
        segments.push(DiarizationSegment {
            start: seg.start,
            end: seg.end,
            speaker: -1,
        });
        slices.push(&diar_samples[start..end]);
    }

    let embeddings = match embedder.embed_batch(&slices) {
        Ok(batch) => batch,
        Err(e) => {
            warn!(
                "Batch embedding failed ({}), falling back to per-segment embedding",
                e
            );
            slices
                .iter()
                .filter_map(|audio| match embedder.embed(audio) {
                    Ok(emb) => Some(emb),
                    Err(e) => {
                        warn!(
                            "Embedding extraction failed for a segment ({}), skipping it",
                            e
                        );
                        None
                    }
                })
                .collect()
        }
    };

    let valid_count = segments.len().min(embeddings.len());
    segments.truncate(valid_count);
    segments
        .into_iter()
        .zip(embeddings.into_iter().take(valid_count))
        .filter_map(|(seg, emb)| {
            if emb.len() == embedder.input_dim() {
                Some((seg, emb))
            } else {
                warn!("Skipping embedding with mismatched dimension");
                None
            }
        })
        .unzip()
}

fn run_chunked_polyvoice_diarization(
    diarizer: &PolyvoiceDiarizer,
    samples: &[f32],
    sample_rate: u32,
    config: &DiarizationConfig,
) -> Result<
    (
        Vec<DiarizationSegment>,
        Vec<ClusteredEmbedding>,
        StageTimings,
    ),
    String,
> {
    let mut timings = StageTimings::default();
    let chunk_duration = config.chunk_duration_secs();
    let chunks = channel_chunks(
        samples,
        sample_rate,
        chunk_duration,
        config.chunk_overlap_secs,
    );

    info!(
        "Chunked diarization: {} chunks ({}s duration, {}s overlap)",
        chunks.len(),
        chunk_duration,
        config.chunk_overlap_secs
    );

    let mut all_segments: Vec<DiarizationSegment> = Vec::new();
    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
    let mut had_raw_segments = false;

    for (chunk_idx, (chunk_start_seconds, chunk_samples)) in chunks.iter().enumerate() {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let diar_samples: std::borrow::Cow<'_, [f32]> = if sample_rate != DIARIZATION_SAMPLE_RATE {
            crate::audio::audio_processing::resample(
                chunk_samples,
                sample_rate,
                DIARIZATION_SAMPLE_RATE,
            )
            .map_err(|e| format!("Resampling failed for chunk {}: {}", chunk_idx, e))?
            .into()
        } else {
            std::borrow::Cow::Borrowed(chunk_samples)
        };

        let seg_start = Instant::now();
        let raw_segments = match diarizer.segmenter.segment(&diar_samples) {
            Ok(segments) => segments,
            Err(e) => {
                warn!(
                    "Segmentation failed for chunk {} ({}), skipping chunk",
                    chunk_idx, e
                );
                continue;
            }
        };
        timings.segmentation_secs += seg_start.elapsed().as_secs_f64();

        if raw_segments.is_empty() {
            continue;
        }
        had_raw_segments = true;

        let embed_start = Instant::now();
        let (mut chunk_segments, chunk_embeddings) = embed_segments(
            diarizer.embedder.as_ref(),
            &diar_samples,
            &raw_segments,
            config,
        );
        // Distinguish layout/shape errors from transient failures in logs.
        if chunk_segments.is_empty() && !raw_segments.is_empty() {
            // `embed_segments` already warned per-batch/per-segment; surface layout hint.
            warn!(
                "Chunk {}: segmentation found {} raw segments but embedding produced 0 valid vectors (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large)",
                chunk_idx,
                raw_segments.len()
            );
        }
        timings.embedding_secs += embed_start.elapsed().as_secs_f64();

        // Adjust segment times so they are relative to the full channel.
        for seg in &mut chunk_segments {
            seg.start += *chunk_start_seconds;
            seg.end += *chunk_start_seconds;
        }

        all_segments.extend(chunk_segments);
        all_embeddings.extend(chunk_embeddings);
    }

    if all_segments.is_empty() {
        if had_raw_segments {
            return Err(
                "Embedding produced zero valid vectors (audio_signal layout mismatch — expected [B,80,T] for titanet_large, but no embeddings survived; check TitaNet layout)".to_string(),
            );
        }
        return Ok((Vec::new(), Vec::new(), timings));
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        return Err("Diarization cancelled".to_string());
    }

    let cluster_start = Instant::now();
    let labels = diarizer
        .clusterer
        .cluster(&all_embeddings)
        .map_err(|e| format!("Speaker clustering failed: {}", e))?;
    timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

    for (segment, label) in all_segments.iter_mut().zip(labels) {
        segment.speaker = label as i32;
    }

    // Capture clustered embeddings while segments and embeddings are still
    // aligned (the sort below reshuffles segments only).
    let clustered: Vec<ClusteredEmbedding> = all_segments
        .iter()
        .zip(all_embeddings.iter())
        .map(|(s, e)| ClusteredEmbedding {
            speaker: s.speaker,
            embedding: e.clone(),
            duration_secs: (s.end - s.start).max(0.0),
            start_secs: Some(s.start),
            end_secs: Some(s.end),
        })
        .collect();

    all_segments.sort_by(|a, b| a.start.total_cmp(&b.start));

    info!(
        "Chunked diarization found {} segments with {} unique speakers",
        all_segments.len(),
        count_unique_speakers(&all_segments)
    );

    Ok((all_segments, clustered, timings))
}

fn channel_chunks(
    samples: &[f32],
    sample_rate: u32,
    chunk_duration_secs: f32,
    overlap_secs: f32,
) -> Vec<(f32, Vec<f32>)> {
    if samples.is_empty() || chunk_duration_secs <= 0.0 {
        return Vec::new();
    }

    let chunk_samples = (chunk_duration_secs * sample_rate as f32) as usize;
    let overlap_samples = (overlap_secs * sample_rate as f32).max(0.0) as usize;
    let step = chunk_samples.saturating_sub(overlap_samples).max(1);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < samples.len() {
        let end = (start + chunk_samples).min(samples.len());
        let chunk = samples[start..end].to_vec();
        let chunk_start_seconds = start as f32 / sample_rate as f32;
        chunks.push((chunk_start_seconds, chunk));
        if end == samples.len() {
            break;
        }
        start += step;
        // Avoid generating a tiny trailing sliver; extend the last chunk instead.
        if start + step >= samples.len() && start < samples.len() {
            // Last iteration will grab [start..end].
        }
    }
    chunks
}

// ===== ffmpeg streaming decode =====

/// A spawned ffmpeg process streaming 16 kHz mono f32le PCM on stdout.
struct PcmStream {
    child: Child,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl PcmStream {
    /// Wait for the process to exit and return an error if it failed.
    fn finish(mut self) -> Result<(), String> {
        let status = self
            .child
            .wait()
            .map_err(|e| format!("Failed to wait for ffmpeg: {}", e))?;
        if !status.success() {
            let stderr = self.stderr.lock().unwrap();
            return Err(format!(
                "ffmpeg exited with {}: {}",
                status,
                String::from_utf8_lossy(&stderr)
            ));
        }
        Ok(())
    }

    /// Kill the process (used on cancellation).
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PcmStream {
    fn drop(&mut self) {
        let still_running = self.child.try_wait().map(|o| o.is_none()).unwrap_or(false);
        if still_running {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Read up to `count` little-endian f32 samples from `reader`, appending them
/// to `out`. Returns the number of samples appended (0 means EOF).
fn read_f32_le(reader: &mut impl Read, out: &mut Vec<f32>, count: usize) -> Result<usize, String> {
    let start = out.len();
    let mut byte_buf = [0u8; 16384];
    while out.len() - start < count {
        let remaining = count - (out.len() - start);
        let max_bytes = (remaining * 4).min(byte_buf.len());
        let n = reader
            .read(&mut byte_buf[..max_bytes])
            .map_err(|e| format!("Failed to read PCM stream: {}", e))?;
        if n == 0 {
            break;
        }
        for b in byte_buf[..n].chunks_exact(4) {
            out.push(f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        }
    }
    Ok(out.len() - start)
}

/// Spawn ffmpeg to decode `input_path` to 16 kHz mono f32le PCM on stdout.
/// `channel: Some(0)` selects the left channel, `Some(1)` the right channel,
/// and `None` downmixes to mono.
fn spawn_ffmpeg_pcm(
    ffmpeg_path: &Path,
    input_path: &Path,
    channel: Option<u32>,
) -> Result<PcmStream, String> {
    let input_str = input_path
        .to_str()
        .ok_or_else(|| "Invalid audio path (non-UTF8)".to_string())?;

    let mut cmd = Command::new(ffmpeg_path);
    cmd.args(["-hide_banner", "-nostats", "-loglevel", "error"])
        .arg("-i")
        .arg(input_str)
        .arg("-vn");
    match channel {
        Some(0) => {
            cmd.args(["-af", "pan=mono|c0=c0"]);
        }
        Some(1) => {
            cmd.args(["-af", "pan=mono|c0=c1"]);
        }
        Some(_) => {
            return Err("Invalid channel index for ffmpeg streaming".to_string());
        }
        None => {
            cmd.args(["-ac", "1"]);
        }
    }
    cmd.args(["-ar", "16000", "-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg stdout was not captured".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ffmpeg stderr was not captured".to_string())?;

    // Drain stderr in a background thread so the pipe cannot fill and deadlock
    // the ffmpeg process.
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf_clone = Arc::clone(&stderr_buf);
    std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        *stderr_buf_clone.lock().unwrap() = buf;
    });

    Ok(PcmStream {
        child,
        stdout,
        stderr: stderr_buf,
    })
}

/// Yields overlapping in-memory windows of 16 kHz f32 PCM read from a stream.
/// Windows overlap by `overlap_samples`, and the final (partial) window ends at
/// the stream's end. Only the trailing overlap is retained between calls, so
/// peak memory stays at one window plus the overlap carry.
struct StreamWindows {
    chunk_samples: usize,
    overlap_samples: usize,
    step_samples: usize,
    carry: Vec<f32>,
    start_seconds: f32,
    first: bool,
    done: bool,
}

impl StreamWindows {
    fn new(chunk_samples: usize, overlap_samples: usize) -> Self {
        Self {
            chunk_samples,
            overlap_samples,
            step_samples: chunk_samples.saturating_sub(overlap_samples).max(1),
            carry: Vec::new(),
            start_seconds: 0.0,
            first: true,
            done: false,
        }
    }

    fn next_from(&mut self, reader: &mut impl Read) -> Result<Option<(f32, Vec<f32>)>, String> {
        if self.done {
            return Ok(None);
        }

        let mut window;
        if self.first {
            window = Vec::with_capacity(self.chunk_samples);
            read_f32_le(reader, &mut window, self.chunk_samples)?;
            self.first = false;
        } else {
            window = std::mem::take(&mut self.carry);
            let before = window.len();
            read_f32_le(reader, &mut window, self.step_samples)?;
            self.start_seconds += self.step_samples as f32 / DIARIZATION_SAMPLE_RATE as f32;
            if window.len() == before {
                self.done = true;
                return Ok(None);
            }
        }

        if window.is_empty() {
            self.done = true;
            return Ok(None);
        }

        // Retain the trailing overlap for the next window.
        let carry_start = window.len().saturating_sub(self.overlap_samples);
        self.carry = window[carry_start..].to_vec();

        if window.len() < self.chunk_samples {
            self.done = true;
        }
        Ok(Some((self.start_seconds, window)))
    }
}

/// Diarize a channel streamed from ffmpeg (already 16 kHz), reading overlapping
/// in-memory windows and clustering all accumulated embeddings globally.
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
    let mut timings = StageTimings::default();
    let chunk_samples = (config.chunk_duration_secs() * DIARIZATION_SAMPLE_RATE as f32) as usize;
    let overlap_samples = (config.chunk_overlap_secs * DIARIZATION_SAMPLE_RATE as f32) as usize;

    let mut all_segments: Vec<DiarizationSegment> = Vec::new();
    let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
    let mut had_raw_segments = false;

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

        let seg_start = Instant::now();
        let raw_segments = match diarizer.segmenter.segment(&window) {
            Ok(segments) => segments,
            Err(e) => {
                warn!(
                    "Segmentation failed for a stream window ({}), skipping it",
                    e
                );
                continue;
            }
        };
        timings.segmentation_secs += seg_start.elapsed().as_secs_f64();

        if !raw_segments.is_empty() {
            had_raw_segments = true;
            let embed_start = Instant::now();
            let (mut chunk_segments, chunk_embeddings) =
                embed_segments(diarizer.embedder.as_ref(), &window, &raw_segments, config);
            if chunk_segments.is_empty() {
                warn!(
                    "Stream window: segmentation found {} raw segments but embedding produced 0 valid vectors (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large)",
                    raw_segments.len()
                );
            }
            timings.embedding_secs += embed_start.elapsed().as_secs_f64();

            for seg in &mut chunk_segments {
                seg.start += window_start_seconds;
                seg.end += window_start_seconds;
            }

            all_segments.extend(chunk_segments);
            all_embeddings.extend(chunk_embeddings);
        }
    }

    if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
        pcm.kill();
        return Err("Diarization cancelled".to_string());
    }

    pcm.finish()?;

    if all_segments.is_empty() {
        if had_raw_segments {
            return Err(
                "Embedding produced zero valid vectors (audio_signal layout mismatch — expected [B,80,T] for titanet_large, but no embeddings survived; check TitaNet layout)".to_string(),
            );
        }
        return Ok((Vec::new(), Vec::new(), timings));
    }

    let cluster_start = Instant::now();
    let labels = diarizer
        .clusterer
        .cluster(&all_embeddings)
        .map_err(|e| format!("Speaker clustering failed: {}", e))?;
    timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

    for (segment, label) in all_segments.iter_mut().zip(labels) {
        segment.speaker = label as i32;
    }

    // Capture clustered embeddings while segments and embeddings are still
    // aligned (the sort below reshuffles segments only).
    let clustered: Vec<ClusteredEmbedding> = all_segments
        .iter()
        .zip(all_embeddings.iter())
        .map(|(s, e)| ClusteredEmbedding {
            speaker: s.speaker,
            embedding: e.clone(),
            duration_secs: (s.end - s.start).max(0.0),
            start_secs: Some(s.start),
            end_secs: Some(s.end),
        })
        .collect();

    all_segments.sort_by(|a, b| a.start.total_cmp(&b.start));

    info!(
        "Streamed diarization found {} segments with {} unique speakers",
        all_segments.len(),
        count_unique_speakers(&all_segments)
    );

    Ok((all_segments, clustered, timings))
}

fn count_unique_speakers(segments: &[DiarizationSegment]) -> usize {
    let mut speakers: Vec<i32> = segments.iter().map(|s| s.speaker).collect();
    speakers.sort();
    speakers.dedup();
    speakers.len()
}

/// Group a channel's clustered embeddings by cluster id, computing the
/// L2-normalized centroid (mean of member embeddings) and a bounded set of
/// exemplar embeddings (top by duration) for each cluster.
fn group_cluster_embeddings(
    embeddings: &[ClusteredEmbedding],
) -> Vec<(i32, Vec<f32>, Vec<Exemplar>)> {
    let mut by_cluster: HashMap<i32, Vec<&ClusteredEmbedding>> = HashMap::new();
    for e in embeddings {
        by_cluster.entry(e.speaker).or_default().push(e);
    }

    let mut out = Vec::new();
    for (spk, items) in by_cluster {
        let dim = items.first().map(|e| e.embedding.len()).unwrap_or(0);
        let mut centroid = vec![0.0f32; dim];
        for it in &items {
            for (i, x) in it.embedding.iter().enumerate() {
                centroid[i] += x;
            }
        }
        let n = items.len().max(1) as f32;
        for c in centroid.iter_mut() {
            *c /= n;
        }
        l2_normalize_in_place(&mut centroid);

        let mut sorted: Vec<&ClusteredEmbedding> = items.clone();
        sorted.sort_by(|a, b| {
            b.duration_secs
                .partial_cmp(&a.duration_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let exemplars: Vec<Exemplar> = sorted
            .iter()
            .take(MAX_CLUSTER_CACHE_EXEMPLARS)
            .map(|e| Exemplar {
                embedding: e.embedding.clone(),
                duration_secs: e.duration_secs as f64,
                start_secs: e.start_secs,
                end_secs: e.end_secs,
            })
            .collect();

        out.push((spk, centroid, exemplars));
    }
    out
}

/// Persist each cluster's centroid + exemplar cache for one channel, then
/// auto-assign recognized speakers from the provided prototypes. User
/// bindings are preserved. `prototypes` is pre-loaded by the caller. All
/// rows and recognition use the enhanced `titanet_large` family.
async fn persist_channel_clusters(
    pool: &SqlitePool,
    meeting_id: &str,
    embeddings: &[ClusteredEmbedding],
    prefix: &str,
    channel: &str,
    prototypes: &[Prototype],
) -> Result<(), String> {
    let clusters = group_cluster_embeddings(embeddings);
    for (spk, centroid, exemplars) in clusters {
        let label = format!("{}_{:02}", prefix, spk);
        SpeakerRepository::write_cluster_cache(
            pool,
            meeting_id,
            &label,
            channel,
            &centroid,
            &exemplars,
            crate::audio::embedder::ENHANCED_MODEL_TAG,
        )
        .await
        .map_err(|e| format!("Failed to persist cluster cache: {}", e))?;

        let threshold = crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD;
        if let Some(m) = crate::audio::speaker_recognition::best_match_with_threshold(
            &centroid,
            Some(channel),
            prototypes,
            threshold,
        ) {
            SpeakerRepository::set_auto_binding_if_unbound(
                pool,
                meeting_id,
                &label,
                &m.speaker_id,
                m.score as f64,
            )
            .await
            .map_err(|e| format!("Failed to auto-assign speaker: {}", e))?;
        }
    }
    Ok(())
}

/// Persist per-cluster centroids + exemplar caches for both channels and
/// auto-assign recognized speakers. The expected-speaker allowlist (or all
/// speakers when empty) constrains candidates; prototypes are loaded once
/// for the enhanced `titanet_large` family. Used by both the offline
/// diarization path and the online recording stop-time finalize.
pub async fn persist_and_recognize_session(
    pool: &SqlitePool,
    meeting_id: &str,
    mic: &[ClusteredEmbedding],
    sys: &[ClusteredEmbedding],
    is_stereo: bool,
) -> Result<(), String> {
    if mic.is_empty() && sys.is_empty() {
        return Ok(());
    }

    let expected = SpeakerRepository::get_expected_speakers(pool, meeting_id)
        .await
        .map_err(|e| format!("Failed to load expected speakers: {}", e))?;
    let candidates: Option<&[String]> = if expected.is_empty() {
        None
    } else {
        Some(&expected)
    };
    let prototypes: Vec<Prototype> = SpeakerRepository::load_prototypes(
        pool,
        candidates,
        crate::audio::embedder::ENHANCED_MODEL_TAG,
    )
    .await
    .map_err(|e| format!("Failed to load prototypes: {}", e))?
    .into_iter()
    .map(Prototype::from)
    .collect();

    let mic_prefix = if is_stereo { "MIC_SPEAKER" } else { "SPEAKER" };
    persist_channel_clusters(pool, meeting_id, mic, mic_prefix, "mic", &prototypes).await?;
    if is_stereo {
        persist_channel_clusters(pool, meeting_id, sys, "SPEAKER", "system", &prototypes).await?;
    }
    Ok(())
}

fn compute_speaker_matches<R: Runtime>(
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

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    status: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

// ===== Memory sampler =====

struct MemorySampler {
    peak_bytes: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl MemorySampler {
    fn start() -> Self {
        let peak_bytes = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let peak_bytes_clone = peak_bytes.clone();
        let running_clone = running.clone();

        let handle = std::thread::spawn(move || {
            let pid = match sysinfo::get_current_pid() {
                Ok(pid) => pid,
                Err(_) => return,
            };
            let mut system = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new().with_memory()),
            );
            let refresh_kind = ProcessRefreshKind::new().with_memory();
            while running_clone.load(Ordering::Relaxed) {
                let _ = system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    false,
                    refresh_kind,
                );
                if let Some(proc) = system.process(pid) {
                    let mem = proc.memory();
                    let mut current = peak_bytes_clone.load(Ordering::Relaxed);
                    while mem > current {
                        match peak_bytes_clone.compare_exchange_weak(
                            current,
                            mem,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(c) => current = c,
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            peak_bytes,
            running,
            handle: Some(handle),
        }
    }

    fn stop(mut self) -> u64 {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.peak_bytes.load(Ordering::Relaxed) / 1024 / 1024
    }
}

// ===== Model management (enhanced set bundled at build time) =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiarizationModelStatus {
    pub segmentation_ready: bool,
    pub embedding_ready: bool,
    #[serde(default)]
    pub ready: bool,
}

/// Removes stale model files that are no longer used: sherpa-era artifacts and
/// the standard polyvoice set (`powerset_int8`/`resnet34_int8`).
fn cleanup_legacy_models(models_dir: &std::path::Path) {
    let legacy: Vec<PathBuf> = [
        models_dir.join("sherpa-onnx-pyannote-segmentation-3-0"),
        models_dir.join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"),
        models_dir.join("powerset_int8.onnx"),
        models_dir.join("resnet34_int8.onnx"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect();

    for path in legacy {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
        info!("Removed stale diarization model file: {}", path.display());
    }
}

#[tauri::command]
pub async fn check_diarization_models<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DiarizationModelStatus, String> {
    let models_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models");

    cleanup_legacy_models(&models_dir);

    // Delegate to the shared 3-location resolver so Settings and engine never disagree.
    if let Some(resolved) = crate::audio::embedder::resolve_enhanced_models_dir(&app) {
        let (seg_ok, emb_ok) = crate::audio::embedder::verify_enhanced_integrity(&resolved);
        return Ok(DiarizationModelStatus {
            segmentation_ready: seg_ok,
            embedding_ready: emb_ok,
            ready: true,
        });
    }
    // No single location has both files — compute per-file OR across candidates for UI granularity,
    // but `ready` remains false because engine requires both in the same directory.
    let candidates = vec![
        models_dir.clone(),
        app.path()
            .resource_dir()
            .map(|p| p.join("models"))
            .unwrap_or_else(|_| models_dir.clone()),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models"),
    ];
    let mut segmentation_ready = false;
    let mut embedding_ready = false;
    for dir in &candidates {
        let (seg_ok, emb_ok) = crate::audio::embedder::verify_enhanced_integrity(dir);
        segmentation_ready |= seg_ok;
        embedding_ready |= emb_ok;
    }
    Ok(DiarizationModelStatus {
        segmentation_ready,
        embedding_ready,
        ready: false,
    })
}

// ===== Spike: polyvoice diarization engine verification (change: switch-to-polyvoice-diarization) =====
//
// polyvoice is the sole diarization engine: enhanced segmentation-3.0 +
// TitaNet-Large embedding + AHC clustering (offline), and StreamingPipeline
// with the same embedder (online). Tests skip gracefully when the models are
// not present (e.g. offline CI).

#[cfg(test)]
mod spike_tests {
    use super::*;
    use polyvoice::clusterer::Clusterer as _;
    use polyvoice::embedder::Embedder as _;

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

    #[test]
    fn chunk_splitting_preserves_overlap_and_offsets() {
        let sample_rate = 16000;
        let samples: Vec<f32> = (0..sample_rate * 30).map(|i| (i as f32).sin()).collect();
        let chunks = channel_chunks(&samples, sample_rate, 10.0, 5.0);

        assert!(!chunks.is_empty());
        // Every chunk except the last should be a full 10-second window.
        for (i, (_, chunk)) in chunks
            .iter()
            .enumerate()
            .take(chunks.len().saturating_sub(1))
        {
            assert_eq!(
                chunk.len(),
                sample_rate as usize * 10,
                "chunk {} has wrong size",
                i
            );
        }

        // Adjacent chunks should overlap by 5 seconds.
        for window in chunks.windows(2) {
            let start_a = window[0].0;
            let start_b = window[1].0;
            let diff = (start_b - start_a - 5.0).abs();
            assert!(
                diff < 0.01,
                "expected 5s overlap, got diff {}s",
                start_b - start_a
            );
        }

        // Last chunk should reach the end of the input.
        let (_, last_chunk) = chunks.last().unwrap();
        let last_start = chunks.last().unwrap().0;
        assert_eq!(
            (last_start * sample_rate as f32) as usize + last_chunk.len(),
            samples.len(),
            "last chunk must reach the end of the input"
        );
    }

    /// A test embedder that returns a deterministic vector whose first element
    /// identifies the input slice length. This lets us verify that
    /// `embed_batch` preserves ordering and that the fallback path handles
    /// mismatched dimensions gracefully.
    struct OrderedTestEmbedder {
        dim: usize,
    }

    impl polyvoice::embedder::Embedder for OrderedTestEmbedder {
        fn dim(&self) -> usize {
            self.dim
        }

        fn embed(&self, audio: &[f32]) -> Result<Vec<f32>, polyvoice::embedder::EmbedderError> {
            let mut v = vec![0.0f32; self.dim];
            if let Some(first) = v.first_mut() {
                *first = audio.len() as f32;
            }
            Ok(v)
        }

        fn embed_batch(
            &self,
            audios: &[&[f32]],
        ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
            audios.iter().map(|a| self.embed(a)).collect()
        }
    }

    #[test]
    fn embed_batch_preserves_ordering_and_handles_errors() {
        let embedder = OrderedTestEmbedder { dim: 4 };
        let inputs: Vec<Vec<f32>> = vec![vec![1.0; 10], vec![2.0; 20], vec![3.0; 30]];
        let refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();

        let batch = embedder.embed_batch(&refs).expect("batch should succeed");
        assert_eq!(batch.len(), inputs.len());
        for (i, emb) in batch.iter().enumerate() {
            assert_eq!(
                emb[0],
                inputs[i].len() as f32,
                "embedding order mismatch at index {}",
                i
            );
        }

        // Empty batch should return an empty result, not an error.
        let empty: Vec<&[f32]> = Vec::new();
        assert!(embedder.embed_batch(&empty).unwrap().is_empty());
    }

    #[test]
    fn diarization_config_uses_fixed_profile() {
        let cfg = DiarizationConfig::default();
        assert!(cfg.max_sessions >= 1 && cfg.max_sessions <= 8);
        assert_eq!(cfg.chunk_overlap_secs, 5.0);
        assert_eq!(cfg.chunk_duration_secs(), 600.0);
    }

    #[test]
    fn fixed_pool_size_respects_floor_and_cap() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let expected = (((cores as f64) * 0.75).ceil() as usize).min(8).max(1);
        assert_eq!(fixed_pool_size(), expected);
        assert!(fixed_pool_size() >= 1);
        assert!(fixed_pool_size() <= 8);
    }

    #[test]
    fn read_f32_le_converts_little_endian_samples() {
        let mut data: Vec<u8> = Vec::new();
        for v in [1.0f32, -2.5, 0.0, 3.25] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);
        let mut out = Vec::new();
        let n = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n, 4);
        assert_eq!(out, vec![1.0, -2.5, 0.0, 3.25]);
    }

    #[test]
    fn read_f32_le_stops_at_eof() {
        let data = 0.5f32.to_le_bytes().to_vec();
        let mut cursor = std::io::Cursor::new(data);
        let mut out = Vec::new();
        let n = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n, 1);
        assert_eq!(out, vec![0.5]);
        let n2 = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n2, 0);
    }

    #[test]
    fn stream_windows_overlap_and_advance() {
        // 30 seconds of 16 kHz mono f32 = 480000 samples (value == sample index).
        let total = 480_000usize;
        let mut data = Vec::with_capacity(total * 4);
        for i in 0..total {
            data.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);

        let mut windows = StreamWindows::new(160_000, 80_000);
        let mut collected: Vec<(f32, Vec<f32>)> = Vec::new();
        while let Some(w) = windows.next_from(&mut cursor).expect("read") {
            collected.push(w);
        }

        // 10s chunks with 5s overlap -> 5s step over 30s: 5 full windows.
        assert_eq!(collected.len(), 5);
        let starts: Vec<f32> = collected.iter().map(|(s, _)| *s).collect();
        assert_eq!(starts, vec![0.0, 5.0, 10.0, 15.0, 20.0]);
        assert!(collected.iter().all(|(_, s)| s.len() == 160_000));
        // The second window begins in the overlap region, i.e. at the tail of
        // the first window (sample value == global sample index).
        assert_eq!(collected[0].1[80_000], 80_000.0);
        assert_eq!(collected[1].1[0], 80_000.0);
    }

    #[test]
    fn stream_windows_final_partial_window() {
        // 22 seconds = 352000 samples, not a multiple of the 5s step.
        let total = 352_000usize;
        let mut data = Vec::with_capacity(total * 4);
        for _ in 0..total {
            data.extend_from_slice(&0.0f32.to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);

        let mut windows = StreamWindows::new(160_000, 80_000);
        let mut collected = Vec::new();
        while let Some(w) = windows.next_from(&mut cursor).expect("read") {
            collected.push(w);
        }

        assert_eq!(collected.len(), 4);
        assert_eq!(collected[3].0, 15.0);
        assert_eq!(collected[3].1.len(), 112_000);
    }
}
