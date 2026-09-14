use crate::audio::audio_file::find_audio_file;
use crate::audio::decoder::{
    convert_to_wav_with_ffmpeg, decode_audio_file, detect_channel_layout, needs_ffmpeg_conversion,
    ChannelLayout,
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

/// Built-in default speaker-count ceiling: sweep-selected (extended grid,
/// 2026-09-04) — tight ceilings force below-threshold merges and inflate
/// confusion, so the ceiling sits well above real meeting speaker counts while
/// still guaranteeing the clusterer never runs unbounded.
pub const DEFAULT_CLUSTER_CEILING: usize = 128;

/// Built-in default same-speaker gap-merge window (sweep-selected, 2026-09-04).
pub const DEFAULT_GAP_MERGE_SECS: f32 = 0.3;

/// Pipeline's clustering-backend maximum speaker count (`u8` local labels).
const MAX_CLUSTERERS: usize = 255;

/// Dense embedding window length (seconds, w/2 hop) — compiled-in constant
/// (pipeline-v2 D4), harness-sweepable via `--embed-window`, not a persisted
/// setting.
pub const DEFAULT_EMBED_WINDOW_SECS: f32 = 5.0;

/// Minimum segment length (seconds) accepted for embedding: sub-0.2 s windows
/// collapse the pooling std toward NaN on the TitaNet time-downsample path
/// (mirrors the vendored `MIN_EMBED_SECS`).
const MIN_EMBED_SECS: f64 = 0.20;

/// Calibrated binarization constants (pipeline-v2 D4): the vendored
/// `BinarizationConfig` default is plain thresholding (0.5/0.5/0/0), so the
/// spike selects hysteresis + min-duration smoothing constants; harness-
/// sweepable via `--binarization`, not a persisted setting.
pub const DEFAULT_BINARIZATION: polyvoice::segmentation::BinarizationConfig =
    polyvoice::segmentation::BinarizationConfig {
        onset: 0.5,
        offset: 0.4,
        min_duration_on: 0.2,
        min_duration_off: 0.2,
    };

/// Built-in minimum turn duration (seconds) after resegmentation (mirrors
/// `PipelineConfig::min_speech_secs`).
pub const DEFAULT_MIN_SPEECH_SECS: f32 = 0.25;

/// Offline clusterer kind (diarization-param-tuning, pipeline-v2 D2).
/// `ahc` is the built-in default (fixed cosine threshold, selected by the 6.2
/// sweep: NME-SC under-clusters dense TitaNet windows into ~1 speaker/file).
/// `nmesc` remains selectable (automatic count over cosine-affinity spectral
/// clustering, dimension-agnostic). `vbx` stays parseable but the clusterer
/// factory rejects it for the enhanced 192-d family (the vendored PLDA params
/// require 256-d embeddings) — no silent kind switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClustererKindSetting {
    Nmesc,
    Vbx,
    Ahc,
}

