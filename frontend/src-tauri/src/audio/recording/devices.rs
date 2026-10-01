// audio/recording/devices.rs
//
// Device resolution for recording start: explicit name, then saved
// preference, then system default.

use log::warn;
use std::sync::Arc;

use crate::audio::{default_input_device, default_output_device, parse_audio_device};

/// Resolve the microphone device: explicit name → saved preference → system default.
/// Microphone is required; returns an error when no device resolves.
pub(super) fn resolve_microphone_device(
    explicit_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Result<Arc<crate::audio::devices::AudioDevice>, String> {
    if let Some(name) = explicit_name {
        match parse_audio_device(name) {
            Ok(device) => return Ok(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Invalid microphone device '{}': {}, falling back...",
                name, e
            ),
        }
    }

    if let Some(pref_name) = preferred_name {
        match parse_audio_device(pref_name) {
            Ok(device) => return Ok(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Preferred microphone '{}' not available: {}, falling back...",
                pref_name, e
            ),
        }
    }

    default_input_device()
        .map(Arc::new)
        .map_err(|e| format!("No microphone device available: {}", e))
}

/// Resolve the system audio device: explicit name → saved preference → system default.
/// System audio is optional; returns None only when no default output device exists.
pub(super) fn resolve_system_audio_device(
    explicit_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Option<Arc<crate::audio::devices::AudioDevice>> {
    if let Some(name) = explicit_name {
        match parse_audio_device(name) {
            Ok(device) => return Some(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Invalid system device '{}': {}, falling back...",
                name, e
            ),
        }
    }

    if let Some(pref_name) = preferred_name {
        match parse_audio_device(pref_name) {
            Ok(device) => return Some(Arc::new(device)),
            Err(e) => warn!(
                "⚠️ Preferred system audio '{}' not available: {}, falling back...",
                pref_name, e
            ),
        }
    }

    match default_output_device() {
        Ok(device) => Some(Arc::new(device)),
        Err(e) => {
            warn!(
                "⚠️ No default system audio available: {}, continuing with microphone only",
                e
            );
            None
        }
    }
}
