# Design

## Context

See `proposal.md` for motivation. Current state that shapes the approach:

- `frontend/src-tauri/src/audio/recording_commands.rs` (2416 lines) is a mix of three kinds of code today:
  1. **Real `#[tauri::command]` functions already thin**: `pause_recording`/`resume_recording`/`is_recording_paused`/`get_recording_state` (1479-1584) lock `RECORDING_MANAGER` and delegate to `RecordingManager` methods (`pause_recording`, `resume_recording`, `is_paused`, `is_active`, `get_recording_duration`, ... — all already implemented on `RecordingManager` in `recording_manager.rs:404-451`); `get_meeting_folder_path`/`get_transcript_history`/`get_recording_meeting_name`/`poll_audio_device_events`/`get_reconnection_status`/`get_active_audio_output` (1588-1741) are similarly short. These need no further extraction.
  2. **Real `#[tauri::command]` functions with real logic inline**: `attempt_device_reconnect` (1742-1792, includes the `spawn_blocking`+`block_on` pattern the audit flags at 1762-1776 for holding `RECORDING_MANAGER.lock().unwrap()` across `.await` — `04-recording-lock-hardening`'s concern, not this change's), `finalize_online_session` (1802-1994), `get_recording_telemetry` + its helper `online_diarization_status` (2045-2172), and `assign_live_speaker` (2184-2253).
  3. **Plain orchestration functions that are *not* `#[tauri::command]` at all**: `start_recording_with_meeting_name` (200-478), `start_recording_with_devices_and_meeting` (497-833), `resolve_microphone_device`/`resolve_system_audio_device` (834-901), and `stop_recording` (902-1465). The actual `#[tauri::command]`s with the names `start_recording_with_devices`/`start_recording_with_devices_and_meeting` are defined in **`lib.rs:295-382`**, and delegate into these plain functions (`lib.rs:335,345`); `lib.rs`'s own `start_recording`/`stop_recording`/`is_recording` commands (`lib.rs:83-217`) delegate the same way (`lib.rs:103,149,155,157,207`). `tray.rs:78,80,177,179,239,251` and `audio/common.rs:23` call the same plain functions directly (`crate::audio::recording_commands::{stop_recording, is_recording, RecordingArgs, is_recording_paused}`). This existing "thin `#[tauri::command]` in `lib.rs`, orchestration in the `audio` module" split is exactly the pattern this change extends to the rest of the file.
- `RecordingManager` (`recording_manager.rs`, 700 lines) already owns per-recording lifecycle state and methods (`start_recording`, `stop_recording`, `pause_recording`, `resume_recording`, `attempt_device_reconnect`, `handle_device_disconnect/reconnect`, ...). `recording_state.rs` (478 lines) owns the audio-chunk/error/device-type types `RecordingManager` uses. Neither file needs new public API for this change — the new `audio/recording/{lifecycle,devices,stop}.rs` files call into `RecordingManager` exactly as `recording_commands.rs` does today; only the *caller's location* moves.
- Module-scoped state currently declared in `recording_commands.rs` and used by the diarization-specific commands: `ONLINE_DIARIZATION_STORE` (live prototype store, `pub(crate)`, line 59), `ONLINE_SESSION_DATA` (`pub(crate)`, line 78, holding `OnlineSessionData` — cluster embeddings, live bindings, expected-speaker ids, raw mic/sys chunk embeddings), `ONLINE_TURN_OVERRIDES` (`pub(crate)`, line 99, holding `Vec<TurnOverride>`). `stop_recording`'s step 2.5 (roughly 1053-1156, "Finalize online diarization") populates `ONLINE_SESSION_DATA`; `assign_live_speaker` reads/writes `ONLINE_DIARIZATION_STORE` and pushes to `ONLINE_TURN_OVERRIDES`; `finalize_online_session` drains all three.
- `05-split-diarization-and-shared-speaker-match` is being redesigned as a unified live-first diarization engine under `audio/diarization/` (currently a single `audio/diarization.rs` file plus `audio/online_diarization.rs`; `05` is expected to fold both, and the state above, into that engine). This change (`07`) does not design that engine — it only states the minimal call surface it needs from it (below) and assumes `05` lands first.
- IPC surface: the `generate_handler!` macro (`lib.rs:611`) registers `audio::recording_commands::{pause_recording, resume_recording, is_recording_paused, get_recording_state, get_meeting_folder_path, get_transcript_history, get_recording_meeting_name, poll_audio_device_events, get_reconnection_status, attempt_device_reconnect, get_active_audio_output}` directly by path (`lib.rs:695-710`), `finalize_online_session`/`assign_live_speaker`/`get_recording_telemetry` a little further down (`lib.rs:887-890`, confirmed by grep), and separately registers `lib.rs`'s own `start_recording`, `stop_recording`, `is_recording`, `get_transcription_status`, `start_recording_with_devices`, `start_recording_with_devices_and_meeting` (`lib.rs:611-616,692-693`). None of these paths or names change in this design.

