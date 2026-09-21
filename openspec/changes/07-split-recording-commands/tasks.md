# Tasks

## 0. Preconditions

- [ ] 0.1 Confirm `04-recording-lock-hardening` and `05-split-diarization-and-shared-speaker-match` are archived (applied) before starting task 4 of this change; tasks 1-3 do not depend on either and may proceed regardless; verify: `openspec list` shows both as archived, or the repo owner confirms in the change discussion.
- [ ] 0.2 Record the pre-split baseline: `wc -l frontend/src-tauri/src/audio/recording_commands.rs` (expect `2416`), `cargo test -p meetily --lib recording_commands -- --list | grep -c ": test"` (expect `3`: `transcript_update_tokens_survive_event_payload_roundtrip`, `transcript_segment_from_update_without_tokens_stays_none`, `recording_telemetry_reports_inactive_and_active_shapes`), and the full list of command names this file registers or is called by, from `grep -n "audio::recording_commands::" frontend/src-tauri/src/lib.rs frontend/src-tauri/src/tray.rs frontend/src-tauri/src/audio/common.rs`; verify: all three commands run and their output is noted in the PR description.

## 1. Scaffold `audio/recording/`

- [ ] 1.1 Create `frontend/src-tauri/src/audio/recording/mod.rs` with `pub mod lifecycle; pub mod devices; pub mod stop;`; add `pub mod recording;` to `frontend/src-tauri/src/audio/mod.rs` (next to the existing `pub mod recording_commands;`/`recording_manager;`/`recording_state;`); leave the three new files empty for now; verify: `cargo check -p meetily` succeeds.

## 2. Move device resolution (no dependents yet)

- [ ] 2.1 Move `resolve_microphone_device` and `resolve_system_audio_device` (currently `frontend/src-tauri/src/audio/recording_commands.rs:834-901`) verbatim into `frontend/src-tauri/src/audio/recording/devices.rs`; update `recording_commands.rs` to `use crate::audio::recording::devices::{resolve_microphone_device, resolve_system_audio_device};` at the one call site each is used from (inside `start_recording_with_devices_and_meeting`, still in `recording_commands.rs` at this point); verify: `cargo check -p meetily`.

## 3. Move lifecycle and stop orchestration

- [ ] 3.1 Move `start_recording_with_meeting_name` (200-478) and `start_recording_with_devices_and_meeting` (497-833) verbatim into `frontend/src-tauri/src/audio/recording/lifecycle.rs`, updating their `resolve_microphone_device`/`resolve_system_audio_device` calls to `super::devices::...`; add `pub use crate::audio::recording::lifecycle::{start_recording_with_meeting_name, start_recording_with_devices_and_meeting};` to `recording_commands.rs`; verify: `cargo check -p meetily` and `grep -rn "audio::recording_commands::start_recording_with_devices_and_meeting\|audio::recording_commands::start_recording_with_meeting_name" frontend/src-tauri/src` still resolves (compiles) from `lib.rs:335,345` with no changes to those call sites.
- [ ] 3.2 Move `stop_recording` (902-1465) verbatim into `frontend/src-tauri/src/audio/recording/stop.rs`; add `pub use crate::audio::recording::stop::stop_recording;` to `recording_commands.rs`; verify: `cargo check -p meetily` and `grep -rn "audio::recording_commands::stop_recording\|crate::audio::recording_commands::stop_recording" frontend/src-tauri/src` still resolves from `lib.rs:155`, `tray.rs:78,177` with no changes to those call sites.
- [ ] 3.3 Run the full non-facade verification: `cargo check -p meetily`, `cargo clippy -p meetily --all-targets --message-format=short` (no new warnings attributable to this move beyond pre-existing ones), `cargo test -p meetily --lib recording_commands` (3 tests, all passing) and `cargo test -p meetily --lib recording` (new `lifecycle`/`devices`/`stop` modules — 0 tests expected, since none moved), `wc -l frontend/src-tauri/src/audio/recording_commands.rs` (expect roughly 2416 − 337 − 564 − 68 ≈ 1450, still well above the ~600 target because tasks 4-6 have not run yet); verify: all commands succeed with the stated results.

