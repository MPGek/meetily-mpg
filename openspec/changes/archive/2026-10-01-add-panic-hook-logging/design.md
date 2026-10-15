# Design

## Context

See `proposal.md` for motivation. Current state that shapes the approach:

- The workspace root `Cargo.toml` (`C:\...\meetily-mpg\Cargo.toml`) is the Cargo workspace and already holds profiles (`[profile.dev]`, `[profile.dev.package."*"]`). `frontend/src-tauri` is a member, so any `[profile.release]` must be added to the root file, not to `frontend/src-tauri/Cargo.toml`.
- `frontend/src-tauri/src/main.rs` is `windows_subsystem = "windows"` in release and initializes `env_logger`. On Windows in release there is no console, so stderr output is lost, which is why the 2026-09-18 abort left no message.
- `app_lib::run()` (`frontend/src-tauri/src/lib.rs`) is the application entry: it sets `log::set_max_level`, builds the Tauri app, registers plugins/commands, and runs a `.setup(...)` closure where `AppHandle` (and thus `path().app_data_dir()`) becomes available.
- The app data directory is `%APPDATA%\com.meetily.ai` (Tauri `app_data_dir()`); it already holds the SQLite DB, models, and preferences, and is created/used across the codebase.
- Existing diagnostics: `llm-debug-logging` writes per-meeting JSON logs to the meeting folder; it is LLM-specific and does not cover panics. There is no existing panic hook (`std::panic::set_hook`/`take_hook`) anywhere in `src-tauri/src`.
- The `chrono`, `dirs`, `serde`/`serde_json` crates and `std::backtrace` are already available; no new dependency is needed.
- Analysis of the 2026-09-18 dump (`%LOCALAPPDATA%\CrashDumps\meetily.exe.67408.dmp`) showed the abort came from a `ud2` trap on the main thread inside Tauri/WebView2 IPC message handling. That specific failure may or may not have gone through the panic runtime, which is a key constraint on what this design can promise.

## Goals / Non-Goals

**Goals:**

- Record every Rust panic (message, source `file:line:column`, thread, timestamp, version, pid, backtrace) to a durable file that survives the process.
- Install early enough to catch startup and background-thread panics.
- Keep the recording path panic-proof: it must never be the reason the process fails differently, and it must not change existing panic output.
- Keep the hook logic unit-testable without writing into the real app data directory.

**Non-Goals:**

- Capturing failures that never construct a Rust panic: raw `abort()`, `handle_alloc_error` (allocator failure), `unreachable_unchecked`, or OS/driver faults. (Windows minidumps remain the tool for those.)
- Capturing crashes in child processes (bundled `ffmpeg`, WebView2 processes).
- Crash upload, telemetry, analytics, or any UI surface.
- Log rotation/retention policy.
- Adding or changing any Tauri command or IPC contract.

## Decisions

### D1: Install the hook at the top of `run()`, before the Tauri builder

Call `panic_log::install()` as the first statement of `pub fn run()`, before `tauri::Builder::default()`.

- Why: startup and background-thread panics are then covered, and the hook exists for the whole process lifetime.
- Alternative considered: install inside `.setup(...)`, where `app.path().app_data_dir()` is directly available. Rejected because panics between `run()` entry and `setup()` would be missed, and it couples the hook to Tauri's lifecycle.
- The proposal requirement "before the main window and background workers are created" is satisfied: `setup()` and plugin/worker initialization all happen after the builder starts.

### D2: Resolve the log directory without an `AppHandle`, and pin it with a test

The hook resolves the path itself: `dirs::data_dir()? / "com.meetily.ai" / "logs" / "panic.log"`, matching Tauri's `app_data_dir()` layout on Windows.

- `dirs::data_dir()` returns `%APPDATA%` and Tauri's `app_data_dir()` is `%APPDATA%\{identifier}`, so the two agree.
- To prevent drift, a single `const APP_IDENTIFIER: &str = "com.meetily.ai"` is used, and a unit test asserts it equals the `identifier` in `frontend/src-tauri/tauri.conf.json` (read with `include_str!` + `serde_json`).
- If `dirs::data_dir()` is `None`, fall back to `std::env::temp_dir().join("meetily-panic")`. This keeps the hook from doing nothing in unusual environments, at the cost of a less discoverable path.
- Alternative considered: pass `app_data_dir()` from `.setup()` into a `OnceLock`. Rejected as unnecessary indirection: the identifier-based path is equivalent, and the drift test gives the same safety with less coupling.
- Alternative considered: write next to the executable. Rejected because the install directory may be read-only (Program Files).

### D3: Preserve the previously installed hook

`install()` takes the current hook with `std::panic::take_hook()` and, inside the new hook, invokes it first, then records the entry.

- Why: development/console runs keep the standard panic output; the change is additive to observable behavior.
- The captured `previous` hook is moved into the closure; it is called exactly once per panic.
- A panic inside the hook itself is unrecoverable in Rust (aborts), which is acceptable because the hook body is deliberately minimal and allocation-light.

### D4: Use `Backtrace::force_capture()` rather than the `backtrace` crate

