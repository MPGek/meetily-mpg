// audio/recording/devices.rs
//
// Device resolution for recording start: explicit name, then saved
// preference, then system default.

use log::{info, warn};
use std::sync::Arc;

use crate::audio::devices::{AudioDevice, DeviceType};
use crate::audio::{default_input_device, default_output_device, parse_audio_device};

/// The microphone a recording starts on.
#[derive(Debug, Clone)]
pub(super) struct ResolvedMic {
    /// Carries the enumerated name, so the session state and the device
    /// monitor track exactly the device that is opened.
    pub device: Arc<AudioDevice>,
    /// The requested or preferred name that was not present, when the start
    /// fell back to the system default instead.
    pub fell_back_from: Option<String>,
}

/// No microphone resolved: not even a system default input exists.
#[derive(Debug, Clone)]
pub(super) struct NoMicrophone {
    pub default_error: String,
}

impl NoMicrophone {
    pub(super) fn message(&self) -> String {
        format!("No microphone device available: {}", self.default_error)
    }
}

/// Match a requested input name against the enumerated input names, with the
/// rule `get_device_and_config` uses to open it: on Windows an exact or
/// substring match (as `get_windows_device`), elsewhere an exact match only.
/// Returns the enumerated name.
pub(super) fn match_input_name(requested: &str, enumerated: &[String]) -> Option<String> {
    if requested.trim().is_empty() {
        return None;
    }
    if let Some(exact) = enumerated.iter().find(|name| name.as_str() == requested) {
        return Some(exact.clone());
    }
    #[cfg(target_os = "windows")]
    {
        enumerated
            .iter()
            .find(|name| name.contains(requested))
            .cloned()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// The enumerated name of the present input device matching `name`, if any.
fn find_present_input(name: &str) -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    #[cfg(target_os = "windows")]
    let host = cpal::host_from_id(cpal::HostId::Wasapi).ok()?;
    #[cfg(not(target_os = "windows"))]
    let host = cpal::default_host();

    let enumerated: Vec<String> = match host.input_devices() {
        Ok(devices) => devices.filter_map(|d| d.name().ok()).collect(),
        Err(e) => {
            warn!("⚠️ Could not enumerate input devices: {}", e);
            return None;
        }
    };
    match_input_name(name, &enumerated)
}

/// Resolve the microphone device: explicit name → saved preference → system
/// default. A name counts only if a present input device matches it.
/// Microphone is required; returns an error when no device resolves.
pub(super) fn resolve_microphone_device(
    explicit_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Result<ResolvedMic, NoMicrophone> {
    let mut missed: Option<String> = None;

    for (what, requested) in [("Microphone", explicit_name), ("Preferred microphone", preferred_name)] {
        let Some(requested) = requested else {
            continue;
        };
        match parse_audio_device(requested) {
            Ok(device) => match find_present_input(&device.name) {
                Some(enumerated) => {
                    info!("✅ Using microphone: '{}'", enumerated);
                    return Ok(ResolvedMic {
                        device: Arc::new(AudioDevice::new(enumerated, DeviceType::Input)),
                        fell_back_from: None,
                    });
                }
                None => {
                    warn!(
                        "⚠️ {} '{}' not available, falling back...",
                        what, device.name
                    );
                    missed.get_or_insert(device.name);
                }
            },
            Err(e) => {
                warn!(
                    "⚠️ Invalid microphone device '{}': {}, falling back...",
                    requested, e
                );
                missed.get_or_insert(requested.to_string());
            }
        }
    }

    let device = default_input_device().map_err(|e| NoMicrophone {
        default_error: e.to_string(),
    })?;
    info!("✅ Using default microphone: '{}'", device.name);
    let fell_back_from = missed.filter(|name| *name != device.name);
    Ok(ResolvedMic {
        device: Arc::new(device),
        fell_back_from,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_exact_match_returns_the_enumerated_name() {
        let enumerated = names(&["Microphone (USB Audio)", "Microphone Array (Realtek)"]);
        assert_eq!(
            match_input_name("Microphone Array (Realtek)", &enumerated).as_deref(),
            Some("Microphone Array (Realtek)")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn a_substring_match_returns_the_full_enumerated_name_on_windows() {
        let enumerated = names(&["Headset Microphone (2- Jabra Evolve 75)"]);
        assert_eq!(
            match_input_name("Jabra Evolve 75", &enumerated).as_deref(),
            Some("Headset Microphone (2- Jabra Evolve 75)")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_substring_does_not_match_off_windows() {
        let enumerated = names(&["Headset Microphone (2- Jabra Evolve 75)"]);
        assert_eq!(match_input_name("Jabra Evolve 75", &enumerated), None);
    }

    #[test]
    fn a_missing_name_returns_none() {
        let enumerated = names(&["Microphone Array (Realtek)"]);
        assert_eq!(match_input_name("USB Mic", &enumerated), None);
        // An empty name never matches (it is a substring of every name).
        assert_eq!(match_input_name("", &enumerated), None);
    }

    #[test]
    fn an_empty_list_returns_none() {
        assert_eq!(match_input_name("USB Mic", &[]), None);
    }
}
