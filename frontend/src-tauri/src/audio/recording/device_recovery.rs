// audio/recording/device_recovery.rs
//
// Mid-recording microphone recovery (mic-disconnect-recovery). One sequential
// task per session consumes the device monitor's events and, when the mic in
// use disconnects, switches capture to the current system default input.
//
// The guard decisions (stale session, stopped session, stale event, retry
// budget) are pure functions over plain values, unit-tested below. The async
// glue around them never holds `RECORDING_MANAGER` across an `.await` or a
// cpal teardown or creation: every lock scope is a block that ends first.

use log::{debug, info, warn};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::mpsc;

use crate::audio::device_monitor::{DeviceEvent, DeviceMonitorType};
use crate::audio::devices::{default_input_device, AudioDevice, DeviceType};
use crate::audio::recording_commands::RECORDING_MANAGER;
use crate::audio::recording_state::{DeviceType as ChannelType, RecordingState};
use crate::audio::stream::AudioStream;
use crate::audio::sync_ext::LockRecover;
use crate::audio::RecordingManager;

/// Failed switch attempts allowed per disconnect before recovery gives up.
pub(crate) const MAX_ATTEMPTS: u32 = 3;

/// Cap on waiting for the lost device's stream to tear down. A cpal teardown
/// of a vanished device can stall; on timeout the blocking thread is leaked.
const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(2);

// ============================================================================
// User-facing events (design D12)
// ============================================================================

/// Why the session's mic changed without the user picking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SwitchReason {
    Disconnected,
    UnavailableAtStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MicDeviceSwitchedPayload {
    pub device_name: String,
    pub previous_device_name: String,
    pub reason: SwitchReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MicSwapFailedPayload {
    /// The lost microphone.
    pub device_name: String,
    pub error: String,
    pub attempt: u32,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MicRecoveryExhaustedPayload {
    pub device_name: String,
}

/// A notification for the frontend toast.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // variants mirror the event names (mic-*)
pub(crate) enum UserEvent {
    MicDeviceSwitched(MicDeviceSwitchedPayload),
    MicSwapFailed(MicSwapFailedPayload),
    MicRecoveryExhausted(MicRecoveryExhaustedPayload),
}

impl UserEvent {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            UserEvent::MicDeviceSwitched(_) => "mic-device-switched",
            UserEvent::MicSwapFailed(_) => "mic-swap-failed",
            UserEvent::MicRecoveryExhausted(_) => "mic-recovery-exhausted",
        }
    }

    pub(crate) fn payload(&self) -> serde_json::Value {
        let value = match self {
            UserEvent::MicDeviceSwitched(p) => serde_json::to_value(p),
            UserEvent::MicSwapFailed(p) => serde_json::to_value(p),
            UserEvent::MicRecoveryExhausted(p) => serde_json::to_value(p),
        };
        value.unwrap_or(serde_json::Value::Null)
    }

    pub(crate) fn emit<R: Runtime>(&self, app: &AppHandle<R>) {
        if let Err(e) = app.emit(self.name(), self.payload()) {
            warn!("[DEVICE_EVENTS] failed to emit {}: {}", self.name(), e);
        }
    }
}

// ============================================================================
// Sans-IO decisions (design D2, D4)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgnoreReason {
    /// The session stopped or was replaced.
    NotLive,
    /// The event names a device other than the mic this session captures
    /// from: a mic already switched away from, or the system-audio device.
    NotActiveMic,
    /// The retry budget for this disconnect is spent.
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Attempt,
    Ignore(IgnoreReason),
}

/// How one switch attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttemptOutcome {
    /// Capture now runs on `target`.
    Switched { target: String },
    Failed { error: String },
    /// The session ended during the attempt; it does not count.
    Aborted,
}

/// Per-session recovery state: the mic in use and the failed attempts since
/// the last successful switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MicRecovery {
    current_mic: String,
    failed: u32,
}

impl MicRecovery {
    pub(crate) fn new(current_mic: String) -> Self {
        Self {
            current_mic,
            failed: 0,
        }
    }

    pub(crate) fn current_mic(&self) -> &str {
        &self.current_mic
    }

    /// The number the next attempt would carry (1-based).
    pub(crate) fn next_attempt(&self) -> u32 {
        self.failed + 1
    }

