# Meetily patch for tao 0.35.2

This directory is an unmodified copy of the crates.io package `tao` **0.35.2**, as unpacked by
cargo into `~/.cargo/registry/src/index.crates.io-*/tao-0.35.2/`, with the registry marker
`.cargo-ok` dropped. The only change is the one described below. The root `Cargo.toml` selects
it via:

```toml
[patch.crates-io]
tao = { path = "vendor/tao" }
```

The workspace version stays `0.35.2`, so `tauri-runtime-wry`'s `tao` requirement is satisfied by
this copy. OpenSpec change: `openspec/changes/patch-tao-atomic-runner-refcount/`
(archived under `openspec/changes/archive/` once done).

## Why

On Windows, `EventLoopWindowTarget` holds `runner_shared: EventLoopRunnerShared<T>`, which
upstream defines as `Rc<EventLoopRunner<T>>`. `tauri-runtime-wry`'s `Context` embeds that target
(through `DispatcherMainThreadContext`, marked `unsafe impl Send + Sync`). Every clone or drop of an
`AppHandle`, `Window` or `Webview` therefore increments or decrements this **non-atomic** count.
Tauri's async-command wrapper does this on tokio worker threads before any app code runs.
Concurrent updates from background threads and the main event loop lose updates. The count
drifts until it wraps, and the next clone aborts with a bare `ud2`. That shows up as
`STATUS_ILLEGAL_INSTRUCTION (0xC000001D)` in `<tauri_runtime_wry::Context as Clone>::clone`.
Alternatively the runner can be freed while it is still in use.

Observed in Meetily 0.7.0 release builds: two crashes on 2026-10-01, each about 20–25 minutes into
an unattended recording. In the full dump, the `Rc` had strong = 0 right after the aborting
increment (so `u64::MAX` before it) and weak = 0.

Upstream tracking:

- tauri-apps/tauri#15408 — "WebView2 custom protocol can clone DispatcherMainThreadContext off the
  main thread" (open; affects tauri 2.10.3–2.11.2, tauri-runtime-wry 2.10.1–2.11.2).
- tauri-apps/tauri#15411 — fix that keeps `window_target` main-thread-only (an `Arc` in `Wry`, a
  `Weak` in `Context`); unmerged at the time of patching, planned for tauri-runtime-wry 2.12.0.
- tauri-apps/tao#1334 — the same `Rc` → `Arc` change applied here; closed in favour of #15411.

## The change

`EventLoopRunnerShared<T>` becomes `Arc<EventLoopRunner<T>>`. Only the reference count becomes
atomic. `EventLoopRunner` still contains `Cell`/`RefCell` and is still only *used* on the main
thread, exactly as before. No new `unsafe` is introduced: thread-crossing was already asserted by
tauri's `unsafe impl Send/Sync for DispatcherMainThreadContext`.

Edited sites (each carries a `// meetily-patch:` comment, except the deleted import):

| File | Line | Edit |
|------|------|------|
| `src/platform_impl/windows/event_loop/runner.rs` | 10 | import `rc::Rc` → `sync::Arc` |
| `src/platform_impl/windows/event_loop/runner.rs` | 27–29 | `type EventLoopRunnerShared<T> = Arc<EventLoopRunner<T>>` |
| `src/platform_impl/windows/event_loop.rs` | 18 (deleted) | removed the now-unused `rc::Rc` import (`sync::Arc` was already imported) |
| `src/platform_impl/windows/event_loop.rs` | 200–201 | `Rc::new(EventLoopRunner::new(..))` → `Arc::new(..)` |

