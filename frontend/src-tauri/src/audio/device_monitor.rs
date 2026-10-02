// Audio device monitoring for disconnect/reconnect detection
use anyhow::Result;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::devices::{list_audio_devices, AudioDevice};
use super::sync_ext::LockRecover;

/// Device monitoring events
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    /// A device that was in use has disconnected
    DeviceDisconnected {
        device_name: String,
        device_type: DeviceMonitorType,
    },
    /// A previously disconnected device has reconnected
    DeviceReconnected {
        device_name: String,
        device_type: DeviceMonitorType,
    },
    /// Device list has changed (new device added or removed)
    DeviceListChanged,
}

/// Type of device being monitored
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceMonitorType {
    Microphone,
    SystemAudio,
}

/// What one polling cycle concluded about one monitored device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Observation {
    Disconnected,
    Reconnected,
}

/// Monitor state for a single device
#[derive(Debug, Clone)]
struct MonitoredDevice {
    name: String,
    device_type: DeviceMonitorType,
    consecutive_missing: u32,
    is_bluetooth: bool,
}

impl MonitoredDevice {
    fn new(name: String, device_type: DeviceMonitorType) -> Self {
        // Heuristic: check if device name contains bluetooth-related keywords
        let is_bluetooth = name.to_lowercase().contains("airpods")
            || name.to_lowercase().contains("bluetooth")
            || name.to_lowercase().contains("wireless");

        Self {
            name,
            device_type,
            consecutive_missing: 0,
            is_bluetooth,
        }
    }

    /// Get appropriate disconnect threshold based on device type
    fn disconnect_threshold(&self) -> u32 {
        // Bluetooth devices get more grace period (they can briefly disconnect)
        if self.is_bluetooth {
            3 // 3 polling cycles (6-15 seconds)
        } else {
            2 // 2 polling cycles (4-10 seconds)
        }
    }

    /// Fold one polling cycle's presence check into this device's state.
    ///
    /// A device that is present again after being missing reports
    /// `Reconnected` once and resets its counter. A missing device reports
    /// `Disconnected` when the counter reaches the threshold. Microphone
    /// entries re-report it every further `threshold` missing cycles, which
    /// drives the session's bounded fallback retries; system-audio entries
    /// report it only once, because nothing recovers a lost output and a
    /// repeat would only add warning noise.
    fn observe(&mut self, present: bool) -> Option<Observation> {
        if present {
            if self.consecutive_missing > 0 {
                self.consecutive_missing = 0;
                return Some(Observation::Reconnected);
            }
            return None;
        }

        self.consecutive_missing += 1;
        let threshold = self.disconnect_threshold();
        let fire = match self.device_type {
            DeviceMonitorType::Microphone => self.consecutive_missing % threshold == 0,
            DeviceMonitorType::SystemAudio => self.consecutive_missing == threshold,
        };
        fire.then_some(Observation::Disconnected)
    }

    /// Get appropriate reconnect check interval
    #[allow(dead_code)]
    fn reconnect_interval(&self) -> Duration {
        if self.is_bluetooth {
            Duration::from_secs(5) // Check every 5s for Bluetooth
        } else {
            Duration::from_secs(3) // Check every 3s for wired devices
        }
    }
}

/// Point the microphone entry at `name`, as a fresh entry: the missing
/// counter starts at zero and the Bluetooth threshold is re-derived from the
/// new name. Other entries are untouched. Adds a microphone entry if none
/// exists.
fn apply_mic_retarget(devices: &mut Vec<MonitoredDevice>, name: String) {
    let fresh = MonitoredDevice::new(name, DeviceMonitorType::Microphone);
    match devices
        .iter_mut()
        .find(|d| d.device_type == DeviceMonitorType::Microphone)
    {
        Some(entry) => *entry = fresh,
        None => devices.push(fresh),
    }
}

/// Audio device monitor that detects disconnects and reconnects
pub struct AudioDeviceMonitor {
    monitor_handle: Option<JoinHandle<()>>,
    event_sender: mpsc::UnboundedSender<DeviceEvent>,
    stop_signal: Arc<tokio::sync::Notify>,
    /// Mailbox for a mid-recording mic switch: the loop takes the new name at
    /// the start of its next cycle and watches that device instead.
    retarget: Arc<std::sync::Mutex<Option<String>>>,
}

impl AudioDeviceMonitor {
    /// Create a new device monitor
    pub fn new() -> (Self, mpsc::UnboundedReceiver<DeviceEvent>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let stop_signal = Arc::new(tokio::sync::Notify::new());

        (
            Self {
                monitor_handle: None,
                event_sender,
                stop_signal,
                retarget: Arc::new(std::sync::Mutex::new(None)),
            },
            event_receiver,
        )
    }