Capture with `std::backtrace::Backtrace::force_capture()` and format with `Display`.

- `force_capture` ignores `RUST_BACKTRACE`, so release builds record a trace regardless of environment.
- Formatted output includes symbol names, and `file:line` where debug line info is available.
- `std::backtrace` is stable since Rust 1.65 and the crate's MSRV is 1.77, so no new dependency (the `backtrace` crate) is needed.

### D5: Message extraction from the panic payload

Downcast the payload to `&str`, then `String`; otherwise write a fixed marker such as `<non-string panic payload; type unavailable>`.

- `panic_info.location()` yields `Option<&Location>`; when `None`, write `unknown`.
- Thread name from `std::thread::current().name()`, falling back to `<unnamed>`.

### D6: Plain-text, delimited, append-only entry format

Each entry is a human-readable block beginning with a `==== panic @ <RFC3339 local timestamp> ====` separator and labeled fields (`version`, `pid`, `thread`, `location`, `message`), followed by the indented backtrace. The file is opened with `create(true).append(true)`.

- Why plain text over JSON: the primary consumer is a developer reading or pasting a crash report; the existing JSON convention (`llm-debug-logging`) exists for machine-parsed LLM payloads, which is a different use case.
- Append-only preserves earlier entries; the app's single-instance plugin means concurrent writers are not expected, and small append writes are effectively atomic on Windows.
- All I/O uses `let _ = ...`; a failure to create the directory or write produces no secondary panic and no error surfaced to the user.

### D7: Enable release line tables in the workspace root profile

Add to the workspace root `Cargo.toml`:

```toml
[profile.release]
debug = "line-tables-only"
```

- Why: `debug = false` (the default, and `[profile.dev]` is already `debug = false` too) leaves backtrace frames with symbol names but no `file:line` for application code. Line tables add the minimum needed for `file:line` at much smaller cost than `debug = 1`/`2`.
- The panic *location* (`file:line:column`) is compiled in regardless of debug info, so the exact panic line is recorded even without this change; D7 only improves caller frames in the backtrace.
- Alternative considered: `debug = 1` (full info) — rejected as unnecessary PDB growth. Alternative considered: no profile change — rejected because multi-frame backtraces without line info are much harder to act on.
- Note: symbolization needs the `.pdb` present next to the executable at runtime. Dev runs (`target\release\meetily.exe`) have it; an installer that omits the PDB will record addresses/module+RVA instead (see Risks).

### D8: Dedicated `panic_log` module with an injectable path for tests

New `frontend/src-tauri/src/panic_log.rs` (registered as `pub mod panic_log;` in `lib.rs`), exposing:

- `pub fn install()` — idempotent (guarded by an `AtomicBool`), installs the hook, resolves the default path.
- a private internal writer `fn write_entry(path: &Path, info: &PanicHookInfo) -> io::Result<()>` (plus small pure helpers for formatting the message/location/entry), so tests can write to a temporary directory and assert on the produced text.

- Why a module rather than inline code in `run()`: the formatting/entry logic is unit-testable this way, and `run()` stays readable.
- Tests use `tempfile` (already a dev-dependency) and `std::panic::catch_unwind` to invoke the writer/formatting directly, not by triggering a real panic.

## Risks / Trade-offs

- **The hook cannot see non-panic aborts** (raw `abort()`, allocator failure, `unreachable_unchecked`, OS faults). The 2026-09-18 `ud2` may have been either a panic-abort (hook records it) or a bare abort (hook records nothing) → If `panic.log` stays empty after a future crash, treat it as evidence the failure bypassed the panic runtime and fall back to dump analysis; document this so an empty log is not read as "no panic happened".
- **Unbounded `panic.log` growth** → Accepted for now: panics are rare and each entry is a few KB; a rotation cap can be added later without changing the specs.
- **Release line tables change build artifacts** → Adds a workspace-wide `[profile.release]` and invalidates the release build cache, and produces a larger PDB. Mitigation: the setting is isolated in its own task so it can be dropped, and it does not change optimization or runtime behavior.
- **PDB is not shipped by installers** → In-field line-level backtraces degrade to symbols/addresses. Mitigation: the panic *location* is still exact in every build; shipping PDBs is left as an open question.
- **Hook runs on the panic path only** → No steady-state performance cost; the backtrace capture cost is paid solely while already failing.
- **Drift between the identifier constant and `tauri.conf.json`** → Mitigated by the D2 unit test; if it ever diverges, the log would land in a sibling directory rather than fail.

## Migration Plan

- Additive change with no data or IPC migration. Deploy by shipping the new build.
- The workspace `[profile.release]` addition is the only change that alters build configuration: it requires a full release rebuild once, and cannot be "partially" applied.
- Rollback: revert the commit; the `panic.log` file, if present, is inert and can be left in place.

## Open Questions

- Should packaged installers ship the `.pdb` (or a symbol server) so end-user machines also get `file:line` backtrace frames?
- Should `panic.log` get a size cap or rotation, and should old entries be trimmed on startup?
- Is a periodic "no panics" signal useful, or is an empty/absent log the right success state?