## Goals / Non-Goals

**Goals:**
- `recording_commands.rs` ends at or below ~600 lines and contains only `#[tauri::command]` functions (plus the small `RecordingArgs`/`TranscriptionStatus`/telemetry DTOs they return) that parse arguments, delegate, and map errors.
- Every `#[tauri::command]` function keeps its exact name and signature; `generate_handler!` at `lib.rs:611` is untouched.
- Every non-command caller of this file's plain functions (`lib.rs`, `tray.rs`, `audio/common.rs`) keeps compiling with zero source changes.
- State the exact 3-method facade interface `07` needs from `05`, so the two changes can be implemented independently once `05` commits to that shape.
- Existing tests (`transcript_update_tokens_survive_event_payload_roundtrip`, `transcript_segment_from_update_without_tokens_stays_none`, `recording_telemetry_reports_inactive_and_active_shapes` — 3 tests today, not the 2 the initial audit estimated) keep passing unmodified, calling the same public function names.

**Non-Goals:**
- Redesigning `05`'s diarization engine, its internal module layout, or where `ONLINE_DIARIZATION_STORE`/`ONLINE_SESSION_DATA`/`ONLINE_TURN_OVERRIDES` ultimately live (open question below).
- Fixing the lock-across-`.await` pattern in `attempt_device_reconnect` (04's scope) — this change relocates `stop_recording` and leaves `attempt_device_reconnect` in place untouched, both as-is, so 04 can land its fix without rebasing through this move.
- Refactoring `stop_recording`'s internal steps (audio stop, transcription drain, model unload, analytics, cleanup) into further sub-functions — task 3 moves the function body verbatim into `recording/stop.rs`; splitting `stop_recording` itself further is future work, not blocked by this change.
- Changing `RecordingManager`'s or `recording_state.rs`'s public API.

## Decisions

### D1: New `audio/recording/{mod,lifecycle,devices,stop}.rs`, re-exported from `recording_commands.rs`

Add `pub mod recording;` to `audio/mod.rs` (alongside the existing `pub mod recording_commands;`, `pub mod recording_manager;`, `pub mod recording_state;` — the new folder is a sibling, not a replacement, matching the brief's naming). `recording/mod.rs` declares `pub mod lifecycle; pub mod devices; pub mod stop;`. Each file gets the corresponding plain functions moved verbatim (module-private helpers like `resolve_microphone_device`/`resolve_system_audio_device` become `pub(crate)` only if a caller outside `audio::recording::devices` needs them directly — today only `lifecycle.rs` calls them, so they can stay `pub(super)` or `pub(crate)` at the implementer's discretion).

`recording_commands.rs` keeps the original names reachable via re-export:
```rust
pub use crate::audio::recording::lifecycle::{start_recording_with_meeting_name, start_recording_with_devices_and_meeting};
pub use crate::audio::recording::stop::stop_recording;
```
`is_recording`, `get_transcription_status`, and `RecordingArgs` are small enough (a handful of lines) that they are **not** moved — `is_recording()` is a one-line `IS_RECORDING.load(...)` read and `RecordingArgs` is a 3-line DTO; moving them would add an indirection for no readability gain, so they stay defined in `recording_commands.rs` as today.

- Why re-export rather than editing every call site: `lib.rs` (7 call sites), `tray.rs` (6), and `audio/common.rs` (1) all spell the path as `audio::recording_commands::X` or `crate::audio::recording_commands::X` (confirmed by `grep -rn` — see proposal.md's Impact section for the exact list). A `pub use` in `recording_commands.rs` makes `audio::recording::lifecycle::start_recording_with_devices_and_meeting` and `audio::recording_commands::start_recording_with_devices_and_meeting` the same item at two paths, so none of those 14 call sites need to change. This mirrors the `mod.rs`-re-export approach used in `06-split-speaker-repository` for the same reason.
- Alternative considered: update all 14 call sites to the new `audio::recording::...` paths and drop the re-export. Rejected — it inflates this change's diff into files (`lib.rs`, `tray.rs`) that otherwise have nothing to do with the split, for a purely cosmetic path difference.

### D2: The diarization engine facade — minimal interface `07` assumes from `05`

`07` needs exactly three operations from whatever concrete type/module `05` produces under `audio/diarization/`. This change treats the facade as an opaque handle obtained however `05` decides (a `tauri::State`, a field on `AppState`, or a free function — see Open Questions) and calls:

```rust
// Shape assumed, not defined by this change — 05 owns the concrete types.
async fn finalize_session(
    &self,
    pool: &SqlitePool,
    meeting_id: &str,
    session: OnlineSessionData,          // today's struct, recording_commands.rs:67-76
    turn_overrides: Vec<TurnOverride>,   // today's struct, recording_commands.rs:89-94
) -> Result<FinalizeSessionOutcome, String>;
// FinalizeSessionOutcome { live_bindings: usize, enrolled: usize } — the two counts
// finalize_online_session currently returns as ad-hoc `serde_json::json!({...})` fields
// (recording_commands.rs:1989-1993).

fn assign_live_speaker(
    &self,
    cluster_label: &str,
    speaker_id: &str,
    speaker_name: &str,
    scope: LiveAssignScope,              // Cluster | Block { start_secs: f64, end_secs: f64 }
) -> Result<(), String>;
// Replaces assign_live_speaker's direct ONLINE_DIARIZATION_STORE / ONLINE_TURN_OVERRIDES
// access (recording_commands.rs:2222-2246).

fn telemetry_snapshot(&self) -> OnlineDiarizationStatus;   // today's struct, unchanged
// Replaces the free function `online_diarization_status()` (recording_commands.rs:2045-2117).
```

With this interface, the three commands become:
- `finalize_online_session`: take `ONLINE_SESSION_DATA`/`ONLINE_TURN_OVERRIDES` (wherever `05` leaves them — see Open Questions), and either call `facade.finalize_session(...)` if `05` owns draining them, or drain them here and pass the values in. Map the returned `FinalizeSessionOutcome` to the same `serde_json::json!({"meeting_id":..., "live_bindings":..., "enrolled":...})` shape so the frontend contract is unchanged.
- `get_recording_telemetry`: unchanged assembly of `pipeline`/`models.vad`/`models.asr`/`models.alignment` (these are not diarization concerns); replace the `online_diarization_status().await?` call with `facade.telemetry_snapshot()`.
- `assign_live_speaker`: unchanged argument resolution (find-or-create the registry speaker via `SpeakerRepository`, still a direct DB call — that is speaker-registry, not live-diarization-engine, state) followed by `facade.assign_live_speaker(...)` instead of the inline `ONLINE_TURN_OVERRIDES.lock().unwrap().push(...)` / `ONLINE_DIARIZATION_STORE` branch.

- Why these exact 3 methods and not more: they are precisely the 3 places in this file where diarization-engine-internal state (`ONLINE_DIARIZATION_STORE`, `ONLINE_SESSION_DATA`, `ONLINE_TURN_OVERRIDES`) is read or written directly from a command body. Everywhere else this file touches diarization, it only reads a value already computed elsewhere (e.g. `stop_recording`'s step 2.5 collects into `ONLINE_SESSION_DATA` — see Non-Goals: that collection point is `05`'s to fold into the engine or leave as-is; `07` does not change it).
- Alternative considered: have `07` also move `stop_recording`'s step 2.5 (diarization finalize collection) behind the facade in this same change. Rejected — the brief's explicit scope for the facade boundary is "online-diarization finalize + telemetry 1802-2183, live speaker assignment 2184+"; step 2.5 sits inside `stop_recording` (902-1465) and is relocated as part of D1's verbatim move to `stop.rs`, not refactored. Bundling a second facade touch-point into `07` would make `07`'s completion depend on more of `05`'s design than necessary.

### D3: Line-count target and what stays in `recording_commands.rs`

After D1 and D2, `recording_commands.rs` contains: the 14 `#[tauri::command]` functions (now all thin), `is_recording`/`get_transcription_status`-adjacent small helpers, the DTOs they return (`RecordingArgs`, `TranscriptionStatus`, `DeviceEventResponse`, `ReconnectionStatus`, `DisconnectedDeviceInfo`, `PipelineStatus`, `DiarizationModelActivity`, `ModelsActivity`, `RecordingTelemetry`), the 3 `pub(crate) static`/struct diarization-session items (until `05` claims them — Open Questions), the `transcript_segment_from_update`/`meeting_span_source` helpers (129-188, used by the transcript-update listener wired in `lifecycle.rs`'s moved code — these stay in `recording_commands.rs` only if nothing in `lifecycle.rs` needs them privately; otherwise they move too, whichever keeps both files compiling with the least indirection — left to the implementer since it does not affect any external contract), and the 3 existing tests. This is estimated well under 600 lines (the removed material — 337 + 564 + 68 + 193 + 73 lines from lifecycle/devices/stop/finalize/telemetry-status alone — is over 1200 lines, more than half the file).

### D4: Ordering — this change applies after `04` and `05`

`attempt_device_reconnect`'s lock pattern (1762-1776) and `stop_recording`'s internal locking are `04`'s concern; if `07` relocates that code to `recording/stop.rs` before `04` lands, `04` would have to redo its patch against the new file. Applying `07` after `04` means `recording/stop.rs` is created already containing `04`'s fix. Likewise, `07`'s D2 facade calls do not compile until `05` provides the facade — `07`'s tasks that touch `finalize_online_session`/`get_recording_telemetry`/`assign_live_speaker` are the last tasks in this change specifically so the bulk of the split (D1) can proceed and be verified independently of `05`'s timeline, with only the facade-dependent tasks blocked on `05`.

## Risks / Trade-offs

- **`05` ships a facade shaped differently from D2's assumption** → Mitigated by keeping the facade-dependent tasks (finalize/telemetry/assign) last and separately verifiable; if the shape differs, only those tasks' call sites need adjusting, not the D1 file split. This design's exact method names/signatures should be confirmed with `05`'s author before those tasks are applied (see Open Questions).
- **Re-export indirection (`pub use audio::recording::lifecycle::X`) obscures where `X` is actually defined** → Accepted: it is the standard Rust idiom for a no-caller-change module split, and `cargo doc`/IDE "go to definition" both resolve through it transparently.
- **`ONLINE_SESSION_DATA`/`ONLINE_TURN_OVERRIDES`/`ONLINE_DIARIZATION_STORE` ownership is left ambiguous by this design** → This is deliberately deferred to `05` (Open Questions) rather than guessed at, since guessing wrong would mean redoing this change's facade-dependent tasks anyway.
- **Moving `stop_recording` (564 lines) verbatim leaves one very large function**, just in a different file → Accepted as a Non-Goal for this change; the brief's ask is "orchestration moves to `recording_manager.rs` (or a new folder)," not "stop_recording is decomposed," and decomposing a function with 5 sequential steps each touching different subsystems (audio capture, transcription drain, model unload, analytics, cleanup) is a larger, separately-reviewable effort.

## Migration Plan

Pure relocation plus a facade call-site swap; no data migration, no IPC change. Sequenced as: (1) create `audio/recording/{mod,lifecycle,devices,stop}.rs` and re-export, verified independently of `05`; (2) once `05` lands, swap the 3 facade call sites. If `05` is delayed, step (1) can still land and this change's remaining tasks wait — `recording_commands.rs` is already smaller and every other command already thin.

## Open Questions

- Where do `ONLINE_SESSION_DATA`, `ONLINE_TURN_OVERRIDES`, and `ONLINE_DIARIZATION_STORE` live once `05` lands — subsumed into the engine's internal state (so `07`'s commands only ever call the facade and never touch these statics again), or do they stay as `recording_commands.rs`-owned state that is merely passed into facade calls (this design's assumption)? Needs `05`'s author's decision before the facade-dependent tasks in this change are applied.
- What concrete type/handle does a `#[tauri::command]` in this file use to reach the facade — a new field on `crate::state::AppState` (already threaded into `finalize_online_session`/`assign_live_speaker` via `tauri::State<'_, AppState>`), or a module-level accessor like `crate::audio::diarization::engine()`? This determines the exact one-line change at each of the 3 call sites but does not affect this change's file layout (D1) or scope.
- Confirm with `05`'s author that `FinalizeSessionOutcome { live_bindings, enrolled }` (or an equivalently-shaped result) is what `finalize_session` returns, so `finalize_online_session`'s `serde_json::json!({...})` response body (a wire contract the frontend already reads) can be reproduced unchanged.
