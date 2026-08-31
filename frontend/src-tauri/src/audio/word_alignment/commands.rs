//! Tauri commands for alignment model management (task 3.4).
//!
//! Mirrors the Parakeet command surface: list/check report catalog + status,
//! download streams progress events, cancel cleans partials, delete removes.

use super::catalog::{list_models, AlignmentModelInfo, AlignmentModelStatus};
use super::download::{AlignmentDownloadProgress, AlignmentModelManager};
use super::settings;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{command, AppHandle, Emitter, Manager, Runtime};

// Global alignment model manager (initialized during app setup).
static ALIGNMENT_MANAGER: Mutex<Option<Arc<AlignmentModelManager>>> = Mutex::new(None);
static MODELS_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Point the module at `<app_data_dir>/models` (alignment models live in the
/// `alignment/` subdirectory). Called once during app setup.
pub fn set_models_directory<R: Runtime>(app: &AppHandle<R>) {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");
    let models_root = app_data_dir.join("models");
    if !models_root.exists() {
        if let Err(e) = std::fs::create_dir_all(&models_root) {
            log::error!("Failed to create models directory: {}", e);
            return;
        }
    }
    *MODELS_ROOT.lock().unwrap() = Some(models_root.clone());
    // Share the root with the settings holder so refine/engine resolve models.
    settings::set_models_root(models_root);
}

fn get_manager() -> Result<Arc<AlignmentModelManager>, String> {
    {
        let guard = ALIGNMENT_MANAGER.lock().unwrap();
        if let Some(mgr) = guard.as_ref() {
            return Ok(mgr.clone());
        }
    }
    let root = MODELS_ROOT
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Alignment models directory not initialized".to_string())?;
    let mgr = Arc::new(AlignmentModelManager::new(root));
    *ALIGNMENT_MANAGER.lock().unwrap() = Some(mgr.clone());
    Ok(mgr)
}

/// List catalogued alignment models with their current status.
#[command]
pub async fn list_alignment_models() -> Result<Vec<AlignmentModelInfo>, String> {
    let mgr = get_manager()?;
    Ok(list_models(mgr.models_root()))
}

/// Readiness of every catalogued model, keyed by id (independent of
/// transcription model readiness).
#[command]
pub async fn check_alignment_models() -> Result<HashMap<String, AlignmentModelStatus>, String> {
    let mgr = get_manager()?;
    let infos = list_models(mgr.models_root());
    let mut out = HashMap::new();
    for i in infos {
        let status = mgr.status(&i.id).await;
        out.insert(i.id, status);
    }
    Ok(out)
}

/// Download a catalogued alignment model, emitting
/// `alignment-model-download-progress` events. Resolves when complete.
#[command]
pub async fn download_alignment_model<R: Runtime>(
    app_handle: AppHandle<R>,
    model_id: String,
) -> Result<(), String> {
    let mgr = get_manager()?;

    let app_for_cb = app_handle.clone();
    let id_for_cb = model_id.clone();
    let callback: Box<dyn Fn(AlignmentDownloadProgress) + Send + Sync> = Box::new(move |p| {
        let _ = app_for_cb.emit(
            "alignment-model-download-progress",
            serde_json::json!({
                "modelId": id_for_cb,
                "progress": p.percent,
                "downloaded_bytes": p.downloaded_bytes,
                "total_bytes": p.total_bytes,
                "speed_mbps": p.speed_mbps,
            }),
        );
    });

    match mgr.download_model(&model_id, Some(callback)).await {
        Ok(()) => {
            let _ = app_handle.emit(
                "alignment-model-download-completed",
                serde_json::json!({ "modelId": model_id }),
            );
            Ok(())
        }
        Err(e) => {
            let _ = app_handle.emit(
                "alignment-model-download-failed",
                serde_json::json!({ "modelId": model_id, "error": e.to_string() }),
            );
            Err(format!("Failed to download alignment model: {}", e))
        }
    }
}

/// Cancel an in-flight download and remove partial files.
#[command]
pub async fn cancel_alignment_download(model_id: String) -> Result<(), String> {
    let mgr = get_manager()?;
    mgr.cancel_download(&model_id)
        .await
        .map_err(|e| format!("Failed to cancel download: {}", e))
}

/// Delete a downloaded alignment model.
#[command]
pub async fn delete_alignment_model(model_id: String) -> Result<(), String> {
    let mgr = get_manager()?;
    mgr.delete_model(&model_id)
        .await
        .map_err(|e| format!("Failed to delete alignment model: {}", e))
}

/// Persist the effective word-alignment settings from the frontend store so
/// the live worker, finalize repair, and offline repair all read the same
/// values (task 6.1). Called on app startup and whenever the user toggles.
#[command]
pub async fn set_word_alignment_settings(
    enabled: bool,
    model_id: Option<String>,
) -> Result<(), String> {
    settings::set_settings(enabled, model_id);
    Ok(())
}
