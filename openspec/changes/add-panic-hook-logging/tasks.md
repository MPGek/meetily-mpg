# Tasks

> Stale premises (re-verified 2026-10-01): `run()` is now at `lib.rs:396`; MSRV is 1.80, not 1.77; no panic hook existed; `main.rs` still `windows_subsystem = "windows"` + `env_logger`; root `Cargo.toml` had only `[profile.dev]`/`[profile.dev.package."*"]`; identifier `com.meetily.ai` unchanged.

## 1. Panic log module

- [x] 1.1 Create `frontend/src-tauri/src/panic_log.rs` with the entry-formatting helpers (timestamp, version, pid, thread name with `<unnamed>` fallback, message extraction with `<non-string panic payload>` fallback, `file:line:column` with `unknown` fallback) and register `pub mod panic_log;` in `frontend/src-tauri/src/lib.rs`; verify with `cargo check -p meetily`.
- [x] 1.2 Implement the append-only writer that creates the parent directory and opens the log with `create(true).append(true)`, writes a delimited entry per panic, and never panics (all I/O as `let _ = ...`), exposed as a small function taking the target path; verify with unit tests in `panic_log.rs` that write two entries to a `tempfile` directory and assert both are present and the first is unchanged.
- [x] 1.3 Implement `install()`: guard with an `AtomicBool` so repeated calls are no-ops, capture the previous hook via `std::panic::take_hook()`, and install a hook that first invokes the previous hook, then appends the entry using `Backtrace::force_capture()`; verify with a unit test that calls `install()` twice and then triggers `panic!` inside `std::panic::catch_unwind`, asserting the panic still propagates through `catch_unwind` and no secondary panic occurs.
  - Note (2026-10-01): the unit test calls the private `install_at(path)` twice (the guarded body of `install()`), not `install()` itself, so the test process never writes into the real `%APPDATA%\com.meetily.ai\logs`. Because the hook is process-wide and one-shot, 1.3 and 3.1 share one test (`installed_hook_records_real_panic_and_propagates`).
- [x] 1.4 Resolve the default log path as `dirs::data_dir()/com.meetily.ai/logs/panic.log` via a single `const APP_IDENTIFIER`, with `std::env::temp_dir()/meetily-panic` as fallback when `dirs::data_dir()` is `None`; verify with a unit test that asserts `APP_IDENTIFIER` equals the `identifier` in `frontend/src-tauri/tauri.conf.json` (read via `include_str!` + `serde_json`).
  - Note (2026-10-01): MSRV is now 1.80 (was 1.77 in design D4); `PanicHookInfo` is only nameable from 1.81, so the module never names the hook-info type: helpers take `&(dyn Any + Send)` and `Option<&Location>` instead of `&PanicHookInfo` (D8 signature adapted).
- [x] 1.5 Call `panic_log::install()` as the first statement of `pub fn run()` in `frontend/src-tauri/src/lib.rs`, before `tauri::Builder::default()`; verify with `cargo check -p meetily` and by confirming the call appears before the builder in the diff.

## 2. Release line tables

- [ ] 2.1 Add `[profile.release]` with `debug = "line-tables-only"` to the workspace root `Cargo.toml` (not `frontend/src-tauri/Cargo.toml`); verify with `cargo build --release -p meetily` and confirm `target/release/meetily.pdb` is produced/regenerated.
  - Done in config: `[profile.release] debug = "line-tables-only"` added to the workspace root `Cargo.toml`; `cargo metadata`/`cargo check`/clippy accept it. Left unchecked: verification needs a full `cargo build --release -p meetily` (not run, per instructions) — user to run and confirm `target/release/meetily.pdb` is regenerated.

## 3. Verification

- [x] 3.1 Add a test that routes the hook to a temporary file and triggers a real `panic!("...")` inside `catch_unwind`, then asserts the file contains the panic message text, a `location:` value containing `panic_log.rs:` and the test line, and a non-empty backtrace; verify with `cargo test -p meetily --lib panic_log`.
- [ ] 3.2 Manually smoke-test the installed hook: temporarily add a `panic!("panic-log smoke test")` at the end of `run()`, run the app once, confirm an entry appears in `%APPDATA%\com.meetily.ai\logs\panic.log` with message, location, and backtrace, then remove the temporary panic; verify the log file exists and the temporary code no longer appears in the diff.
  - Left unchecked (live-app-only): requires running the desktop app with a temporary panic.
- [ ] 3.3 Confirm non-interference: run a build from a console and confirm the standard panic output is still printed on stderr in addition to the file entry (previous hook preserved), and confirm the recording path makes no network call (no analytics/telemetry code added in `panic_log.rs`); verify by inspecting console output and the module's imports.
  - Partially verified (2026-10-01): `panic_log.rs` imports only `std` (+ `chrono`, `dirs` by path) — no network/analytics code; `cargo test ... installed_hook -- --nocapture` shows the default hook's `panicked at ...` stderr line still printed. Left unchecked (live-app-only): console run of an actual app build.
- [x] 3.4 Run `cargo clippy -p meetily --all-targets` and `openspec validate add-panic-hook-logging --strict`; verify both complete without new errors.
  - Verified 2026-10-01: clippy summary identical to pre-change (lib 23, db_inspect 3, lib test 27 incl. 20 duplicates), no warnings in `panic_log.rs`; `openspec validate --strict` valid. Lib tests 515 passed / 0 failed / 9 ignored (509 baseline + 6 new).