impl ClustererKindSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nmesc => "nmesc",
            Self::Vbx => "vbx",
            Self::Ahc => "ahc",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "nmesc" => Some(Self::Nmesc),
            "vbx" => Some(Self::Vbx),
            "ahc" => Some(Self::Ahc),
            _ => None,
        }
    }

    pub fn is_automatic_count(self) -> bool {
        matches!(self, Self::Nmesc | Self::Vbx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DiarizationConfig {
    pub max_sessions: usize,
    pub chunk_overlap_secs: f32,
    /// AHC merge criterion: minimum cosine similarity to merge two clusters.
    /// Applies to the `ahc` kind only (ignored under automatic count).
    pub cluster_threshold: f32,
    /// Hard ceiling on distinct speaker labels per channel per pass.
    pub cluster_ceiling: usize,
    /// Merge consecutive same-speaker segments when the gap between them is
    /// within this window (0 disables gap-merging).
    pub gap_merge_secs: f32,
    /// Clusterer kind (nmesc|vbx|ahc); default `ahc` (6.2 sweep).
    pub clusterer: ClustererKindSetting,
    /// Dense embedding window (0 = sparse one-embedding-per-segment).
    pub embed_window_secs: f32,
    /// Calibrated binarization of segmentation posteriors (None = argmax).
    pub binarization: Option<polyvoice::segmentation::BinarizationConfig>,
    /// Minimum output turn duration.
    pub min_speech_secs: f32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            max_sessions: fixed_pool_size(),
            chunk_overlap_secs: 5.0,
            cluster_threshold: crate::audio::embedder::TITANET_CLUSTER_THRESHOLD,
            cluster_ceiling: DEFAULT_CLUSTER_CEILING,
            gap_merge_secs: DEFAULT_GAP_MERGE_SECS,
            clusterer: ClustererKindSetting::Ahc,
            embed_window_secs: DEFAULT_EMBED_WINDOW_SECS,
            binarization: Some(DEFAULT_BINARIZATION),
            min_speech_secs: DEFAULT_MIN_SPEECH_SECS,
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

    /// App-path config: built-in defaults overlaid with persisted clustering
    /// settings (diarization-param-tuning D2). A stored override wins per key;
    /// unset keys fall back to the built-in defaults.
    pub fn resolved() -> Self {
        Self {
            cluster_threshold: stored_cluster_threshold()
                .unwrap_or(crate::audio::embedder::TITANET_CLUSTER_THRESHOLD),
            cluster_ceiling: stored_cluster_ceiling().unwrap_or(DEFAULT_CLUSTER_CEILING),
            gap_merge_secs: stored_gap_merge_secs().unwrap_or(DEFAULT_GAP_MERGE_SECS),
            clusterer: stored_clusterer_kind().unwrap_or(ClustererKindSetting::Ahc),
            ..Self::default()
        }
    }
}

// ===== Persisted clustering-settings holder (diarization-param-tuning D2) ====
//
// Mirrors `word_alignment::settings`: the frontend persists the keys in its
// settings store and mirrors them to the backend via
// `set_diarization_clustering_settings`; offline diarization reads them when
// building `DiarizationConfig`. The `diarize-eval` harness never consults
// these globals (D4: it measures defaults + explicit CLI flags).

static CLUSTER_THRESHOLD_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else f32 bits + 1
static CLUSTER_CEILING_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else value
static GAP_MERGE_SECS_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else f32 bits + 1
static CLUSTERER_KIND_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0 = unset

fn store_f32_option(cell: &AtomicU64, value: Option<f32>) {
    match value {
        Some(v) => cell.store((v.to_bits() as u64) + 1, Ordering::SeqCst),
        None => cell.store(0, Ordering::SeqCst),
    }
}

fn load_f32_option(cell: &AtomicU64) -> Option<f32> {
    let raw = cell.load(Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(f32::from_bits((raw - 1) as u32))
    }
}

fn stored_cluster_threshold() -> Option<f32> {
    load_f32_option(&CLUSTER_THRESHOLD_OVERRIDE)
}

fn stored_cluster_ceiling() -> Option<usize> {
    match CLUSTER_CEILING_OVERRIDE.load(Ordering::SeqCst) {
        0 => None,
        v => Some(v as usize),
    }
}

fn stored_gap_merge_secs() -> Option<f32> {
    load_f32_option(&GAP_MERGE_SECS_OVERRIDE)
}

fn kind_code(kind: ClustererKindSetting) -> u8 {
    match kind {
        ClustererKindSetting::Nmesc => 1,
        ClustererKindSetting::Vbx => 2,
        ClustererKindSetting::Ahc => 3,
    }
}

fn stored_clusterer_kind() -> Option<ClustererKindSetting> {
    match CLUSTERER_KIND_OVERRIDE.load(Ordering::SeqCst) {
        0 => None,
        1 => Some(ClustererKindSetting::Nmesc),
        2 => Some(ClustererKindSetting::Vbx),
        3 => Some(ClustererKindSetting::Ahc),
        _ => None,
    }
}

/// Update the persisted clustering overrides (None clears a key back to the
/// built-in default).
pub fn set_clustering_overrides(
    cluster_threshold: Option<f32>,
    cluster_ceiling: Option<usize>,
    gap_merge_secs: Option<f32>,
    clusterer: Option<ClustererKindSetting>,
) {
    store_f32_option(&CLUSTER_THRESHOLD_OVERRIDE, cluster_threshold);
    CLUSTER_CEILING_OVERRIDE.store(
        cluster_ceiling.map(|v| v as u64).unwrap_or(0),
        Ordering::SeqCst,
    );
    store_f32_option(&GAP_MERGE_SECS_OVERRIDE, gap_merge_secs);
    CLUSTERER_KIND_OVERRIDE
        .store(clusterer.map(kind_code).unwrap_or(0), Ordering::SeqCst);
    log::info!(
        "Diarization clustering settings updated: threshold={:?}, ceiling={:?}, gap_merge={:?}, clusterer={:?}",
        stored_cluster_threshold(),
        stored_cluster_ceiling(),
        stored_gap_merge_secs(),
        stored_clusterer_kind(),
    );
}

#[tauri::command]
pub async fn set_diarization_clustering_settings(
    cluster_threshold: Option<f32>,
    cluster_ceiling: Option<usize>,
    gap_merge_secs: Option<f32>,
    clusterer: Option<String>,
) -> Result<(), String> {
    let kind = match clusterer.as_deref() {
        None => None,
        Some(raw) => Some(ClustererKindSetting::parse(raw).ok_or_else(|| {
            format!(
                "Unknown diarizationClusterer '{raw}' (expected vbx|nmesc|ahc)"
            )
        })?),
    };
    set_clustering_overrides(cluster_threshold, cluster_ceiling, gap_merge_secs, kind);
    Ok(())
}

// ===== Blocking diarization orchestration =====

#[derive(Debug, Clone, Copy, Default)]
struct StageTimings {
    decode_secs: f64,
    segmentation_secs: f64,
    embedding_secs: f64,
    clustering_secs: f64,
    resegmentation_secs: f64,
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

    let peak_mb = memory_sampler.stop();
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

fn run_channel_diarization(
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

#[derive(Debug, Clone)]
pub struct DiarizationSegment {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
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
pub struct ChannelClusters {
    pub segments: Vec<DiarizationSegment>,
    pub embeddings: Vec<ClusteredEmbedding>,
}

/// Polyvoice diarization engine (pipeline_v2 architecture): enhanced
/// segmentation-3.0 with calibrated binarization + TitaNet-Large embedding +
/// kind-selected clusterer + overlap resegmenter, loaded once per run.
pub struct PolyvoiceDiarizer {
    segmenter: Box<dyn polyvoice::segmentation::Segmenter>,
    embedder: Box<dyn crate::audio::embedder::SpeakerEmbedder>,
    clusterer: Box<dyn polyvoice::clusterer::Clusterer>,
    resegmenter: polyvoice::resegmentation::OverlapResegmenter,
}

/// Build the offline clusterer for the resolved kind. The effective ceiling is
/// clamped to the pipeline's 255 maximum (logged). `vbx` is gated: the vendored
/// PLDA parameters require 256-d embeddings while the enhanced TitaNet-Large
/// family is 192-d, so selecting it errors actionably — no silent kind switch.
fn build_clusterer(
    config: &DiarizationConfig,
    max_clusters: usize,
) -> Result<Box<dyn polyvoice::clusterer::Clusterer>, String> {
    let ceiling = max_clusters.min(MAX_CLUSTERERS);
    if ceiling < max_clusters {
        warn!(
            "Speaker-count ceiling {} exceeds the clustering backend maximum; clamped to {}",
            max_clusters, ceiling
        );
    }
    match config.clusterer {
        ClustererKindSetting::Ahc => {
            log::info!(
                "Diarization clusterer: ahc (threshold={:.3}, ceiling={})",
                config.cluster_threshold,
                ceiling
            );
            Ok(Box::new(polyvoice::clusterer::AhcClusterer::with_threshold(
                ceiling,
                config.cluster_threshold,
            )))
        }
        ClustererKindSetting::Nmesc => {
            log::info!(
                "Diarization clusterer: nmesc (automatic count, ceiling={}; merge threshold inert)",
                ceiling
            );
            Ok(Box::new(polyvoice::clusterer::NmeScClusterer::new(ceiling)))
        }
        ClustererKindSetting::Vbx => Err(format!(
            "The 'vbx' clusterer is unavailable for the enhanced diarization model set: \
             VBx requires 256-dimensional embeddings (the vendored PLDA parameters are \
              dimension-locked), while the bundled TitaNet-Large family embeds 192-d. \
              Select 'ahc' (default) or 'nmesc' instead."
        )),
    }
}

fn create_polyvoice_diarizer(
    models_dir: &PathBuf,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    // Enhanced-only engine family: segmentation and embedding construction
    // error with clear messages when the bundled enhanced models are absent
    // (no fallback to a standard/legacy model set).
    let segmenter = crate::audio::segmentation::create_v2_segmenter(
        models_dir,
        config.segmenter_pool_size(),
        config.binarization,
    )?;
    let embedder =
        crate::audio::embedder::create_speaker_embedder(models_dir, config.embedder_pool_size())
            .map_err(|e| format!("Failed to create embedder: {}", e))?;
    let model_tag = embedder.model_tag();

    // Always-on ceiling: user max_speakers when smaller, else the configured
    // default ceiling. Clamped to >= 1 so the clusterer never runs unbounded,
    // and to the backend's 255 maximum inside `build_clusterer`.
    let user_max = max_speakers.filter(|m| *m > 0).unwrap_or(i32::MAX) as usize;
    let max_clusters = user_max.min(config.cluster_ceiling).max(1);
    let clusterer = build_clusterer(config, max_clusters)?;
    log::info!(
        "Diarizer using enhanced family tag={} embed_window={:.1}s kind={}",
        model_tag,
        config.embed_window_secs,
        config.clusterer.as_str(),
    );

    Ok(PolyvoiceDiarizer {
        segmenter,
        embedder,
        clusterer,
        resegmenter: polyvoice::resegmentation::OverlapResegmenter::default(),
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

// ===== Tauri/DB-free diarization core (shared with the `diarize-eval` bin) =====

/// Candidate model directories without an `AppHandle`, mirroring the app's
/// 3-location fallback: app data dir → resource dir near the executable →
/// dev manifest dir. An explicit override (CLI flag / env) is prepended.
fn standalone_model_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = explicit {
        // Explicit override is strict: no fallback locations are searched.
        dirs.push(dir.to_path_buf());
        return dirs;
    }
    if let Ok(dir) = std::env::var("MEETILY_MODELS_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    // 1. app_data_dir/models (same identifier the Tauri resolver uses)
    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("com.meetily.ai").join("models"));
    }
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.meetily.ai")
                .join("models"),
        );
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(xdg).join("com.meetily.ai").join("models"));
        } else if let Ok(home) = std::env::var("HOME") {
            dirs.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("com.meetily.ai")
                    .join("models"),
            );
        }
    }
    // 2. resource dir near the executable (bundled install layout)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("resources").join("models"));
            dirs.push(parent.join("models"));
        }
    }
    // 3. dev manifest fallback (`cargo run`/`cargo build` from src-tauri)
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"));
    dirs
}

