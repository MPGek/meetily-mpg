// audio/transcription/commands.rs
//
// Tauri commands for transcription model readiness checks.

use serde::Serialize;
use tauri::{command, AppHandle, Manager, Runtime};

#[derive(Serialize)]
pub struct TranscriptionModelStatus {
    pub ready: bool,
    pub provider: String,
    pub downloading: bool,
}

#[command]
pub async fn check_active_transcription_model_ready<R: Runtime>(
    app: AppHandle<R>,
) -> Result<TranscriptionModelStatus, String> {
    let config =
        match crate::api::api::api_get_transcript_config(app.clone(), app.state(), None).await {
            Ok(Some(config)) => config,
            Ok(None) => crate::api::api::TranscriptConfig {
                provider: "parakeet".to_string(),
                model: crate::config::DEFAULT_PARAKEET_MODEL.to_string(),
                api_key: None,
            },
            Err(_) => crate::api::api::TranscriptConfig {
                provider: "parakeet".to_string(),
                model: crate::config::DEFAULT_PARAKEET_MODEL.to_string(),
                api_key: None,
            },
        };

    match config.provider.as_str() {
        "localWhisper" => {
            crate::whisper_engine::commands::whisper_init().await?;

            let has_models =
                crate::whisper_engine::commands::whisper_has_available_models().await?;

            let downloading = if !has_models {
                let models =
                    crate::whisper_engine::commands::whisper_get_available_models().await?;
                models.iter().any(|m| {
                    matches!(
                        m.status,
                        crate::whisper_engine::ModelStatus::Downloading { .. }
                    )
                })
            } else {
                false
            };

            Ok(TranscriptionModelStatus {
                ready: has_models,
                provider: "localWhisper".to_string(),
                downloading,
            })
        }
        "parakeet" => {
            crate::parakeet_engine::commands::parakeet_init().await?;

            let has_models =
                crate::parakeet_engine::commands::parakeet_has_available_models().await?;

            let downloading = if !has_models {
                let models =
                    crate::parakeet_engine::commands::parakeet_get_available_models().await?;
                models.iter().any(|m| {
                    matches!(
                        m.status,
                        crate::parakeet_engine::ModelStatus::Downloading { .. }
                    )
                })
            } else {
                false
            };

            Ok(TranscriptionModelStatus {
                ready: has_models,
                provider: "parakeet".to_string(),
                downloading,
            })
        }
        other => Err(format!("Unsupported transcription provider: {}", other)),
    }
}
