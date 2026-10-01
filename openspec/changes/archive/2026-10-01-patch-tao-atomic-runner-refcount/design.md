# Design

## Context

See proposal.md (Why) for the crash analysis. Facts from the current tree that shape the approach:

- `Cargo.lock` resolves `tauri` 2.11.1, `tauri-runtime-wry` 2.11.1, `wry` 0.55.1 and `tao` 0.35.2. The workspace root is `Cargo.toml`, with members `frontend/src-tauri` and `llama-helper`. It has no `[patch]` section yet.
- In tao 0.35.2 (`src/platform_impl/windows/`), `event_loop/runner.rs:27` declares `pub(crate) type EventLoopRunnerShared<T> = Rc<EventLoopRunner<T>>;`. `event_loop.rs:201` is its only construction site (`Rc::new(EventLoopRunner::new(..))`). Every other use goes through the alias: the fields at `event_loop.rs:113`, `:127`, `:173` and `:693`, plus `.clone()` calls. No `Rc::downgrade`, `Weak` or `Rc::ptr_eq` is involved.
- `EventLoopRunner` contains `Cell` and `RefCell`, so it is `!Sync`. `Arc<EventLoopRunner>` is therefore `!Send`/`!Sync`, exactly like the `Rc` today. Thread-crossing is already granted by tauri's `unsafe impl Send/Sync for DispatcherMainThreadContext`, so no new `unsafe` is needed.
- In the dumped release binary, `Context::clone` (`target/release/meetily.exe`, RVA `0x525370`) contains exactly one non-`lock` increment: `incq (%r14); je abort`, loading `0x30(%rdx)`. That is the `Rc` this change replaces. It gives us an objective before/after check.

## Goals / Non-Goals

**Goals:**
- Remove the data race on the event-loop runner's reference count with the smallest possible diff to tao.
- Make the patch visible, documented, and impossible to lose silently.

**Non-Goals:**
- Changing tauri, tauri-runtime-wry or wry, or how our code uses handles. For example, we do not marshal `get_webview_window` to the main thread as in the Meetily-ActuallyFree PR #30. That approach only reduces the race, because tauri's command wrapper clones the handles anyway.
- Making `EventLoopRunner` itself thread-safe. Its contents are still touched only on the main thread; we only make the count atomic.
- Patching macOS/Linux backends, which do not use this `Rc`.
- Auditing our own `unsafe impl Send` audio types (separate concern; noted in the proposal).

## Decisions

1. **Vendor the full tao 0.35.2 crate into `vendor/tao/` and use `[patch.crates-io] tao = { path = "vendor/tao" }` in the root `Cargo.toml`.**
   - Copy the crate exactly as cargo unpacked it from the registry: `Cargo.toml` (normalized), `src/`, `LICENSE*` and `README.md`. Do not refetch it from git, so the bytes we diff against are the ones we already build and ship. Keep `examples/` as well: the normalized `Cargo.toml` declares 30 `[[example]]` targets, and the whole crate is only ~1.5 MB.
   - *Alternative: a `[patch]` pointing at a git fork.* Rejected: it needs an external repo plus network access during builds, and hides the diff from code review.
   - *Alternative: wait for tauri#15411 (tauri-runtime-wry 2.12).* Rejected: still unmerged, and the crash hits the core use case now.
   - *Alternative: app-level mitigation (marshal handle lookups to the main thread).* Rejected for the reason above (partial).

2. **The patch is exactly the `Rc` → `Arc` swap: the alias in `runner.rs`, `Rc::new` → `Arc::new` in `event_loop.rs`, plus import adjustments.** Each edited site carries a `// meetily-patch:` comment, so a grep finds them all. Do not change `Cargo.toml`'s version. It must stay `0.35.2` so that `tauri-runtime-wry`'s `tao` requirement is satisfied by the patch.

3. **Document the patch in `vendor/tao/MEETILY_PATCH.md`.** The file covers: the upstream version, the diff, the root cause with links (tauri#15408, tauri#15411, tao#1334), and the removal condition. The removal condition is that tao or tauri-runtime-wry no longer clones a non-atomic `Rc` in `Context::clone`, and the test from Decision 4 is then deleted along with the patch.

4. **Guard test: a Rust integration test, `frontend/src-tauri/tests/tao_patch_guard.rs` (next to the existing `tests/*.rs`), that parses the workspace `Cargo.lock` (via `include_str!("../../../Cargo.lock")`).** It asserts that the `[[package]] name = "tao"` entry has no `source = ` line, which is how a path dependency appears in the lockfile. It runs under the existing `cargo test` for the app crate, and its failure message names `vendor/tao/MEETILY_PATCH.md`.
   - *Alternative: a script in `scripts/`.* Rejected: it would not run as part of the normal test command the spec requires.
   - *Alternative: a `build.rs` check.* Rejected: it would break every local build during a deliberate upgrade, instead of failing in tests.

5. **Behavioural verification is manual, plus a disassembly check.** After a release build, disassemble `Context::clone` and confirm that every increment carries a `lock` prefix (no bare `incq` on the `0x30` field). Then run a recording of 45 minutes or more, unattended, on the user's machine. That is about twice the observed time to crash. A reliable automated reproduction would need tao's event loop running on a test main thread, plus a contention harness. That costs more than it is worth here.

## Risks / Trade-offs

- [`cargo update` moves `tao` to a newer 0.35.x, the `[patch]` stops matching, and cargo only prints a warning] → The guard test (Decision 4) fails. Re-vendor the new version and re-apply the documented two-line diff.
- [Upstream fixes the race differently (tauri#15411 uses an `Arc`/`Weak` split), and later tauri versions start requiring a tao that conflicts with our patch] → Remove the patch per `MEETILY_PATCH.md` when upgrading tauri. The guard test is deleted together with it.
- [`Arc` of a `!Send` type triggers `clippy::arc_with_non_send_sync`] → Lints in dependencies are capped by cargo, so it does not affect our build. Mention it in `MEETILY_PATCH.md`.
- [Other crash sources remain: the 15:17 debug-build access violation in `MMDevApi.dll_unloaded` is a different signature] → Out of scope. If the 45-minute run still crashes, the new full dump (LocalDumps `DumpType=2` is now enabled) will show whether it is a new cause.
- [The vendored copy adds about 1.5 MB to the repo] → Acceptable. The crate is small and permissively licensed (Apache-2.0/MIT), and its licence files are kept.

## Migration Plan

1. Land the vendored crate, the `[patch]` entry, the lockfile update and the guard test in one commit.
2. Rebuild release, run the disassembly check, then run the long recording.
3. Rollback: delete the `[patch]` entry and `vendor/tao/`, then run `cargo update -p tao --precise 0.35.2` and delete the guard test.