    /// Start monitoring specified devices
    pub fn start_monitoring(
        &mut self,
        microphone: Option<Arc<AudioDevice>>,
        system_audio: Option<Arc<AudioDevice>>,
    ) -> Result<()> {
        if self.monitor_handle.is_some() {
            warn!("Device monitor already running");
            return Ok(());
        }

        let mut monitored_devices = Vec::new();

        if let Some(mic) = microphone {
            monitored_devices.push(MonitoredDevice::new(
                mic.name.clone(),
                DeviceMonitorType::Microphone,
            ));
            info!(
                "🔍 Monitoring microphone: '{}' (Bluetooth: {})",
                mic.name,
                monitored_devices.last().unwrap().is_bluetooth
            );
        }

        if let Some(sys) = system_audio {
            monitored_devices.push(MonitoredDevice::new(
                sys.name.clone(),
                DeviceMonitorType::SystemAudio,
            ));
            info!(
                "🔍 Monitoring system audio: '{}' (Bluetooth: {})",
                sys.name,
                monitored_devices.last().unwrap().is_bluetooth
            );
        }

        if monitored_devices.is_empty() {
            return Err(anyhow::anyhow!("No devices to monitor"));
        }

        let event_sender = self.event_sender.clone();
        let stop_signal = self.stop_signal.clone();
        let retarget = self.retarget.clone();

        let handle = tokio::spawn(async move {
            Self::monitor_loop(monitored_devices, event_sender, stop_signal, retarget).await;
        });

        self.monitor_handle = Some(handle);
        info!("✅ Device monitor started");
        Ok(())
    }

    /// Watch `name` as the microphone from the next polling cycle on, after
    /// the session switched its mic to that device. Synchronous and cheap, so
    /// it is safe to call while holding the recording-manager lock.
    pub fn notify_mic_swapped(&self, name: String) {
        *self.retarget.lock_or_recover() = Some(name);
    }

    /// Stop monitoring
    pub async fn stop_monitoring(&mut self) {
        info!("Stopping device monitor");
        self.stop_signal.notify_one();

        if let Some(handle) = self.monitor_handle.take() {
            let _ = handle.await;
        }

        info!("Device monitor stopped");
    }

    /// Main monitoring loop
    async fn monitor_loop(
        mut monitored_devices: Vec<MonitoredDevice>,
        event_sender: mpsc::UnboundedSender<DeviceEvent>,
        stop_signal: Arc<tokio::sync::Notify>,
        retarget: Arc<std::sync::Mutex<Option<String>>>,
    ) {
        let mut last_device_list = Vec::new();
        let check_interval = Duration::from_secs(2); // Poll every 2 seconds

        loop {
            // Check for stop signal with timeout
            tokio::select! {
                _ = stop_signal.notified() => {
                    info!("Device monitor received stop signal");
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    // Continue with monitoring check
                }
            }

            // A mic switch since the last cycle: watch the new device.
            let retarget_name = retarget.lock_or_recover().take();
            if let Some(name) = retarget_name {
                info!("🔍 Monitoring switched microphone: '{}'", name);
                apply_mic_retarget(&mut monitored_devices, name);
            }

            // Get current device list
            let current_devices = match list_audio_devices().await {
                Ok(devices) => devices,
                Err(e) => {
                    error!("Failed to list audio devices: {}", e);
                    continue;
                }
            };

            // Check if device list changed
            if current_devices.len() != last_device_list.len() {
                debug!(
                    "Device list changed: {} -> {} devices",
                    last_device_list.len(),
                    current_devices.len()
                );
                let _ = event_sender.send(DeviceEvent::DeviceListChanged);
            }
            last_device_list = current_devices.clone();

            // Check each monitored device
            for monitored in &mut monitored_devices {
                let device_found = current_devices.iter().any(|d| d.name == monitored.name);
                let missing_before = monitored.consecutive_missing;

                match monitored.observe(device_found) {
                    Some(Observation::Reconnected) => {
                        info!(
                            "✅ Device '{}' reconnected after {} missing checks",
                            monitored.name, missing_before
                        );

                        let _ = event_sender.send(DeviceEvent::DeviceReconnected {
                            device_name: monitored.name.clone(),
                            device_type: monitored.device_type.clone(),
                        });
                    }
                    Some(Observation::Disconnected) => {
                        warn!(
                            "❌ Device '{}' ({:?}) disconnected! (missing for {} checks)",
                            monitored.name, monitored.device_type, monitored.consecutive_missing
                        );

                        let _ = event_sender.send(DeviceEvent::DeviceDisconnected {
                            device_name: monitored.name.clone(),
                            device_type: monitored.device_type.clone(),
                        });
                    }
                    None => {
                        if !device_found {
                            debug!(
                                "⚠️ Device '{}' missing for {} checks (threshold: {})",
                                monitored.name,
                                monitored.consecutive_missing,
                                monitored.disconnect_threshold()
                            );
                        }
                    }
                }
            }

            // Adjust check interval based on device states
            // If any device is missing, check more frequently
            let has_missing = monitored_devices.iter().any(|d| d.consecutive_missing > 0);
            let next_interval = if has_missing {
                Duration::from_secs(2) // Fast polling when device missing
            } else {
                Duration::from_secs(5) // Slower polling when all devices present
            };

            if next_interval != check_interval {
                debug!("Adjusting monitor interval to {:?}", next_interval);
            }
        }
    }
}