/// Resolve the enhanced models directory without an `AppHandle`. When
/// `explicit` is given it must verify on its own (no fallback); otherwise the
/// standalone 3-location fallback applies. Errors name the missing model
/// files and every searched location.
pub fn resolve_models_dir_standalone(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let candidates = standalone_model_candidates(explicit);
    if let Some(dir) = crate::audio::embedder::resolve_enhanced_models_dir_from_paths(&candidates) {
        return Ok(dir);
    }
    Err(format!(
        "Enhanced diarization models not found ({} and {}). Searched: {}",
        crate::audio::embedder::ENHANCED_SEGMENTATION_FILE,
        crate::audio::embedder::ENHANCED_EMBEDDING_FILE,
        crate::audio::embedder::format_search_locations_from_paths(&candidates),
    ))
}

/// Create the production diarizer without an `AppHandle`, using an explicit
/// models directory or the standalone 3-location fallback.
pub fn create_diarizer_standalone(
    models_dir: Option<&Path>,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    let dir = resolve_models_dir_standalone(models_dir)?;
    create_polyvoice_diarizer(&dir, max_speakers, config)
}

/// Tauri/DB-free entry point: diarize one channel of in-memory samples with
/// the same chunked pipeline the app uses for offline diarization.
pub fn diarize_wav_samples(
    samples: &[f32],
    sample_rate: u32,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
    models_dir: Option<&Path>,
) -> Result<ChannelClusters, String> {
    let diarizer = create_diarizer_standalone(models_dir, max_speakers, config)?;
    let (segments, embeddings, _) =
        run_channel_diarization(&diarizer, samples, sample_rate, config, "mono")?;
    Ok(ChannelClusters {
        segments,
        embeddings,
    })
}

const DIARIZATION_SAMPLE_RATE: u32 = 16000;