```diff
--- a/src/platform_impl/windows/event_loop/runner.rs
+++ b/src/platform_impl/windows/event_loop/runner.rs
@@ -7,7 +7,7 @@
   cell::{Cell, RefCell},
   collections::{HashSet, VecDeque},
   mem, panic,
-  rc::Rc,
+  sync::Arc, // meetily-patch: was `rc::Rc`; see vendor/tao/MEETILY_PATCH.md
   time::Instant,
 };
 
@@ -24,7 +24,9 @@
   window::WindowId,
 };
 
-pub(crate) type EventLoopRunnerShared<T> = Rc<EventLoopRunner<T>>;
+// meetily-patch: atomic refcount — this handle is cloned/dropped off the main thread
+// through tauri-runtime-wry `Context::clone`; see vendor/tao/MEETILY_PATCH.md.
+pub(crate) type EventLoopRunnerShared<T> = Arc<EventLoopRunner<T>>;
 pub(crate) struct EventLoopRunner<T: 'static> {
   // The event loop's win32 handles
   thread_msg_target: HWND,
--- a/src/platform_impl/windows/event_loop.rs
+++ b/src/platform_impl/windows/event_loop.rs
@@ -15,7 +15,6 @@
   ffi::c_void,
   marker::PhantomData,
   mem, panic,
-  rc::Rc,
   sync::Arc,
   thread,
   time::{Duration, Instant},
@@ -198,7 +197,8 @@
     thread::spawn(move || wait_thread(thread_id, HWND(send_thread_msg_target as _)));
     let wait_thread_id = get_wait_thread_id();
 
-    let runner_shared = Rc::new(EventLoopRunner::new(thread_msg_target, wait_thread_id));
+    // meetily-patch: was `Rc::new`; see vendor/tao/MEETILY_PATCH.md
+    let runner_shared = Arc::new(EventLoopRunner::new(thread_msg_target, wait_thread_id));
 
     let thread_msg_sender = subclass_event_target_window(thread_msg_target, runner_shared.clone());
```

**Clippy note:** `Arc<EventLoopRunner<T>>` wraps a `!Send + !Sync` type, which
`clippy::arc_with_non_send_sync` would flag. Cargo caps lints for dependencies, so it does not
surface in the Meetily build. It is intentional: atomicity of the count is the whole point.

## Guard

`frontend/src-tauri/tests/tao_patch_guard.rs` fails if `Cargo.lock` resolves `tao` from a
registry instead of this path. A path package has no `source =` line. Cargo only *warns* about
an unused `[patch]`, for example after `cargo update` moves `tao` past 0.35.2, so this test is
what catches it:

```bash
cargo test -p meetily --test tao_patch_guard
```

## Binary verification

Find `<tauri_runtime_wry::Context<tauri::EventLoopMessage> as Clone>::clone` in a release build:
look it up with `llvm-pdbutil dump --publics target/release/meetily.pdb`, then disassemble it with
`llvm-objdump -d`. The fix is effective when **every** `incq` in that function has a `lock`
prefix.

Before (0.7.0 build of 2026-10-01 13:37, the build that crashed). The bare increment is the `Rc`;
its overflow check jumps straight to `ud2`:

```
140525370 <Context::clone>
  1405253f7:  movq   0x30(%rdx), %r14
  1405253fb:  incq   (%r14)              ; Rc::clone — not atomic
  1405253fe:  je     0x1405254f1         ; → ud2 (STATUS_ILLEGAL_INSTRUCTION)
```

After (patched build of 2026-10-01 18:01). There are 13 `incq` in the function, and all 13 are
`lock`-prefixed. The `runner_shared` clone symbolizes to `vendor/tao/src/platform_impl/windows/event_loop.rs:172`
via `alloc::sync::Arc::clone`:

```
1402729c0 <Context::clone>
  140272a4b:  lock
  140272a4c:  incq   (%r14)              ; Arc::clone — atomic
```

## Re-applying on a tao upgrade

1. Replace this directory with the new registry copy of tao (drop `.cargo-ok`).
2. Re-apply the diff above, provided upstream still defines `EventLoopRunnerShared` as an `Rc`.
3. Run `cargo update -p tao`, then the guard test and the binary check above.

## Removal

Remove the patch when the tauri-runtime-wry version in use no longer clones a non-atomic `Rc` in
`Context::clone`, i.e. it includes tauri-apps/tauri#15411, or tao itself makes the runner
refcount atomic. Steps:

1. Delete the `[patch.crates-io]` `tao` entry from the root `Cargo.toml` and delete `vendor/tao/`.
2. Run `cargo update -p tao` (or `cargo update -p tao --precise 0.35.2` for a pure rollback).
3. Delete `frontend/src-tauri/tests/tao_patch_guard.rs`.
4. Disassemble `Context::clone` in a release build and confirm it has no bare (non-`lock`)
   `incq`.
