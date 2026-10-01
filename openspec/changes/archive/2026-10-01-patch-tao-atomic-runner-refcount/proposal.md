# Proposal

## Why

The release build (0.7.0) crashes on Windows with `STATUS_ILLEGAL_INSTRUCTION (0xC000001D)` about 20–25 minutes into a recording, even when nobody is using the app. Two crash dumps from 2026-10-01 (16:41 and 17:08) both stop on the same instruction: a `ud2` inside `<tauri_runtime_wry::Context as Clone>::clone`, reached through `AppManager::get_webview` while the main thread handles an IPC message. The full dump shows why. `Context` holds tao's `runner_shared: Rc<EventLoopRunner<T>>`, and that `Rc` had strong = `u64::MAX` and weak = 0. The count is not atomic, and it is cloned and dropped from tokio worker threads: every clone or drop of an `AppHandle`, `Window` or `Webview` touches it, and tauri's async-command wrapper does so before any app code runs. During a recording these operations race with the event loop, increments get lost, and the counter eventually wraps. The next clone then aborts, or the runner is freed while still in use.

This is a known upstream defect ([tauri-apps/tauri#15408](https://github.com/tauri-apps/tauri/issues/15408), affecting tauri 2.10.3–2.11.2). The proposed fix in tao ([tauri-apps/tao#1334](https://github.com/tauri-apps/tao/issues/1334)) was closed in favour of [tauri-apps/tauri#15411](https://github.com/tauri-apps/tauri/pull/15411), which is still unmerged. Our app cannot wait for it: long recordings are the core use case.

## What Changes

- Vendor `tao` 0.35.2 (the version `Cargo.lock` currently resolves) into the repository. On the Windows backend, change `EventLoopRunnerShared<T>` from `Rc<EventLoopRunner<T>>` to `Arc<EventLoopRunner<T>>`. This makes the reference count atomic; the runner itself stays main-thread-only, as before.
- Point the workspace at the vendored copy with `[patch.crates-io]` in the root `Cargo.toml`, so `tauri-runtime-wry` picks it up without any change to tauri.
- Record the patch: what changed, why, and when to remove it (once a tauri-runtime-wry release contains #15411, or tao makes the runner refcount atomic itself).
- Add a check that fails loudly if the patch silently stops applying, for example after `cargo update` moves `tao` past 0.35.2. Cargo only warns about an unused `[patch]`.
- No application code changes. No change to frontend or IPC behaviour.

## Capabilities

### New Capabilities
- `app-handle-thread-safety`: the app stays up while Tauri application, window and webview handles are cloned and dropped from background threads concurrently with the main event loop, including during long recordings.

### Modified Capabilities
<!-- None: no existing spec's requirements change. -->

## Impact

- **Dependencies**: `tao` is resolved from `vendor/tao` instead of crates.io, and `Cargo.lock` changes its source accordingly. This is a new maintenance obligation: any `tao` upgrade must re-apply or drop the patch.
- **Build**: root `Cargo.toml` (`[patch.crates-io]`), new `vendor/tao/` tree (about 1.5 MB, Apache-2.0/MIT, licence files kept).
- **Runtime**: on Windows, each clone or drop of a handle does one atomic increment or decrement instead of a plain one. The cost is negligible. macOS and Linux tao backends are untouched.
- **Code**: none in `frontend/src-tauri/src`. Existing `unsafe impl Send` on audio types (`audio/stream.rs`, `audio/recording_manager.rs`) is a separate concern and out of scope.