type TimeRange = polyvoice::types::TimeRange;
type RawSegment = polyvoice::segmentation::RawSegment;
type SpeakerTurn = polyvoice::types::SpeakerTurn;

/// One dense embedding unit: a window slice of a primary segment, tagged with
/// its chunk-local time span, the segment's local speaker index, and the index
/// of its parent segment in the chunk's primary list.
#[derive(Debug, Clone)]
struct DenseUnit {
    time: TimeRange,
    local_idx: u8,
    segment_idx: usize,
    embedding: Vec<f32>,
}

/// Per-chunk accumulation between chunks (bounded): segment metadata with
/// chunk-local times plus the embedded units. `start_secs` offsets to global
/// channel time.
#[derive(Debug, Default)]
struct ChunkRecord {
    start_secs: f32,
    primary: Vec<RawSegment>,
    overlaps: Vec<(TimeRange, u8, u8)>,
    units: Vec<DenseUnit>,
    /// Mixed-voice embeddings (L2-normalized) precomputed during the chunk
    /// pass for overlap regions where at least one local speaker never
    /// appeared as a primary segment — the resegmentation fallback needs them
    /// after global clustering, when the chunk audio buffer is gone.
    mixed_overlaps: Vec<(TimeRange, Vec<f32>)>,
}

/// Expand primary segments into embedding units. Segments longer than `window`
/// are split into `window`-second sub-windows hopped by `window/2` (dense,
/// v2-style); sub-window segments stay whole. Mirrors the vendored
/// `pipeline_v2::expand_embed_units` (private). `window <= 0` keeps one unit
/// per segment (sparse).
fn expand_embed_units(segs: &[RawSegment], window: f32) -> Vec<(TimeRange, u8, usize)> {
    let w = if window > 0.0 { window as f64 } else { 0.0 };
    let mut out = Vec::new();
    for (idx, seg) in segs.iter().enumerate() {
        if w <= 0.0 || seg.time.end - seg.time.start <= w {
            out.push((seg.time, seg.local_speaker_idx, idx));
            continue;
        }
        let hop = (w / 2.0).max(0.05);
        let mut t = seg.time.start;
        loop {
            let end = (t + w).min(seg.time.end);
            out.push((
                TimeRange {
                    start: t,
                    end,
                },
                seg.local_speaker_idx,
                idx,
            ));
            if end >= seg.time.end {
                break;
            }
            t += hop;
        }
    }
    out
}

/// Embed masked unit slices through the batched multi-core path, falling back
/// to per-item embedding when the batch call fails. Returns embeddings aligned
/// 1:1 with `masked` (`None` marks dropped units: fallback failures,
/// non-finite, or dimension-mismatched vectors).
fn embed_unit_slices(
    embedder: &dyn crate::audio::embedder::SpeakerEmbedder,
    masked: &[Vec<f32>],
) -> Vec<Option<Vec<f32>>> {
    let valid = |emb: Vec<f32>| {
        let ok = emb.len() == embedder.input_dim() && emb.iter().all(|v| v.is_finite());
        if !ok {
            warn!("Skipping invalid embedding (dimension or non-finite values)");
        }
        ok.then_some(emb)
    };
    let refs: Vec<&[f32]> = masked.iter().map(Vec::as_slice).collect();
    match embedder.embed_batch(&refs) {
        Ok(batch) => batch.into_iter().map(valid).collect(),
        Err(e) => {
            warn!(
                "Batch embedding failed ({}), falling back to per-segment embedding",
                e
            );
            masked
                .iter()
                .map(|audio| {
                    embedder
                        .embed(audio)
                        .map_err(|e| {
                            warn!(
                                "Embedding extraction failed for a segment ({}), skipping it",
                                e
                            );
                            e
                        })
                        .ok()
                        .and_then(valid)
                })
                .collect()
        }
    }
}

/// The pipeline_v2 chunked core: per-chunk binarized segmentation + dense
/// embedding accumulation, then global clustering, per-chunk Hungarian
/// local→global mapping, overlap-aware two-speaker resegmentation, minimum-
/// speech filtering, and gap-fill. Bounded memory: only embeddings, segment
/// metadata, and small overlap embeddings accumulate across chunks.
struct V2Core<'a> {
    diarizer: &'a PolyvoiceDiarizer,
    config: &'a DiarizationConfig,
    chunks: Vec<ChunkRecord>,
    had_raw_segments: bool,
    timings: StageTimings,
}

impl<'a> V2Core<'a> {
    fn new(diarizer: &'a PolyvoiceDiarizer, config: &'a DiarizationConfig) -> Self {
        Self {
            diarizer,
            config,
            chunks: Vec::new(),
            had_raw_segments: false,
            timings: StageTimings::default(),
        }
    }