## 4. Facade-dependent: `finalize_online_session`

- [ ] 4.1 Confirm the facade interface with `05`'s implementation (design.md D2's `finalize_session`, `assign_live_speaker`, `telemetry_snapshot`); if the shape differs from design.md, update design.md's D2 section to match before proceeding; verify: design.md's D2 code block matches the actual facade signatures in `audio/diarization/`.
- [ ] 4.2 Replace `finalize_online_session`'s body (1802-1994) with: read/drain `ONLINE_SESSION_DATA` and `ONLINE_TURN_OVERRIDES` (or omit this step if `05` subsumed them into the engine — see design.md Open Questions), call `facade.finalize_session(pool, &meeting_id, session_data, turn_overrides).await`, map the result into the same `serde_json::json!({"meeting_id": ..., "live_bindings": ..., "enrolled": ...})` shape the frontend already receives; verify: `cargo check -p meetily` and manually diff the JSON shape against the pre-change version (same 3 keys, same types).

## 5. Facade-dependent: `get_recording_telemetry`

- [ ] 5.1 Replace the `online_diarization_status()` free function (2045-2117) and its call site inside `get_recording_telemetry` (2128) with `facade.telemetry_snapshot()`, keeping the rest of `get_recording_telemetry`'s assembly (`pipeline`, `models.vad`/`models.asr`/`models.alignment`) unchanged; verify: `cargo test -p meetily --lib recording_commands::tests::recording_telemetry_reports_inactive_and_active_shapes` passes unmodified (this test exercises the full JSON wire shape, including the diarization sub-object, so it pins the facade's output shape).

## 6. Facade-dependent: `assign_live_speaker`

- [ ] 6.1 Replace `assign_live_speaker`'s inline `ONLINE_TURN_OVERRIDES.lock().unwrap().push(...)` / `ONLINE_DIARIZATION_STORE` branch (2222-2246) with `facade.assign_live_speaker(&cluster_label, &speaker.id, &speaker.name, scope)`, where `scope` is `LiveAssignScope::Block { start_secs, end_secs }` when `scope == Some("block")` and `start_time` is set, else `LiveAssignScope::Cluster` — preserving the existing default-window logic (`end_time.filter(|e| *e > start).unwrap_or(start + 1.0)`) either in this command or inside the facade call, whichever `05`'s interface expects; keep the speaker find-or-create logic (`SpeakerRepository::get_speaker`/`find_or_create_by_name`) unchanged, since that is registry state, not live-engine state; verify: `cargo check -p meetily`.

## 7. Final verification

- [ ] 7.1 Confirm no IPC contract change: `grep -n "recording_commands::\|start_recording\|stop_recording\|is_recording\b" frontend/src-tauri/src/lib.rs` shows the same command names at the same `generate_handler!` entries as task 0.2's baseline (no additions, removals, or renames); verify: diff against the task 0.2 baseline is empty.
- [ ] 7.2 Confirm the frontend contract is untouched: for each of the 14 command names (`pause_recording`, `resume_recording`, `is_recording_paused`, `get_recording_state`, `get_meeting_folder_path`, `get_transcript_history`, `get_recording_meeting_name`, `poll_audio_device_events`, `get_reconnection_status`, `attempt_device_reconnect`, `get_active_audio_output`, `finalize_online_session`, `get_recording_telemetry`, `assign_live_speaker`), `grep -rn "invoke(['\"]<name>" frontend/src` still finds the same call sites as before this change (no frontend file needs to change); verify: command names are unchanged in `frontend/src/services/recordingService.ts` and `frontend/src/components/RecordingControls.tsx`.
- [ ] 7.3 Run `cargo check -p meetily`, `cargo clippy -p meetily --all-targets --message-format=short`, and `cargo test -p meetily --lib recording_commands` / `cargo test -p meetily --lib recording` (all passing, 3 tests total, none added or removed); `wc -l frontend/src-tauri/src/audio/recording_commands.rs` is at or below `600`; verify: all commands succeed with the stated results.
- [ ] 7.4 Run `openspec validate 07-split-recording-commands --strict` and `openspec status --change 07-split-recording-commands`; verify: validate passes with no errors and status shows every task complete.
