# Proposal

## Why

`frontend/src-tauri/src/audio/recording_commands.rs` is 2416 lines. Some of its 14 `#[tauri::command]`-tagged functions are already thin delegations to `RecordingManager` (`pause_recording`, `resume_recording`), but others hold real orchestration inline: `finalize_online_session` (1802-1994, ~193 lines) does speaker-registry persistence and enrollment directly, `get_recording_telemetry`/`online_diarization_status` (2045-2172) assembles a live diarization snapshot inline, and `assign_live_speaker` (2184-2253) mutates live diarization state inline. Two more functions in this file are not commands at all but large plain orchestration functions called by thin wrappers defined in `lib.rs`: `start_recording_with_devices_and_meeting` (497-833, 337 lines) and `stop_recording` (902-1465, 564 lines). A command file mixing thin wrappers, fat command bodies, and non-command orchestration in one 2416-line file is hard to review and hard to keep IPC-contract-safe when changed.

## What Changes

- Keep `frontend/src-tauri/src/audio/recording_commands.rs` as the thin `#[tauri::command]` layer only: argument parsing, delegation to orchestration code, `Result<T, String>` mapping, and event emission — nothing else. Target: at or below ~600 lines (from 2416).
- Move the large plain orchestration functions to a new `frontend/src-tauri/src/audio/recording/` module:
  - `recording/lifecycle.rs`: `start_recording_with_meeting_name` (200-478) and `start_recording_with_devices_and_meeting` (497-833).
  - `recording/devices.rs`: `resolve_microphone_device` (834-864) and `resolve_system_audio_device` (865-901).
  - `recording/stop.rs`: `stop_recording` (902-1465).
  `recording_commands.rs` re-exports these under their original names (`pub use crate::audio::recording::{lifecycle::*, stop::stop_recording};`) so every existing caller path (`audio::recording_commands::start_recording_with_devices_and_meeting`, `::start_recording_with_meeting_name`, `::stop_recording`, `::is_recording`, `::RecordingArgs` — used from `lib.rs:103,149,155,157,207,335,345`, `tray.rs:78,80,177,179,239,251`, and `audio/common.rs:23`) keeps compiling unchanged.
- Slim the three commands that currently embed diarization-specific business logic, by delegating to the diarization engine facade that `05-split-diarization-and-shared-speaker-match` is redesigning (see design.md for the exact 3-method interface this change assumes):
  - `finalize_online_session` becomes argument handling + one call to `facade.finalize_session(...)` + `Result` mapping.
  - `get_recording_telemetry` keeps assembling the non-diarization parts (pipeline, VAD/ASR/alignment activity) as it does today, but replaces its inline `online_diarization_status()` (2045-2117) with `facade.telemetry_snapshot()`.
  - `assign_live_speaker` becomes argument handling + one call to `facade.assign_live_speaker(...)` + `Result` mapping.
- Everything else in the file that is already a thin wrapper (`pause_recording`, `resume_recording`, `is_recording_paused`, `get_recording_state`, `get_meeting_folder_path`, `get_transcript_history`, `get_recording_meeting_name`, `poll_audio_device_events`, `get_reconnection_status`, `get_active_audio_output`, `attempt_device_reconnect`) stays in place; `attempt_device_reconnect`'s current lock-across-`.await` pattern (1762-1776) is left to `04-recording-lock-hardening` (this change applies after 04 and after 05, and only relocates code — it does not change locking behavior).
- No IPC contract change: the `generate_handler!` list at `lib.rs:611` and the `#[tauri::command]` function names/signatures it registers are unchanged (verified with `grep -rn` for each command name used from `frontend/src`).

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
<!-- None: no spec-level behavior changes. See `skip_specs: true` in .openspec.yaml. -->

## Impact

- Code: `frontend/src-tauri/src/audio/recording_commands.rs` (2416 → ≤~600 lines); new `frontend/src-tauri/src/audio/recording/{mod,lifecycle,devices,stop}.rs`; `frontend/src-tauri/src/audio/mod.rs` gains `pub mod recording;`.
- Depends on: `04-recording-lock-hardening` (must land first — this change relocates `attempt_device_reconnect` and `stop_recording`'s lock usage as-is, and does not want to relocate code that 04 is about to change) and `05-split-diarization-and-shared-speaker-match` (must land first — this change calls the facade `05` is building; see design.md for the minimal interface assumed).
- Callers unaffected: `lib.rs` (`start_recording_with_devices`, `start_recording_with_devices_and_meeting` commands defined there, plus the 3 direct `audio::recording_commands::` calls in `start_recording`/`stop_recording`/`is_recording`), `tray.rs`, `audio/common.rs`, and the frontend's `invoke(...)` calls (verified by grepping each of the 14 command names, e.g. `finalizeOnlineSession`, `assignLiveSpeaker`, `pauseRecording`, in `frontend/src`) all keep working with zero changes.
- Out of scope: the diarization engine facade's own implementation (owned by `05`); the lock-hardening changes in `attempt_device_reconnect`/`stop_recording` (owned by `04`); any change to `recording_manager.rs`'s or `recording_state.rs`'s existing public API (both are only called into, not modified, except that `recording/lifecycle.rs`, `recording/devices.rs`, `recording/stop.rs` now sit alongside them as siblings under `audio/`).
