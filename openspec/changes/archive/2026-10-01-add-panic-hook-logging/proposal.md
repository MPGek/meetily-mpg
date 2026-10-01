# Proposal

## Why

The release build currently gives no usable evidence when it aborts. The 2026-09-18 crash of `target\release\meetily.exe` was a native abort (`0xC000001D`, a Rust `ud2` trap) 33 minutes into a session; the only artifacts left were a 282 MB minidump and Windows event-log entries. Because the shipped binary is a GUI app with no attached console, the Rust panic message, its source location, and the backtrace were lost, and reconstructing them required manual dump analysis. We need the next such failure to record its own cause (message + location + backtrace) to a file that survives the process.

## What Changes

- Install a process-wide panic hook at the start of `run()` in `frontend/src-tauri/src/lib.rs`, before the Tauri builder runs, so panics from any thread and any startup phase are captured.
- The hook appends one entry per panic to a persistent log file under the app data directory (`logs/panic.log`), containing: local timestamp, app version, process id, thread name, panic message, source location (`file:line:column`), and a forced backtrace (`std::backtrace::Backtrace::force_capture()`).
- Preserve existing behavior: the previously installed hook is still invoked, so stderr output in dev/console runs is unchanged.
- Make the hook panic-proof and side-effect-safe: all file I/O is best-effort (`let _ = ...`), the hook never unwraps, and a failure to write must not change or mask the original failure.
- Enable release line tables (`[profile.release] debug = "line-tables-only"`) so backtraces from the release binary resolve to source lines, not just symbol names.
- No new IPC command, no UI, no telemetry, no automatic crash upload.

## Capabilities

### New Capabilities
- `panic-logging`: capture Rust panics to a persistent on-disk log with message, source location, and backtrace, so post-mortem diagnosis does not require a debugger or dump analysis.

### Modified Capabilities
<!-- None: no existing capability's requirements change. -->

## Impact

- Code: `frontend/src-tauri/src/lib.rs` (`run()`), a new small module (e.g. `frontend/src-tauri/src/panic_log.rs`) registered in the module tree.
- Build config: the workspace root `Cargo.toml` (profiles there apply to `frontend/src-tauri`; member-level profiles are ignored) gains `[profile.release] debug = "line-tables-only"` (larger release PDB; no change to optimization or runtime behavior).
- Runtime artifacts: new append-only `logs/panic.log` under the Tauri app data directory (`%APPDATA%\com.meetily.ai\logs\panic.log` on Windows); directory is created on demand.
- Dependencies: none new (`std::backtrace`, `std::panic`, `chrono`, `dirs` already available).
- Out of scope: crashes that never construct a Rust panic (raw `abort()`, allocator failure, OS/driver faults) and crashes in the bundled WebView2/ffmpeg child processes; these are not observable to a panic hook.
