use crate::audio::recording_commands::current_meeting_folder;
use crate::database::models::{MeetingTag, MeetingTagWithUsage};
use crate::database::repositories::tags::TagsRepository;
use crate::state::AppState;
use serde::Serialize;

/// Tag with usage count, as returned by `list_tags`.
pub type TagWithUsage = MeetingTagWithUsage;

/// List all tags with usage counts for autocomplete.
#[tauri::command]
pub async fn list_tags(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingTagWithUsage>, String> {
    let pool = state.db_manager.pool();
    TagsRepository::list_tags(pool)
        .await
        .map_err(|e| format!("Failed to list tags: {}", e))
}

/// Create a tag (or return the existing NOCASE duplicate).
#[tauri::command]
pub async fn create_tag(
    name: String,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingTag, String> {
    let pool = state.db_manager.pool();
    TagsRepository::create_tag(pool, &name, color.as_deref())
        .await
        .map_err(|e| format!("Failed to create tag: {}", e))
}

/// Rename a tag, preserving meeting links.
#[tauri::command]
pub async fn rename_tag(
    tag_id: String,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingTag, String> {
    let pool = state.db_manager.pool();
    TagsRepository::rename_tag(pool, &tag_id, &new_name)
        .await
        .map_err(|e| format!("Failed to rename tag: {}", e))
}

/// Override a tag's palette color.
#[tauri::command]
pub async fn set_tag_color(
    tag_id: String,
    color: String,
    state: tauri::State<'_, AppState>,
) -> Result<MeetingTag, String> {
    let pool = state.db_manager.pool();
    TagsRepository::set_tag_color(pool, &tag_id, &color)
        .await
        .map_err(|e| format!("Failed to set tag color: {}", e))
}

/// Delete a tag and its meeting links.
#[tauri::command]
pub async fn delete_tag(
    tag_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    TagsRepository::delete_tag(pool, &tag_id)
        .await
        .map_err(|e| format!("Failed to delete tag: {}", e))
}

/// Link a tag to a meeting (idempotent).
#[tauri::command]
pub async fn assign_tag(
    meeting_id: String,
    tag_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    TagsRepository::assign_tag(pool, &meeting_id, &tag_id)
        .await
        .map_err(|e| format!("Failed to assign tag: {}", e))
}

/// Remove a tag from a meeting (dictionary entry survives).
#[tauri::command]
pub async fn unassign_tag(
    meeting_id: String,
    tag_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    TagsRepository::unassign_tag(pool, &meeting_id, &tag_id)
        .await
        .map_err(|e| format!("Failed to unassign tag: {}", e))
}

/// Create-and-assign in one step: finds or creates the tag by name, then
/// links it to the meeting. Returns the tag plus whether a new link formed.
#[derive(Debug, Serialize)]
pub struct AssignedTag {
    pub tag: MeetingTag,
    pub linked: bool,
}

#[tauri::command]
pub async fn create_and_assign_tag(
    meeting_id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<AssignedTag, String> {
    let pool = state.db_manager.pool();
    let tag = TagsRepository::create_tag(pool, &name, None)
        .await
        .map_err(|e| format!("Failed to create tag: {}", e))?;
    let linked = TagsRepository::assign_tag(pool, &meeting_id, &tag.id)
        .await
        .map_err(|e| format!("Failed to assign tag: {}", e))?;
    Ok(AssignedTag { tag, linked })
}

/// Read the pending tag ids of the active recording (change:
/// tags-before-during-recording). Errors when no recording is active.
#[tauri::command]
pub async fn get_recording_pending_tags() -> Result<Vec<String>, String> {
    let Some(folder) = current_meeting_folder() else {
        return Err("No active recording".to_string());
    };
    crate::summary::metadata::read_pending_tag_ids_from_metadata(&folder)
        .map_err(|e| format!("Failed to read pending tags: {}", e))
}

/// Overwrite the pending tag ids of the active recording. Ids are trimmed,
/// deduplicated and capped by the metadata helper. Errors when no recording
/// is active.
#[tauri::command]
pub async fn set_recording_pending_tags(tag_ids: Vec<String>) -> Result<Vec<String>, String> {
    let Some(folder) = current_meeting_folder() else {
        return Err("No active recording".to_string());
    };
    crate::summary::metadata::write_pending_tag_ids_to_metadata(&folder, &tag_ids)
        .map_err(|e| format!("Failed to write pending tags: {}", e))?;
    crate::summary::metadata::read_pending_tag_ids_from_metadata(&folder)
        .map_err(|e| format!("Failed to read pending tags: {}", e))
}
