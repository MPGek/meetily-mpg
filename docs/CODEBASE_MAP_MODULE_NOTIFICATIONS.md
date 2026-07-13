---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:33:00Z
module: notifications
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Notifications

## Overview

**Purpose**: The notifications module provides system-level desktop notifications with user preference management, Do Not Disturb (DND) awareness, and per-event type configuration. Used for recording start/stop alerts, error notifications, and summary completion messages.

**Entry point**: `notifications/mod.rs` — module root
**Sub-packages**: None (single directory)

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mod.rs` | Module root, re-exports all sub-modules | notification types | ~1k |
| `manager.rs` | Notification manager core | NotificationManager struct, send/dismiss | ~6k |
| `commands.rs` | Tauri command handlers | show_notification, get_settings | ~3k |
| `settings.rs` | User notification preferences | NotificationSettings struct, save/load | ~4k |
| `system.rs` | Platform-specific notification API | native_send(), platform detection | ~5k |
| `types.rs` | Shared notification types | NotificationType enum, metadata | ~2k |

## Public API

### Key Functions (Tauri Commands)

| Function | Signature | Description |
|----------|-----------|-------------|
| `show_recording_started_notification` | `(meeting_name?) -> Result<(), String>` | Show "Recording started" notification |
| `show_recording_stopped_notification` | `(meeting_name?, recording_path?) -> Result<(), String>` | Show "Recording stopped" notification |
| `show_error_notification` | `(title, message) -> Result<(), String>` | Show error notification to user |
| `show_summary_completed_notification` | `(meeting_title?, summary_content?) -> Result<(), String>` | Show summary completion notification |
| `get_notification_settings` | `() -> NotificationSettings` | Get current notification preferences |
| `update_notification_settings` | `(settings) -> Result<(), String>` | Update notification preferences |

### Key Types

```rust
struct NotificationManager<R: Runtime> {
    settings: Arc<RwLock<NotificationSettings>>,
    pending_notifications: VecDeque<Notification>,
}

enum NotificationType {
    RecordingStarted,
    RecordingStopped,
    Error,
    SummaryCompleted,
    DeviceDisconnected,
}

struct NotificationSettings {
    enabled: bool,
    show_recording_started: bool,
    show_recording_stopped: bool,
    show_errors: bool,
    show_summary_completed: bool,
    dnd_enabled: bool,
    dnd_start_time: Option<String>,
    dnd_end_time: Option<String>,
}
```

## Internal Architecture

### Notification Flow

1. **Trigger**: Event occurs (recording start/stop, error, etc.)
2. **Settings Check**: `NotificationManager` checks current settings and DND status
3. **Platform API Call**: `system.rs` calls native notification API
4. **Display**: OS shows notification to user

### Platform-Specific Implementation

| Platform | API Used | Notes |
|----------|----------|-------|
| macOS | NSUserNotificationCenter | Native macOS notifications |
| Windows | Windows Toast Notifications | Requires Windows 10+ |
| Linux | libnotify (libappindicator) | Desktop environment dependent |

### DND Handling

- Checks if current time falls within DND window
- If DND active, notification is queued or suppressed based on settings
- Can be toggled via system tray menu

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `tauri` | `Manager`, `AppHandle` | Tauri app context for notifications |
| `chrono` | Time handling for DND | DND schedule calculation |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `audio/` | Recording start/stop notifications | When recording begins/ends |
| `summary/` | Summary completion notification | After AI summarization finishes |
| `lib.rs` (main) | All Tauri commands | Entry point for frontend control |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enabled` | true | Global notification toggle |
| `show_recording_started` | true | Show when recording begins |
| `show_recording_stopped` | true | Show when recording ends |
| `show_errors` | true | Show error notifications |
| `show_summary_completed` | false | Show summary completion by default |
| `dnd_enabled` | false | Do Not Disturb mode |

## Error Handling

- **Platform API unavailable**: Log warning, notification silently suppressed
- **Notification limit exceeded**: OS typically limits to 5 pending; older ones auto-dismissed
- **DND schedule parsing**: Invalid time formats logged and ignored

## Concurrency and Thread Safety

- `Arc<RwLock<NotificationSettings>>` for shared settings across tasks
- Notifications sent synchronously (non-blocking from caller perspective)