    /// Segment one 16 kHz chunk and embed its dense units (chunk-local times).
    fn process_chunk(&mut self, chunk_start_secs: f32, samples: &[f32]) -> Result<(), String> {
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let seg_start = Instant::now();
        let raw_segments = match self.diarizer.segmenter.segment(samples) {
            Ok(segments) => segments,
            Err(e) => {
                warn!("Segmentation failed for a chunk ({}), skipping chunk", e);
                return Ok(());
            }
        };
        self.timings.segmentation_secs += seg_start.elapsed().as_secs_f64();
        if raw_segments.is_empty() {
            return Ok(());
        }
        self.had_raw_segments = true;

        let overlaps = polyvoice::resegmentation::extract_overlap_time_ranges(&raw_segments);
        let primary: Vec<RawSegment> = raw_segments
            .iter()
            .filter(|s| !s.is_overlap)
            .cloned()
            .collect();

        let embed_start = Instant::now();
        let mut units: Vec<DenseUnit> = Vec::new();
        if !primary.is_empty() {
            let specs = expand_embed_units(&primary, self.config.embed_window_secs);
            let sample_rate = DIARIZATION_SAMPLE_RATE as f64;
            let mut masked: Vec<Vec<f32>> = Vec::with_capacity(specs.len());
            let mut kept: Vec<(TimeRange, u8, usize)> = Vec::with_capacity(specs.len());
            for (time, local_idx, segment_idx) in specs {
                let start_idx = (time.start * sample_rate) as usize;
                let end_idx = ((time.end * sample_rate) as usize).min(samples.len());
                if end_idx <= start_idx {
                    continue;
                }
                if (end_idx - start_idx) as f64 / sample_rate < MIN_EMBED_SECS {
                    continue;
                }
                // Zero-fill overlap regions inside the unit so two-speaker
                // audio cannot bias the embedding (v2 masking contract).
                let local_overlaps: Vec<(f32, f32)> = overlaps
                    .iter()
                    .filter_map(|(ot, _, _)| {
                        let lo = ot.start.max(time.start);
                        let hi = ot.end.min(time.end);
                        if hi > lo {
                            Some(((lo - time.start) as f32, (hi - time.start) as f32))
                        } else {
                            None
                        }
                    })
                    .collect();
                let chunk = polyvoice::embedder::apply_overlap_mask(
                    &samples[start_idx..end_idx],
                    &local_overlaps,
                    DIARIZATION_SAMPLE_RATE,
                );
                masked.push(chunk);
                kept.push((time, local_idx, segment_idx));
            }
            let embeddings = embed_unit_slices(self.diarizer.embedder.as_ref(), &masked);
            for ((time, local_idx, segment_idx), embedding) in kept.into_iter().zip(embeddings) {
                if let Some(embedding) = embedding {
                    units.push(DenseUnit {
                        time,
                        local_idx,
                        segment_idx,
                        embedding,
                    });
                }
            }
        }

        // Pre-embed mixed-voice overlap regions whose local speakers never
        // appear solo in this chunk (resegmentation fallback after clustering).
        let primary_locals: std::collections::HashSet<u8> =
            primary.iter().map(|s| s.local_speaker_idx).collect();
        let mut mixed_overlaps: Vec<(TimeRange, Vec<f32>)> = Vec::new();
        let unresolved: Vec<(TimeRange, u8, u8)> = overlaps
            .iter()
            .filter(|(_, lo, hi)| !primary_locals.contains(lo) || !primary_locals.contains(hi))
            .cloned()
            .collect();
        let sample_rate = DIARIZATION_SAMPLE_RATE as f64;
        let embeddable: Vec<(TimeRange, Vec<f32>)> = unresolved
            .into_iter()
            .filter_map(|(time, _, _)| {
                let start_idx = (time.start * sample_rate) as usize;
                let end_idx = ((time.end * sample_rate) as usize).min(samples.len());
                if end_idx > start_idx
                    && (end_idx - start_idx) as f64 / sample_rate >= MIN_EMBED_SECS
                {
                    Some((time, samples[start_idx..end_idx].to_vec()))
                } else {
                    None
                }
            })
            .collect();
        if !embeddable.is_empty() {
            let embeddings = embed_unit_slices(
                self.diarizer.embedder.as_ref(),
                &embeddable.iter().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
            );
            for ((time, _), emb) in embeddable.into_iter().zip(embeddings) {
                if let Some(mut emb) = emb {
                    polyvoice::utils::l2_normalize(&mut emb);
                    mixed_overlaps.push((time, emb));
                }
            }
        }

        self.timings.embedding_secs += embed_start.elapsed().as_secs_f64();
        if units.is_empty() && !primary.is_empty() {
            warn!(
                "Chunk at {:.0}s: segmentation found {} primary segments but embedding produced 0 valid vectors (possible audio_signal layout mismatch — expected [B,80,T] for titanet_large)",
                chunk_start_secs,
                primary.len()
            );
        }

        self.chunks.push(ChunkRecord {
            start_secs: chunk_start_secs,
            primary,
            overlaps,
            units,
            mixed_overlaps,
        });
        Ok(())
    }

    /// Global stage: cluster all accumulated units, map per-chunk local
    /// speakers onto global clusters, resegment overlaps, filter, gap-fill.
    fn finish(self) -> Result<
        (
            Vec<DiarizationSegment>,
            Vec<ClusteredEmbedding>,
            StageTimings,
        ),
        String,
    > {
        let mut timings = self.timings;
        let all_units: Vec<&DenseUnit> = self.chunks.iter().flat_map(|c| c.units.iter()).collect();
        if all_units.is_empty() {
            if self.had_raw_segments {
                return Err(
                    "Embedding produced zero valid vectors (audio_signal layout mismatch — expected [B,80,T] for titanet_large, but no embeddings survived; check TitaNet layout)".to_string(),
                );
            }
            return Ok((Vec::new(), Vec::new(), timings));
        }
        if DIARIZATION_CANCELLED.load(Ordering::SeqCst) {
            return Err("Diarization cancelled".to_string());
        }

        let raw_embeddings = self.diarizer.clusterer.wants_raw_embeddings();
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_units.len());
        let mut durations: Vec<f64> = Vec::with_capacity(all_units.len());
        for unit in &all_units {
            let mut emb = unit.embedding.clone();
            if !raw_embeddings {
                polyvoice::utils::l2_normalize(&mut emb);
            }
            durations.push(unit.time.end - unit.time.start);
            embeddings.push(emb);
        }

        let cluster_start = Instant::now();
        let labels = self
            .diarizer
            .clusterer
            .cluster_with_durations(&embeddings, &durations)
            .map_err(|e| format!("Speaker clustering failed: {}", e))?;
        timings.clustering_secs = cluster_start.elapsed().as_secs_f64();