    pub(crate) fn on_disconnect(&self, device_name: &str, live: bool) -> Action {
        if !live {
            Action::Ignore(IgnoreReason::NotLive)
        } else if device_name != self.current_mic {
            Action::Ignore(IgnoreReason::NotActiveMic)
        } else if self.failed >= MAX_ATTEMPTS {
            Action::Ignore(IgnoreReason::Exhausted)
        } else {
            Action::Attempt
        }
    }

    /// Fold an attempt's outcome into the budget. A success resets the budget
    /// and tracks the new mic; the last allowed failure reports exhaustion
    /// instead of a third "failed" event. An aborted attempt, or any outcome
    /// once the session is no longer live, changes nothing and reports nothing.
    pub(crate) fn on_attempt_result(
        &mut self,
        outcome: AttemptOutcome,
        live: bool,
    ) -> Option<UserEvent> {
        if !live {
            return None;
        }
        match outcome {
            AttemptOutcome::Aborted => None,
            AttemptOutcome::Switched { target } => {
                let previous = std::mem::replace(&mut self.current_mic, target.clone());
                self.failed = 0;
                Some(UserEvent::MicDeviceSwitched(MicDeviceSwitchedPayload {
                    device_name: target,
                    previous_device_name: previous,
                    reason: SwitchReason::Disconnected,
                }))
            }
            AttemptOutcome::Failed { error } => {
                self.failed += 1;
                if self.failed < MAX_ATTEMPTS {
                    Some(UserEvent::MicSwapFailed(MicSwapFailedPayload {
                        device_name: self.current_mic.clone(),
                        error,
                        attempt: self.failed,
                        max_attempts: MAX_ATTEMPTS,
                    }))
                } else {
                    Some(UserEvent::MicRecoveryExhausted(MicRecoveryExhaustedPayload {
                        device_name: self.current_mic.clone(),
                    }))
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscardReason {
    SessionStopped,
    SessionChanged,
}

impl std::fmt::Display for DiscardReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscardReason::SessionStopped => write!(f, "session stopped"),
            DiscardReason::SessionChanged => write!(f, "session changed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallDecision {
    Install,
    Discard(DiscardReason),
}

/// Whether a switch started by `session` may touch what the recording slot
/// holds now. `slot` is the slot's state and whether it is recording.
///
/// The pointer comparison is sound because the processor task holds a clone
/// of `session`, so its allocation cannot be reused while the task runs.
pub(crate) fn install_decision(
    slot: Option<(&Arc<RecordingState>, bool)>,
    session: &Arc<RecordingState>,
) -> InstallDecision {
    match slot {
        None => InstallDecision::Discard(DiscardReason::SessionStopped),
        Some((state, _)) if !Arc::ptr_eq(state, session) => {
            InstallDecision::Discard(DiscardReason::SessionChanged)
        }
        Some((_, false)) => InstallDecision::Discard(DiscardReason::SessionStopped),
        Some((_, true)) => InstallDecision::Install,
    }
}

/// The slot's view for `install_decision`.
fn slot_view(slot: &Option<RecordingManager>) -> Option<(&Arc<RecordingState>, bool)> {
    slot.as_ref().map(|m| (m.get_state(), m.is_recording()))
}

/// Session liveness (design D2): the manager in the slot is this session's
/// manager and is recording. Stop empties the slot as its first mutation, so
/// "slot empty" already means "stopping"; Stop→Start puts a different state
/// in the slot.
///
/// Constraint for future code: anything that temporarily takes
/// `RECORDING_MANAGER` out and restores it looks "stopped" to an in-flight
/// switch, which then aborts silently and uncounted.
fn session_live(session: &Arc<RecordingState>) -> bool {
    let slot = RECORDING_MANAGER.lock_or_recover();
    install_decision(slot_view(&slot), session) == InstallDecision::Install
}

// ============================================================================
// Async glue (design D1, D3)
// ============================================================================

/// Spawn the session's device event processor. It handles each event to
/// completion, including any switch attempt, before reading the next, and
/// ends when the channel closes (the monitor stopped and the manager was
/// dropped at the end of Stop).
pub fn spawn_device_event_processor<R: Runtime>(
    app: AppHandle<R>,
    mut receiver: mpsc::UnboundedReceiver<DeviceEvent>,
    session: Arc<RecordingState>,
) {
    let initial_mic = session
        .get_microphone_device()
        .map(|d| d.name.clone())
        .unwrap_or_default();

    tauri::async_runtime::spawn(async move {
        info!("[DEVICE_EVENTS] processor started (mic '{}')", initial_mic);
        let mut recovery = MicRecovery::new(initial_mic);

        while let Some(event) = receiver.recv().await {
            match event {
                DeviceEvent::DeviceDisconnected {
                    device_name,
                    device_type: DeviceMonitorType::Microphone,
                } => {
                    info!("[DEVICE_EVENTS] microphone '{}' disconnected", device_name);
                    match recovery.on_disconnect(&device_name, session_live(&session)) {
                        Action::Ignore(reason) => {
                            info!(
                                "[DEVICE_EVENTS] ignored disconnect of '{}': {:?}",
                                device_name, reason
                            );
                        }
                        Action::Attempt => {
                            let attempt = recovery.next_attempt();
                            let outcome =
                                attempt_mic_fallback(&session, &device_name, attempt).await;
                            if let AttemptOutcome::Failed { error } = &outcome {
                                warn!(
                                    "[HOT_SWAP] attempt {}/{} failed: {}",
                                    attempt, MAX_ATTEMPTS, error
                                );
                            }
                            let user_event =
                                recovery.on_attempt_result(outcome, session_live(&session));
                            if let Some(user_event) = user_event {
                                if let UserEvent::MicRecoveryExhausted(p) = &user_event {
                                    warn!("[HOT_SWAP] recovery exhausted for '{}'", p.device_name);
                                }
                                // Emit only while the session is still live.
                                if session_live(&session) {
                                    user_event.emit(&app);
                                }
                            }
                        }
                    }
                }
                DeviceEvent::DeviceListChanged => {
                    debug!("[DEVICE_EVENTS] device list changed");
                }
                other => {
                    info!("[DEVICE_EVENTS] {:?} (no action)", other);
                }
            }
        }

        info!(
            "[DEVICE_EVENTS] processor ended (mic '{}')",
            recovery.current_mic()
        );
    });
}

/// Resolve the fallback target: the current system default input, after a
/// short settle wait and one re-check. Never the lost device itself.
async fn resolve_fallback_target(lost_name: &str) -> Result<AudioDevice, String> {
    tokio::time::sleep(Duration::from_millis(150)).await;
    let mut target = query_default_input().await?;
    if target.name == lost_name {
        tokio::time::sleep(Duration::from_millis(300)).await;
        target = query_default_input().await?;
    }
    if target.name == lost_name {
        return Err(format!(
            "the default input is still the lost device '{}'",
            lost_name
        ));
    }
    Ok(target)
}

async fn query_default_input() -> Result<AudioDevice, String> {
    tokio::task::spawn_blocking(default_input_device)
        .await
        .map_err(|e| format!("default input query failed: {}", e))?
        .map_err(|e| e.to_string())
}

/// Stop a stream on a blocking thread, waiting at most `TEARDOWN_TIMEOUT`.
/// Never called with a lock held.
async fn stop_stream_off_thread(stream: AudioStream, what: &str) {
    let teardown = tokio::task::spawn_blocking(move || stream.stop());
    match tokio::time::timeout(TEARDOWN_TIMEOUT, teardown).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => warn!("[HOT_SWAP] {} stream stop failed: {}", what, e),
        Ok(Err(e)) => warn!("[HOT_SWAP] {} stream stop panicked: {}", what, e),
        Err(_) => warn!("[HOT_SWAP] {} stream teardown timed out", what),
    }
}

/// One switch attempt (design D3). The lock is held only for the synchronous
/// slot operations in Phase 1 and Phase 3; each lock scope is a block that
/// ends before the next `.await`.
async fn attempt_mic_fallback(
    session: &Arc<RecordingState>,
    lost_name: &str,
    attempt: u32,
) -> AttemptOutcome {
    // Settle and resolve the target, without the lock.
    let target = match resolve_fallback_target(lost_name).await {
        Ok(target) => target,
        Err(error) => return AttemptOutcome::Failed { error },
    };
    if !session_live(session) {
        return AttemptOutcome::Aborted;
    }
    info!(
        "[HOT_SWAP] mic '{}' disconnected → target '{}' (attempt {}/{})",
        lost_name, target.name, attempt, MAX_ATTEMPTS
    );

    // Phase 1, under the lock: detach the old stream and mark the gap.
    let old_stream = {
        let mut slot = RECORDING_MANAGER.lock_or_recover();
        if install_decision(slot_view(&slot), session) != InstallDecision::Install {
            return AttemptOutcome::Aborted;
        }
        let old_stream = slot
            .as_mut()
            .and_then(|manager| manager.take_mic_stream_for_swap());
        session.mark_mic_discontinuity();
        old_stream
    };

    // Teardown, without the lock. The stream may already be gone after an
    // earlier failed attempt.
    if let Some(stream) = old_stream {
        stop_stream_off_thread(stream, "old").await;
    }

    // Phase 2, without the lock: open the replacement, retrying once.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let device = Arc::new(AudioDevice::new(target.name.clone(), DeviceType::Input));
    let stream = match AudioStream::create(
        device.clone(),
        session.clone(),
        ChannelType::Microphone,
        None,
    )
    .await
    {
        Ok(stream) => stream,
        Err(first) => {
            warn!(
                "[HOT_SWAP] opening '{}' failed: {}; retrying once",
                target.name, first
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
            match AudioStream::create(
                device.clone(),
                session.clone(),
                ChannelType::Microphone,
                None,
            )
            .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    return AttemptOutcome::Failed {
                        error: format!("could not open '{}': {}", target.name, e),
                    }
                }
            }
        }
    };
    let (rate, channels) = stream.native_format();

    // Phase 3, under the lock: install only into the session that started
    // the attempt.
    let discarded = {
        let mut slot = RECORDING_MANAGER.lock_or_recover();
        match install_decision(slot_view(&slot), session) {
            InstallDecision::Install => {
                if let Some(manager) = slot.as_mut() {
                    manager.install_swapped_mic(stream, device);
                }
                None
            }
            InstallDecision::Discard(reason) => Some((stream, reason)),
        }
    };
    if let Some((stream, reason)) = discarded {
        info!("[HOT_SWAP] discarded: {}", reason);
        stop_stream_off_thread(stream, "discarded").await;
        return AttemptOutcome::Aborted;
    }

    info!(
        "[HOT_SWAP] mic switched to '{}' ({} Hz, {} ch)",
        target.name, rate, channels
    );
    AttemptOutcome::Switched {
        target: target.name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_state() -> Arc<RecordingState> {
        let state = RecordingState::new();
        state.start_recording().unwrap();
        state
    }

    #[test]
    fn a_stale_session_is_discarded_as_changed() {
        let ours = recording_state();
        let theirs = recording_state();
        assert_eq!(
            install_decision(Some((&theirs, true)), &ours),
            InstallDecision::Discard(DiscardReason::SessionChanged)
        );
    }

    #[test]
    fn an_empty_slot_is_discarded_as_stopped() {
        let ours = recording_state();
        assert_eq!(
            install_decision(None, &ours),
            InstallDecision::Discard(DiscardReason::SessionStopped)
        );
    }

    #[test]
    fn the_same_session_not_recording_is_discarded_as_stopped() {
        let ours = RecordingState::new();
        assert_eq!(
            install_decision(Some((&ours, ours.is_recording())), &ours),
            InstallDecision::Discard(DiscardReason::SessionStopped)
        );
    }

    #[test]
    fn the_same_recording_session_installs() {
        let ours = recording_state();
        assert_eq!(
            install_decision(Some((&ours, ours.is_recording())), &ours),
            InstallDecision::Install
        );
    }

    #[test]
    fn events_for_other_devices_are_ignored() {
        let recovery = MicRecovery::new("USB Mic".to_string());
        assert_eq!(
            recovery.on_disconnect("Old Headset", true),
            Action::Ignore(IgnoreReason::NotActiveMic)
        );
        assert_eq!(
            recovery.on_disconnect("USB Mic", false),
            Action::Ignore(IgnoreReason::NotLive)
        );
        assert_eq!(recovery.on_disconnect("USB Mic", true), Action::Attempt);
    }

    fn failed() -> AttemptOutcome {
        AttemptOutcome::Failed {
            error: "no default input".to_string(),
        }
    }

    #[test]
    fn three_failures_exhaust_the_budget() {
        let mut recovery = MicRecovery::new("USB Mic".to_string());

        let first = recovery.on_attempt_result(failed(), true);
        assert!(matches!(
            first,
            Some(UserEvent::MicSwapFailed(MicSwapFailedPayload { attempt: 1, max_attempts: 3, .. }))
        ));
        let second = recovery.on_attempt_result(failed(), true);
        assert!(matches!(
            second,
            Some(UserEvent::MicSwapFailed(MicSwapFailedPayload { attempt: 2, .. }))
        ));
        let third = recovery.on_attempt_result(failed(), true);
        assert_eq!(
            third,
            Some(UserEvent::MicRecoveryExhausted(MicRecoveryExhaustedPayload {
                device_name: "USB Mic".to_string()
            }))
        );
        assert_eq!(
            recovery.on_disconnect("USB Mic", true),
            Action::Ignore(IgnoreReason::Exhausted)
        );
    }

    #[test]
    fn an_aborted_attempt_reports_nothing_and_does_not_count() {
        let mut recovery = MicRecovery::new("USB Mic".to_string());
        assert_eq!(recovery.on_attempt_result(AttemptOutcome::Aborted, true), None);
        assert_eq!(recovery.on_attempt_result(failed(), false), None);
        assert_eq!(recovery.next_attempt(), 1);
        assert_eq!(recovery.on_disconnect("USB Mic", true), Action::Attempt);
    }

    #[test]
    fn a_success_resets_the_budget_and_tracks_the_new_mic() {
        let mut recovery = MicRecovery::new("USB Mic".to_string());
        recovery.on_attempt_result(failed(), true);
        recovery.on_attempt_result(failed(), true);

        let event = recovery.on_attempt_result(
            AttemptOutcome::Switched {
                target: "Laptop Mic".to_string(),
            },
            true,
        );
        assert_eq!(
            event,
            Some(UserEvent::MicDeviceSwitched(MicDeviceSwitchedPayload {
                device_name: "Laptop Mic".to_string(),
                previous_device_name: "USB Mic".to_string(),
                reason: SwitchReason::Disconnected,
            }))
        );
        assert_eq!(recovery.current_mic(), "Laptop Mic");
        assert_eq!(recovery.next_attempt(), 1);
        // A late event for the old mic is ignored; the new one is watched.
        assert_eq!(
            recovery.on_disconnect("USB Mic", true),
            Action::Ignore(IgnoreReason::NotActiveMic)
        );
        assert_eq!(recovery.on_disconnect("Laptop Mic", true), Action::Attempt);
    }

    #[test]
    fn payloads_match_the_wire_contract() {
        let switched = UserEvent::MicDeviceSwitched(MicDeviceSwitchedPayload {
            device_name: "Laptop Mic".to_string(),
            previous_device_name: "USB Mic".to_string(),
            reason: SwitchReason::UnavailableAtStart,
        });
        assert_eq!(switched.name(), "mic-device-switched");
        assert_eq!(
            switched.payload(),
            serde_json::json!({
                "device_name": "Laptop Mic",
                "previous_device_name": "USB Mic",
                "reason": "unavailable_at_start"
            })
        );

        let failed = UserEvent::MicSwapFailed(MicSwapFailedPayload {
            device_name: "USB Mic".to_string(),
            error: "boom".to_string(),
            attempt: 2,
            max_attempts: 3,
        });
        assert_eq!(failed.name(), "mic-swap-failed");
        assert_eq!(
            failed.payload(),
            serde_json::json!({
                "device_name": "USB Mic",
                "error": "boom",
                "attempt": 2,
                "max_attempts": 3
            })
        );

        let exhausted = UserEvent::MicRecoveryExhausted(MicRecoveryExhaustedPayload {
            device_name: "USB Mic".to_string(),
        });
        assert_eq!(exhausted.name(), "mic-recovery-exhausted");
        assert_eq!(
            exhausted.payload(),
            serde_json::json!({ "device_name": "USB Mic" })
        );

        let reason = serde_json::to_value(SwitchReason::Disconnected).unwrap();
        assert_eq!(reason, serde_json::json!("disconnected"));
    }
}
