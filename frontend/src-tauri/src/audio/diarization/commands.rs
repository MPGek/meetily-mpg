//! The Tauri command surface of diarization: status, manual speaker edits,
//! re-matching a meeting from cached centroids, model readiness, and the
//! thin wrapper that starts an offline run.

use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, Runtime};

use super::batch::guard::{DIARIZATION_CANCELLED, DIARIZATION_IN_PROGRESS};
use super::identity::matching::Prototype;
use super::config::{set_clustering_overrides, ClustererKindSetting};
use super::DiarizationResult;
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::speaker::SpeakerRepository;
use crate::state::AppState;

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

/// Start the offline diarization pass for a saved meeting. The run itself
/// lives in `batch::orchestrator`; this is only the command surface.
#[tauri::command]
pub async fn start_diarization<R: Runtime>(
    app: AppHandle<R>,
    meeting_id: String,
    max_speakers: Option<i32>,
    state: tauri::State<'_, AppState>,
) -> Result<DiarizationResult, String> {
    super::batch::orchestrator::run_offline_diarization(app, meeting_id, max_speakers, state).await
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