impl Default for AudioDeviceMonitor {
    fn default() -> Self {
        Self::new().0
    }
}

impl Drop for AudioDeviceMonitor {
    fn drop(&mut self) {
        // Signal stop
        self.stop_signal.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bluetooth_detection() {
        let airpods = MonitoredDevice::new(
            "John's AirPods Pro".to_string(),
            DeviceMonitorType::Microphone,
        );
        assert!(airpods.is_bluetooth);
        assert_eq!(airpods.disconnect_threshold(), 3);

        let builtin = MonitoredDevice::new(
            "Built-in Microphone".to_string(),
            DeviceMonitorType::Microphone,
        );
        assert!(!builtin.is_bluetooth);
        assert_eq!(builtin.disconnect_threshold(), 2);
    }

    fn disconnects_over(device: &mut MonitoredDevice, missing_cycles: u32) -> u32 {
        (0..missing_cycles)
            .filter(|_| device.observe(false) == Some(Observation::Disconnected))
            .count() as u32
    }

    #[test]
    fn wired_mic_re_reports_disconnect_every_threshold_cycles() {
        let mut mic = MonitoredDevice::new("USB Mic".to_string(), DeviceMonitorType::Microphone);
        let mut fired_at = Vec::new();
        for cycle in 1..=6 {
            if mic.observe(false) == Some(Observation::Disconnected) {
                fired_at.push(cycle);
            }
        }
        assert_eq!(fired_at, vec![2, 4, 6]);
    }

    #[test]
    fn system_device_reports_disconnect_once() {
        let mut sys = MonitoredDevice::new("Speakers".to_string(), DeviceMonitorType::SystemAudio);
        assert_eq!(disconnects_over(&mut sys, 6), 1);
    }

    #[test]
    fn present_after_missing_reports_reconnected_once_and_resets() {
        let mut mic = MonitoredDevice::new("USB Mic".to_string(), DeviceMonitorType::Microphone);
        assert_eq!(disconnects_over(&mut mic, 3), 1);
        assert_eq!(mic.observe(true), Some(Observation::Reconnected));
        assert_eq!(mic.consecutive_missing, 0);
        assert_eq!(mic.observe(true), None);
        // The counter restarted: the next disconnect needs a full threshold.
        assert_eq!(mic.observe(false), None);
        assert_eq!(mic.observe(false), Some(Observation::Disconnected));
    }

    #[test]
    fn mic_retarget_replaces_only_the_mic_entry() {
        let mut devices = vec![
            MonitoredDevice::new("John's AirPods".to_string(), DeviceMonitorType::Microphone),
            MonitoredDevice::new("Speakers".to_string(), DeviceMonitorType::SystemAudio),
        ];
        devices[0].consecutive_missing = 5;
        devices[1].consecutive_missing = 1;

        apply_mic_retarget(&mut devices, "Laptop Mic Array".to_string());

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].name, "Laptop Mic Array");
        assert_eq!(devices[0].device_type, DeviceMonitorType::Microphone);
        assert_eq!(devices[0].consecutive_missing, 0);
        assert!(!devices[0].is_bluetooth);
        assert_eq!(devices[1].name, "Speakers");
        assert_eq!(devices[1].consecutive_missing, 1);
    }

    #[test]
    fn notify_mic_swapped_fills_the_mailbox() {
        let (monitor, _receiver) = AudioDeviceMonitor::new();
        monitor.notify_mic_swapped("Laptop Mic Array".to_string());
        assert_eq!(
            monitor.retarget.lock_or_recover().take().as_deref(),
            Some("Laptop Mic Array")
        );
    }

    #[tokio::test]
    async fn test_monitor_creation() {
        let (mut monitor, _receiver) = AudioDeviceMonitor::new();
        assert!(monitor.monitor_handle.is_none());

        // Stop should be safe even if not started
        monitor.stop_monitoring().await;
    }
}
