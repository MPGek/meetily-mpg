use crate::database::models::Speaker;
use crate::database::repositories::speaker::{
    ClearAllResult, SpeakerRepository, SpeakerStorageStats, VoiceprintBrowser,
};
use crate::state::AppState;
use serde::{Deserialize, Serialize};

/// Response for `assign_speaker`: the registry speaker now bound to the cluster.
#[derive(Debug, Serialize)]
pub struct AssignedSpeaker {
    pub meeting_id: String,
    pub cluster_label: String,
    pub speaker_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SetExpectedSpeakersRequest {
    pub meeting_id: String,
    pub speaker_ids: Vec<String>,
}

/// List all registry speakers (for the editor dropdown).
#[tauri::command]
pub async fn list_speakers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Speaker>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::list_speakers(pool)
        .await
        .map_err(|e| format!("Failed to list speakers: {}", e))
}

/// Link a meeting cluster to a registry speaker with `matched_by='user'` and
/// enroll the cluster's cached embeddings as that speaker's prototypes.
/// Pass `speaker_id` to bind an existing person, or `new_name` to create one.
#[tauri::command]
pub async fn assign_speaker(
    meeting_id: String,
    cluster_label: String,
    speaker_id: Option<String>,
    new_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AssignedSpeaker, String> {
    let pool = state.db_manager.pool();

    let speaker: Speaker = match (speaker_id.as_ref(), new_name.as_ref()) {
        (Some(id), _) => {
            SpeakerRepository::get_speaker(pool, id)
                .await
                .map_err(|e| format!("Failed to load speaker: {}", e))?
                .ok_or_else(|| format!("Speaker {} not found", id))?
        }
        (None, Some(name)) => {
            SpeakerRepository::find_or_create_by_name(pool, name)
                .await
                .map_err(|e| format!("Failed to find-or-create speaker: {}", e))?
        }
        (None, None) => {
            return Err("Either speaker_id or new_name must be provided".to_string());
        }
    };

    SpeakerRepository::set_user_binding(pool, &meeting_id, &cluster_label, &speaker.id)
        .await
        .map_err(|e| format!("Failed to bind speaker: {}", e))?;

    // Enroll the cluster's cached exemplars as prototypes (no-op when the
    // cluster has no cache, e.g. a legacy meeting without diarization).
    let _ = SpeakerRepository::enroll_cluster(pool, &meeting_id, &cluster_label, &speaker.id)
        .await
        .map_err(|e| format!("Failed to enroll speaker: {}", e))?;

    Ok(AssignedSpeaker {
        meeting_id,
        cluster_label,
        speaker_id: speaker.id,
        name: speaker.name,
    })
}

/// Response for `assign_block_speaker`: the registry speaker now overridden
/// on a single transcript block (no cluster change, no enrollment).
#[derive(Debug, Serialize)]
pub struct AssignedBlockSpeaker {
    pub transcript_id: String,
    pub speaker_id: String,
    pub name: String,
}

/// Link a single transcript block to a registry speaker via a per-transcript
/// override (design D10). The editor defaults to this scope: only the edited
/// block is relabeled. In addition to the override, the block's cluster cached
/// exemplars are enrolled as the speaker's prototypes so a single-block
/// correction doubles as a teaching signal (no-op on clusters with no cache).
/// Pass `speaker_id` for an existing person or `new_name` to create one.
#[tauri::command]
pub async fn assign_block_speaker(
    transcript_id: String,
    speaker_id: Option<String>,
    new_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AssignedBlockSpeaker, String> {
    let pool = state.db_manager.pool();

    let speaker: Speaker = match (speaker_id.as_ref(), new_name.as_ref()) {
        (Some(id), _) => {
            SpeakerRepository::get_speaker(pool, id)
                .await
                .map_err(|e| format!("Failed to load speaker: {}", e))?
                .ok_or_else(|| format!("Speaker {} not found", id))?
        }
        (None, Some(name)) => {
            SpeakerRepository::find_or_create_by_name(pool, name)
                .await
                .map_err(|e| format!("Failed to find-or-create speaker: {}", e))?
        }
        (None, None) => {
            return Err("Either speaker_id or new_name must be provided".to_string());
        }
    };

    let written = SpeakerRepository::set_transcript_override(pool, &transcript_id, &speaker.id)
        .await
        .map_err(|e| format!("Failed to set block override: {}", e))?;
    if !written {
        return Err(format!("Transcript {} not found", transcript_id));
    }

    // Enroll the block's cluster cached exemplars as the speaker's prototypes.
    // Resolve the transcript's (meeting_id, cluster_label) and reparent the
    // best-K cache rows. No-op when the cluster has no cache (legacy meeting).
    if let Some((meeting_id, Some(cluster_label))) =
        SpeakerRepository::get_transcript_cluster(pool, &transcript_id)
            .await
            .map_err(|e| format!("Failed to load transcript cluster: {}", e))?
    {
        let _ = SpeakerRepository::enroll_cluster(pool, &meeting_id, &cluster_label, &speaker.id)
            .await
            .map_err(|e| format!("Failed to enroll speaker: {}", e))?;
    }

    Ok(AssignedBlockSpeaker {
        transcript_id,
        speaker_id: speaker.id,
        name: speaker.name,
    })
}

/// Apply a speaker to ALL blocks of a transcript's cluster — the "apply to
/// all blocks of this speaker" option in the editor. Reuses the cluster-wide
/// assignment path (`assign_speaker` semantics): links the cluster with
/// `matched_by='user'` and enrolls its cached embeddings.
#[tauri::command]
pub async fn apply_block_speaker_to_cluster(
    transcript_id: String,
    speaker_id: Option<String>,
    new_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<AssignedSpeaker, String> {
    let pool = state.db_manager.pool();

    let cluster = SpeakerRepository::get_transcript_cluster(pool, &transcript_id)
        .await
        .map_err(|e| format!("Failed to load transcript: {}", e))?
        .ok_or_else(|| format!("Transcript {} not found", transcript_id))?;
    let (meeting_id, cluster_label) = cluster;
    let cluster_label = cluster_label.ok_or_else(|| {
        "Transcript has no diarization cluster label; cannot apply to a cluster".to_string()
    })?;

    let speaker: Speaker = match (speaker_id.as_ref(), new_name.as_ref()) {
        (Some(id), _) => {
            SpeakerRepository::get_speaker(pool, id)
                .await
                .map_err(|e| format!("Failed to load speaker: {}", e))?
                .ok_or_else(|| format!("Speaker {} not found", id))?
        }
        (None, Some(name)) => {
            SpeakerRepository::find_or_create_by_name(pool, name)
                .await
                .map_err(|e| format!("Failed to find-or-create speaker: {}", e))?
        }
        (None, None) => {
            return Err("Either speaker_id or new_name must be provided".to_string());
        }
    };

    SpeakerRepository::set_user_binding(pool, &meeting_id, &cluster_label, &speaker.id)
        .await
        .map_err(|e| format!("Failed to bind speaker: {}", e))?;

    let _ = SpeakerRepository::enroll_cluster(pool, &meeting_id, &cluster_label, &speaker.id)
        .await
        .map_err(|e| format!("Failed to enroll speaker: {}", e))?;

    Ok(AssignedSpeaker {
        meeting_id,
        cluster_label,
        speaker_id: speaker.id,
        name: speaker.name,
    })
}

/// Globally rename a registry speaker. Applies to every meeting via the
/// read-time join (no per-meeting propagation).
#[tauri::command]
pub async fn rename_speaker(
    speaker_id: String,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::rename_speaker(pool, &speaker_id, &new_name)
        .await
        .map_err(|e| format!("Failed to rename speaker: {}", e))
}

/// Confirm an automatically recognized speaker binding as correct WITHOUT
/// changing the name and WITHOUT re-enrolling voiceprints. When `scope_all` is
/// true, flips the whole cluster to user provenance; otherwise also marks the
/// single transcript block as user-owned. Clears the auto confidence so the
/// `(auto) xx%` suffix drops.
#[tauri::command]
pub async fn confirm_block_speaker(
    transcript_id: String,
    scope_all: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<usize, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::confirm_speaker_binding(pool, &transcript_id, scope_all.unwrap_or(false))
        .await
        .map_err(|e| format!("Failed to confirm speaker: {}", e))
}

/// Replace the expected-speaker allowlist for a meeting. An empty list means
/// recognition matches against ALL registry speakers.
#[tauri::command]
pub async fn set_expected_speakers(
    request: SetExpectedSpeakersRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::set_expected_speakers(pool, &request.meeting_id, &request.speaker_ids)
        .await
        .map_err(|e| format!("Failed to set expected speakers: {}", e))
}

/// Read the expected-speaker ids for a meeting.
#[tauri::command]
pub async fn get_expected_speakers(
    meeting_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::get_expected_speakers(pool, &meeting_id)
        .await
        .map_err(|e| format!("Failed to load expected speakers: {}", e))
}

/// Voiceprint storage statistics for the Settings display.
#[tauri::command]
pub async fn speaker_storage_stats(
    state: tauri::State<'_, AppState>,
) -> Result<SpeakerStorageStats, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::storage_stats(pool)
        .await
        .map_err(|e| format!("Failed to load speaker storage stats: {}", e))
}

/// Voiceprint browser: grouped speakers + unconfirmed caches with provenance.
#[tauri::command]
pub async fn list_voiceprints(
    speaker_id: Option<String>,
    unconfirmed_only: Option<bool>,
    limit: Option<i64>,
    offset: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<VoiceprintBrowser, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::list_voiceprints(
        pool,
        speaker_id.as_deref(),
        unconfirmed_only.unwrap_or(false),
        limit,
        offset,
    )
    .await
    .map_err(|e| format!("Failed to list voiceprints: {}", e))
}

#[tauri::command]
pub async fn reject_voiceprint(
    id: String,
    permanent: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<SpeakerRepositoryReexportRejectResult, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::reject_voiceprint(pool, &id, permanent.unwrap_or(false))
        .await
        .map_err(|e| format!("Failed to reject voiceprint: {}", e))
        .map(|r| SpeakerRepositoryReexportRejectResult {
            speaker_id: r.speaker_id,
            remaining_prototypes: r.remaining_prototypes,
        })
}

#[derive(Debug, Serialize)]
pub struct SpeakerRepositoryReexportRejectResult {
    pub speaker_id: Option<String>,
    pub remaining_prototypes: i64,
}

#[tauri::command]
pub async fn reconfirm_voiceprint(
    id: String,
    speaker_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::reconfirm_voiceprint(pool, &id, &speaker_id)
        .await
        .map_err(|e| format!("Failed to reconfirm voiceprint: {}", e))
}

#[tauri::command]
pub async fn replace_speaker(
    source: String,
    target: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<SpeakerRepositoryReplaceResult, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::replace_speaker(pool, &source, target.as_deref())
        .await
        .map_err(|e| format!("Failed to replace speaker: {}", e))
        .map(|r| SpeakerRepositoryReplaceResult {
            affected_meetings: r.affected_meetings,
            affected_clusters: r.affected_clusters,
            affected_transcripts: r.affected_transcripts,
        })
}

#[derive(Debug, Serialize)]
pub struct SpeakerRepositoryReplaceResult {
    pub affected_meetings: i64,
    pub affected_clusters: i64,
    pub affected_transcripts: i64,
}

#[tauri::command]
pub async fn find_or_create_speaker(
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<Speaker, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::find_or_create_by_name(pool, &name)
        .await
        .map_err(|e| format!("Failed to find-or-create speaker: {}", e))
}

#[tauri::command]
pub async fn preview_replace_speaker(
    source: String,
    state: tauri::State<'_, AppState>,
) -> Result<SpeakerRepositoryReplaceResult, String> {
    let pool = state.db_manager.pool();
    SpeakerRepository::preview_replace_speaker(pool, &source)
        .await
        .map_err(|e| format!("Failed to preview replace: {}", e))
        .map(|r| SpeakerRepositoryReplaceResult {
            affected_meetings: r.affected_meetings,
            affected_clusters: r.affected_clusters,
            affected_transcripts: r.affected_transcripts,
        })
}

#[tauri::command]
pub async fn clear_all_voiceprints(
    state: tauri::State<'_, AppState>,
) -> Result<ClearAllResult, String> {
    let pool = state.db_manager.pool();
    let result = SpeakerRepository::clear_all_voiceprints(pool)
        .await
        .map_err(|e| format!("Failed to clear voiceprints: {}", e))?;
    // Best-effort clear of in-memory PrototypeStore for live Fast-mode
    // sessions so subsequent recognition does not use deleted prototypes.
    // The global store lives in audio::recording_commands::ONLINE_DIARIZATION_STORE.
    {
        use crate::audio::recording_commands::ONLINE_DIARIZATION_STORE;
        if let Ok(guard) = ONLINE_DIARIZATION_STORE.try_lock() {
            if let Some(store_arc) = guard.as_ref() {
                if let Ok(mut store) = store_arc.try_write() {
                    store.prototypes.clear();
                    store.bindings.clear();
                    store.session_embeddings.clear();
                }
            }
        }
    }
    Ok(result)
}