        let reseg_start = Instant::now();
        let (segments, clustered) = assemble_channel_turns(
            &self.chunks,
            &embeddings,
            &labels,
            &self.diarizer.resegmenter,
            self.config,
        )?;
        timings.resegmentation_secs = reseg_start.elapsed().as_secs_f64();

        info!(
            "Chunked v2 diarization found {} segments with {} unique speakers ({} embedding units)",
            segments.len(),
            count_unique_speakers(&segments),
            all_units.len()
        );
        Ok((segments, clustered, timings))
    }
}

/// Pure global stage (unit-testable): per-chunk Hungarian local→global mapping
/// over the clustered unit labels, per-segment primary turns (majority label
/// of the segment's dense windows) with splitting at mapped overlap spans,
/// overlap-aware two-speaker resegmentation, min-speech filter, and gap-fill.
fn assemble_channel_turns(
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
    let chunk_duration = config.chunk_duration_secs();
    let chunks = channel_chunks(
        samples,
        sample_rate,
        chunk_duration,
        config.chunk_overlap_secs,
    );

    info!(
        "Chunked v2 diarization: {} chunks ({}s duration, {}s overlap)",
        chunks.len(),
        chunk_duration,
        config.chunk_overlap_secs
    );

    let mut core = V2Core::new(diarizer, config);
    for (chunk_idx, (chunk_start_seconds, chunk_samples)) in chunks.iter().enumerate() {
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
        core.process_chunk(*chunk_start_seconds, &diar_samples)?;
    }
    core.finish()
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

    /// SpeakerEmbedder double for the v2 embed path: echoes input length in
    /// every dim, batch failure is injectable to exercise the fallback.
    struct EchoEmbedder {
        dim: usize,
        fail_batch: bool,
    }

    impl crate::audio::embedder::SpeakerEmbedder for EchoEmbedder {
        fn embed_batch(
            &self,
            audios: &[&[f32]],
        ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
            if self.fail_batch {
                return Err(polyvoice::embedder::EmbedderError::Legacy(
                    "injected batch failure".to_string(),
                ));
            }
            Ok(audios
                .iter()
                .map(|a| vec![a.len() as f32; self.dim])
                .collect())
        }
        fn embed(&self, audio: &[f32]) -> Result<Vec<f32>, polyvoice::embedder::EmbedderError> {
            Ok(vec![audio.len() as f32; self.dim])
        }
        fn input_dim(&self) -> usize {
            self.dim
        }
        fn model_tag(&self) -> &'static str {
            crate::audio::embedder::ENHANCED_MODEL_TAG
        }
        fn family_threshold(&self) -> f32 {
            crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
        }
    }

    #[test]
    fn embed_unit_slices_batch_and_fallback_are_position_aligned() {
        let inputs: Vec<Vec<f32>> = vec![vec![0.0; 10], vec![0.0; 20], vec![0.0; 30]];
        let batch = embed_unit_slices(&EchoEmbedder { dim: 4, fail_batch: false }, &inputs);
        let fallback = embed_unit_slices(&EchoEmbedder { dim: 4, fail_batch: true }, &inputs);
        assert_eq!(batch.len(), inputs.len());
        assert_eq!(fallback.len(), inputs.len(), "fallback keeps alignment");
        for (b, f) in batch.iter().zip(fallback.iter()) {
            assert_eq!(
                b.as_ref().map(|v| v[0]),
                f.as_ref().map(|v| v[0]),
                "batch and fallback results must agree"
            );
        }
        assert_eq!(
            batch.iter().map(|o| o.as_ref().map(|v| v[0]).unwrap()).collect::<Vec<_>>(),
            vec![10.0, 20.0, 30.0],
            "input order preserved"
        );
        // Wrong-dimension batch output is dropped in place (None), not shifted.
        struct BadDim;
        impl crate::audio::embedder::SpeakerEmbedder for BadDim {
            fn embed_batch(
                &self,
                _audios: &[&[f32]],
            ) -> Result<Vec<Vec<f32>>, polyvoice::embedder::EmbedderError> {
                Ok(vec![vec![1.0; 2]; 3])
            }
            fn input_dim(&self) -> usize {
                4
            }
            fn model_tag(&self) -> &'static str {
                crate::audio::embedder::ENHANCED_MODEL_TAG
            }
            fn family_threshold(&self) -> f32 {
                crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
            }
        }
        let out = embed_unit_slices(&BadDim, &inputs);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(Option::is_none), "dimension mismatches drop in place");
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

    #[test]
    fn diarization_config_uses_fixed_profile() {        let cfg = DiarizationConfig::default();
        assert!(cfg.max_sessions >= 1 && cfg.max_sessions <= 8);
        assert_eq!(cfg.chunk_overlap_secs, 5.0);
        assert_eq!(cfg.chunk_duration_secs(), 600.0);
    }

    #[test]
    fn diarization_config_clustering_defaults() {
        let cfg = DiarizationConfig::default();
        assert_eq!(cfg.cluster_threshold, 0.60);
        assert_eq!(cfg.cluster_ceiling, DEFAULT_CLUSTER_CEILING);
        assert_eq!(cfg.cluster_ceiling, 128);
        assert_eq!(cfg.gap_merge_secs, 0.3);
        // Built-in default kind is ahc (6.2 sweep: nmesc under-clusters).
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        assert_eq!(cfg.embed_window_secs, DEFAULT_EMBED_WINDOW_SECS);
        assert_eq!(cfg.embed_window_secs, 5.0);
        assert_eq!(cfg.min_speech_secs, 0.25);
        let bin = cfg.binarization.expect("v2 default enables calibrated binarization");
        assert!(bin.offset < bin.onset, "hysteresis: offset below onset");
        assert!(bin.min_duration_on > 0.0 && bin.min_duration_off > 0.0);
        // Built-in threshold equals the family default (single source of truth).
        assert_eq!(
            cfg.cluster_threshold,
            crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
        );
    }

    #[test]
    fn effective_ceiling_is_user_max_when_smaller_and_always_positive() {
        // Mirrors the ceiling computation in create_polyvoice_diarizer +
        // build_clusterer (clamp to the backend's 255 maximum).
        let ceiling = |max_speakers: Option<i32>, config: &DiarizationConfig| -> usize {
            let user_max = max_speakers.filter(|m| *m > 0).unwrap_or(i32::MAX) as usize;
            user_max.min(config.cluster_ceiling).max(1).min(MAX_CLUSTERERS)
        };
        let cfg = DiarizationConfig::default();
        // No user max -> default ceiling.
        assert_eq!(ceiling(None, &cfg), 128);
        assert_eq!(ceiling(Some(0), &cfg), 128);
        assert_eq!(ceiling(Some(-1), &cfg), 128);
        // User max below the ceiling wins.
        assert_eq!(ceiling(Some(5), &cfg), 5);
        // User max above the ceiling is capped by the configured ceiling.
        assert_eq!(ceiling(Some(200), &cfg), 128);
        // Oversized stored ceiling is clamped to the backend maximum.
        let stored = DiarizationConfig {
            cluster_ceiling: 300,
            ..DiarizationConfig::default()
        };
        assert_eq!(ceiling(None, &stored), 255);
        assert_eq!(ceiling(Some(999), &stored), 255);
        // Never zero (unbounded stop unreachable).
        assert_eq!(ceiling(Some(1), &cfg), 1);
    }

    #[test]
    fn clusterer_kind_parses_and_resolves_without_rebuild() {
        assert_eq!(ClustererKindSetting::parse("vbx"), Some(ClustererKindSetting::Vbx));
        assert_eq!(ClustererKindSetting::parse("NMEsc"), Some(ClustererKindSetting::Nmesc));
        assert_eq!(ClustererKindSetting::parse(" ahc "), Some(ClustererKindSetting::Ahc));
        assert_eq!(ClustererKindSetting::parse("kmeans"), None);
        assert_eq!(ClustererKindSetting::Nmesc.as_str(), "nmesc");
        assert!(ClustererKindSetting::Nmesc.is_automatic_count());
        assert!(ClustererKindSetting::Vbx.is_automatic_count());
        assert!(!ClustererKindSetting::Ahc.is_automatic_count());
    }

    #[test]
    fn vbx_kind_fails_actionably_on_the_192d_enhanced_family() {
        // Gate (revised D2): the vendored PLDA params require 256-d
        // embeddings; selecting vbx must error with a clear message and never
        // silently switch kinds.
        let config = DiarizationConfig {
            clusterer: ClustererKindSetting::Vbx,
            ..DiarizationConfig::default()
        };
        let err = match build_clusterer(&config, 128) {
            Ok(_) => panic!("vbx must not build for the 192-d enhanced family"),
            Err(e) => e,
        };
        assert!(err.contains("256-dimensional"), "error names the dim requirement: {err}");
        assert!(err.contains("192-d"), "error names the active family: {err}");
        assert!(err.contains("ahc"), "error suggests the default kind: {err}");
    }

    #[test]
    fn clusterer_factory_enforces_clamped_ceiling() {
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Ahc,
            ..DiarizationConfig::default()
        };
        let c = build_clusterer(&cfg, 300).expect("ahc builds");
        assert_eq!(c.max_clusters(), 255, "clamped to the backend maximum");
        let c = build_clusterer(&cfg, 12).expect("ahc builds");
        assert_eq!(c.max_clusters(), 12);
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Nmesc,
            ..DiarizationConfig::default()
        };
        let c = build_clusterer(&cfg, 300).expect("nmesc builds");
        assert_eq!(c.max_clusters(), 255);
    }

    #[test]
    fn merge_threshold_is_inert_under_automatic_count() {
        // Same embeddings, two stored thresholds, kind nmesc: identical labels
        // (the threshold only decorates the ahc kind).
        use polyvoice::clusterer::Clusterer as _;
        let embeddings: Vec<Vec<f32>> = (0..8)
            .map(|i| {
                let mut v = vec![0.0f32; 6];
                v[i % 2] = 1.0;
                v[(i % 2) + 2] = 0.3;
                v
            })
            .collect();
        let mut labels_by_threshold = Vec::new();
        for threshold in [0.1f32, 0.9] {
            let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Ahc,
                cluster_threshold: threshold,
                ..DiarizationConfig::default()
            };
            let clusterer = build_clusterer(&cfg, 128).expect("nmesc builds");
            labels_by_threshold.push(clusterer.cluster(&embeddings).expect("cluster"));
        }
        assert_eq!(
            labels_by_threshold[0], labels_by_threshold[1],
            "stored merge threshold must not change nmesc results"
        );
    }

    #[test]
    fn expand_embed_units_dense_windowing() {
        // 12 s segment, 5 s window, 2.5 s hop -> windows [0,5],[2.5,7.5],[5,10],[7.5,12].
        let segs = vec![raw_seg(0.0, 12.0, 0), raw_seg(20.0, 23.0, 1)];
        let units = expand_embed_units(&segs, 5.0);
        assert_eq!(units.len(), 5, "4 dense windows + 1 short whole segment");
        assert_eq!((units[0].0.start, units[0].0.end), (0.0, 5.0));
        assert_eq!((units[1].0.start, units[1].0.end), (2.5, 7.5));
        assert_eq!((units[2].0.start, units[2].0.end), (5.0, 10.0));
        assert_eq!((units[3].0.start, units[3].0.end), (7.5, 12.0));
        assert_eq!((units[4].0.start, units[4].0.end), (20.0, 23.0));
        // Ordering + parent linkage.
        for w in units.windows(2) {
            assert!(w[0].0.start <= w[1].0.start, "units sorted by start");
        }
        assert_eq!(
            (units[0].2, units[3].2, units[4].2),
            (0, 0, 1),
            "segment_idx ties units to their parent"
        );
        assert_eq!(units[4].1, 1, "local speaker inherited");
        // Sparse mode: one unit per segment.
        let sparse = expand_embed_units(&segs, 0.0);
        assert_eq!(sparse.len(), 2);
    }

    #[test]
    fn per_segment_embedding_is_l2_normalized_window_mean() {
        use polyvoice::clusterer::Clusterer as _;
        // One 12 s primary segment split into 4 dense windows by the clusterer
        // path; the segment's ClusteredEmbedding must be the L2-normalized
        // mean of its window embeddings.
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
                    embedding: vec![1.0, (i as f32) * 0.5],
                })
                .collect(),
            mixed_overlaps: Vec::new(),
        };
        let embeddings: Vec<Vec<f32>> = chunk.units.iter().map(|u| u.embedding.clone()).collect();
        let config = DiarizationConfig::default();
        let clusterer = build_clusterer(&config, 128).expect("default kind builds");
        let durations: Vec<f64> = vec![5.0; 4];
        let labels = clusterer
            .cluster_with_durations(&embeddings, &durations)
            .expect("cluster");
        let (segments, clustered) = assemble_channel_turns(
            &[chunk],
            &embeddings,
            &labels,
            &polyvoice::resegmentation::OverlapResegmenter::default(),
            &config,
        )
        .expect("assemble");
        assert_eq!(clustered.len(), 1, "one per-segment aggregate");
        assert_eq!(segments.len(), 1);
        let emb = &clustered[0].embedding;
        let expected_mean = vec![1.0f32, 0.75]; // mean of [0, .5, 1, 1.5]
        let norm = expected_mean.iter().map(|x| x * x).sum::<f32>().sqrt();
        for (got, want) in emb.iter().zip(&expected_mean) {
            assert!(
                (got - want / norm).abs() < 1e-5,
                "aggregate must be the L2-normalized window mean"
            );
        }
        let unit_norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((unit_norm - 1.0).abs() < 1e-5);
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

    #[test]
    fn fragmented_long_recording_is_capped_by_the_ceiling() {
        // Singleton pruning is gone (3.6); the always-enforced ceiling still
        // bounds the label count on a fragmented embedding set.
        use polyvoice::clusterer::Clusterer as _;
        let embeddings: Vec<Vec<f32>> = (0..40)
            .map(|i| {
                let mut v = vec![0.0f32; 16];
                v[i % 16] = 1.0;
                v[(i / 16) + 8] = 0.2;
                polyvoice::utils::l2_normalize(&mut v);
                v
            })
            .collect();
        let cfg = DiarizationConfig {
            clusterer: ClustererKindSetting::Nmesc,
            ..DiarizationConfig::default()
        };
        let clusterer = build_clusterer(&cfg, 4).expect("nmesc builds");
        let labels = clusterer.cluster(&embeddings).expect("cluster");
        let distinct: std::collections::HashSet<_> = labels.iter().collect();
        assert!(
            distinct.len() <= 4,
            "ceiling must bound distinct labels, got {}",
            distinct.len()
        );
    }

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

    fn raw_seg(start: f64, end: f64, local: u8) -> RawSegment {
        RawSegment {
            time: polyvoice::types::TimeRange { start, end },
            local_speaker_idx: local,
            is_overlap: false,
            confidence: polyvoice::types::Confidence::new(0.9).unwrap_or_default(),
        }
    }

    fn test_unit(start: f64, end: f64, local: u8, segment_idx: usize) -> DenseUnit {
        DenseUnit {
            time: polyvoice::types::TimeRange { start, end },
            local_idx: local,
            segment_idx,
            embedding: vec![1.0, 0.0],
        }
    }

    #[test]
    fn resolved_config_prefers_stored_overrides_and_falls_back_to_defaults() {
        // Save/restore the process globals so the test is parallel-safe:
        // no other test reads the clustering overrides.
        let saved = (
            stored_cluster_threshold(),
            stored_cluster_ceiling(),
            stored_gap_merge_secs(),
            stored_clusterer_kind(),
        );
        // Unset -> built-in defaults.
        set_clustering_overrides(None, None, None, None);
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, crate::audio::embedder::TITANET_CLUSTER_THRESHOLD);
        assert_eq!(cfg.cluster_ceiling, DEFAULT_CLUSTER_CEILING);
        assert_eq!(cfg.gap_merge_secs, DEFAULT_GAP_MERGE_SECS);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        // Stored override wins per key.
        set_clustering_overrides(Some(0.35), Some(12), Some(0.3), Some(ClustererKindSetting::Ahc));
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, 0.35);
        assert_eq!(cfg.cluster_ceiling, 12);
        assert_eq!(cfg.gap_merge_secs, 0.3);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        // Partial override: only the ceiling is stored, others fall back.
        set_clustering_overrides(None, Some(7), None, None);
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, crate::audio::embedder::TITANET_CLUSTER_THRESHOLD);
        assert_eq!(cfg.cluster_ceiling, 7);
        assert_eq!(cfg.gap_merge_secs, DEFAULT_GAP_MERGE_SECS);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        set_clustering_overrides(saved.0, saved.1, saved.2, saved.3);
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
