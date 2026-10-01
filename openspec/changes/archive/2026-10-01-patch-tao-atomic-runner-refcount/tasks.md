# Tasks

## 1. Vendor and patch tao

- [x] 1.1 Copy the registry crate `~/.cargo/registry/src/index.crates.io-*/tao-0.35.2/` verbatim into `vendor/tao/` (Cargo.toml, Cargo.toml.orig, src/, examples/, LICENSE*, README.md). Verify `diff -r` against the registry copy shows no differences.
- [x] 1.2 In `vendor/tao/src/platform_impl/windows/event_loop/runner.rs`, change `EventLoopRunnerShared<T>` to `Arc<EventLoopRunner<T>>` and fix the `std` imports. In `vendor/tao/src/platform_impl/windows/event_loop.rs:201`, change `Rc::new` to `Arc::new` and drop the `rc::Rc` import if it becomes unused. Mark each edited site with `// meetily-patch:`. Verify that `diff -r` against the registry copy now shows only these edits, and that `grep -rn "Rc<EventLoopRunner\|Rc::new(EventLoopRunner" vendor/tao/src` returns nothing.
- [x] 1.3 Write `vendor/tao/MEETILY_PATCH.md`: upstream version 0.35.2, the exact diff, the root cause with links (tauri-apps/tauri#15408, tauri-apps/tauri#15411, tauri-apps/tao#1334), the clippy `arc_with_non_send_sync` note, and the removal and rollback steps from design.md. Verify the file names every `// meetily-patch:` site found by grep.

## 2. Wire the patch into the workspace

- [x] 2.1 Add `[patch.crates-io]` with `tao = { path = "vendor/tao" }` to the root `Cargo.toml`, then run `cargo update -p tao`. Verify that `cargo tree -i tao` shows `tao v0.35.2 (…\vendor\tao)` and that cargo prints no "patch … was not used" warning.
- [x] 2.2 Confirm `Cargo.lock` changed only the `tao` entry, which loses its `source`/`checksum` lines. Verify with `git diff Cargo.lock` that no other package moved.
- [x] 2.3 Add `frontend/src-tauri/tests/tao_patch_guard.rs`. It reads the workspace lockfile via `include_str!("../../../Cargo.lock")` and asserts the `name = "tao"` package block has no `source = ` line. On failure the message must mention `vendor/tao/MEETILY_PATCH.md`. Verify `cargo test -p meetily --test tao_patch_guard` passes. Then temporarily add a `source = "registry+…"` line to a copy of the lockfile fed to the same parser and confirm the check fails. Do this with a helper in the test that takes the lock text as an argument, so the negative case is a second `#[test]` and the real `Cargo.lock` is never edited.

## 3. Build and binary verification

- [x] 3.1 Build the release app with the project's normal release command (see docs/CODEBASE_MAP_OPERATIONS.md). Verify the build succeeds and `target/release/meetily.exe` is fresh.
- [x] 3.2 Disassemble `<tauri_runtime_wry::Context<tauri::EventLoopMessage> as core::clone::Clone>::clone` in the new binary. Locate it with `llvm-symbolizer`/`llvm-objdump` against `target/release/meetily.pdb`. Verify that every `incq` in it has a `lock` prefix: the old bare `incq (%r14); je` on field `0x30` must be gone. Record the before/after snippet in `vendor/tao/MEETILY_PATCH.md`.
- [x] 3.3 Run `graphify update .` and verify it completes without errors.

## 4. Runtime verification (user)

- [x] 4.1 With the new release build, record for at least 45 minutes on Windows with the window open but unused. Verify the process stays up, the recording stops and saves normally, and no new `meetily.exe.*.dmp` appears in `%LOCALAPPDATA%\CrashDumps`.
