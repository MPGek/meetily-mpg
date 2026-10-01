# app-handle-thread-safety Specification

## Purpose

Keeps the desktop app running when application, window and webview handles are cloned and dropped from background threads at the same time as the main event loop uses them. This matters most during long recordings, when background work touches those handles constantly.

## Requirements

### Requirement: Off-main-thread handle use does not corrupt shared event-loop state

On Windows, the system SHALL keep the shared event-loop state referenced by application, window and webview handles intact when those handles are cloned or dropped on background threads at the same time as the main thread clones or drops them. No interleaving of such operations SHALL abort the process or free that state while a handle still refers to it.

#### Scenario: Long recording with the app unattended
- **WHEN** a recording runs for at least 45 minutes on Windows in the release build while the app window stays open but unused
- **THEN** the process SHALL NOT terminate with `STATUS_ILLEGAL_INSTRUCTION (0xC000001D)` or any other crash, and the recording SHALL stop and save normally afterwards

#### Scenario: Frontend IPC during background activity
- **WHEN** the frontend invokes commands that take an application, window or webview handle while background recording, transcription and diarization tasks are active
- **THEN** every such command SHALL complete or fail with an ordinary error, and the main thread SHALL NOT abort while it looks up or clones a webview

### Requirement: The release build fails verification if the event-loop fix is not applied

The project SHALL provide an automated check, runnable with the app's normal test command, that fails when the dependency fix providing the requirement above is not the one actually resolved into the build. A dependency update that silently drops the fix SHALL be caught before release.

#### Scenario: Dependency resolves to the fixed copy
- **WHEN** the check runs against the committed lockfile with the fix in place
- **THEN** it SHALL pass

#### Scenario: Dependency update bypasses the fix
- **WHEN** a lockfile update makes the affected dependency resolve from the public registry instead of the fixed copy
- **THEN** the check SHALL fail with a message saying the fix is no longer applied and pointing to where it is documented
