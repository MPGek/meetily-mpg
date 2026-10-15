# Proposal

## Why

`frontend/src-tauri/src` carries 3,764 lines across four dead files (`lib_old_complex.rs`, `audio/core-old.rs`, `audio/recording_saver_old.rs`, `audio_v2/`) that nothing references, three stale planning `.md` files left over from a prior cleanup pass, an MSRV declaration that already understates the compiler features the code uses, and two git dependencies pinned to a moving branch instead of a commit. None of this changes behavior; all of it costs reviewers and new contributors time and, for the git deps, risks an unannounced breaking change on the next `cargo update`. Clean it up before the larger refactors (changes 04-10) land on top of it.

## What Changes

- Delete `frontend/src-tauri/src/lib_old_complex.rs`, `frontend/src-tauri/src/audio/core-old.rs`, `frontend/src-tauri/src/audio/recording_saver_old.rs`, and the `frontend/src-tauri/src/audio_v2/` directory (9 files) — confirmed zero `mod` declarations and zero references anywhere in `frontend/src-tauri/src`, `build.rs`, or any `Cargo.toml`.
- Delete `frontend/src-tauri/CLEANUP_PLAN.md`, `LOGGING_OPTIMIZATIONS.md`, and `NOTIFICATION_TESTING.md` — the first is superseded by this change (it proposed the exact deletions above), the second documents logging work already merged (past tense, all items checked off), the third is a manual macOS QA script for notification commands that still exist but belongs in a test/QA doc, not shipped source.
- Bump `rust-version` from `"1.77"` to `"1.80"` in **both** `Cargo.toml` (`[workspace.package]`) and `frontend/src-tauri/Cargo.toml` (`[package]`) — the latter is a literal string, not `rust-version.workspace = true`, so the workspace bump alone would not fix the MSRV lint.
- Pin `ffmpeg-sidecar` in `frontend/src-tauri/Cargo.toml` to the exact commit already resolved in `Cargo.lock`, dropping `branch = "main"`.
- Remove the non-functional `[patch.crates-io]` block from `frontend/src-tauri/Cargo.toml` instead of pinning it — see `design.md` D2 for why pinning it is not the right fix.
- `git rm --cached baseline-oldcore-timing.txt` and add it to `.gitignore`; nothing reads it programmatically (one `eval/reports/*.md` prose mention only).
- **BREAKING** (build-time only, not runtime): raises the effective minimum Rust compiler version for building `meetily` from 1.77 to 1.80. No source or behavior change.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
<!-- None: this is a pure hygiene/dependency change with no spec-level behavior change. skip_specs: true. -->

## Impact

- Code removed: `frontend/src-tauri/src/lib_old_complex.rs` (2,437 lines), `frontend/src-tauri/src/audio/core-old.rs` (923), `frontend/src-tauri/src/audio/recording_saver_old.rs` (404), `frontend/src-tauri/src/audio_v2/` (9 files, 1,389 lines) — 3,764 lines total, none reachable from `lib.rs`'s module tree.
- Docs removed: `frontend/src-tauri/CLEANUP_PLAN.md`, `LOGGING_OPTIMIZATIONS.md`, `NOTIFICATION_TESTING.md`.
- Config: `Cargo.toml` (`rust-version`, `[patch.crates-io]` removed), `frontend/src-tauri/Cargo.toml` (`rust-version`, `ffmpeg-sidecar` dependency), `.gitignore`.
- Repo history: `baseline-oldcore-timing.txt` untracked (kept on disk, no longer versioned).
- Out of scope, left for change 11 (docs-refresh): `docs/CODEBASE_MAP_MODULE_AUDIO.md`, `docs/CODEBASE_MAP_ARCHITECTURE.md`, and `docs/CODEBASE_MAP_NAVIGATION.md` all describe `audio_v2/`, `core-old.rs`, and `recording_saver_old.rs` as dead/orphaned; once this change deletes those files, the docs will reference paths that no longer exist. `openspec/changes/archive/*` historical changes that mention `audio_v2` are archived records of past work and are intentionally not edited.
- Out of scope: the `cpal` patch (`frontend/src-tauri/Cargo.toml:229`) and everything else in `[patch.crates-io]` beyond `esaxx-rs` — see design.md D2; both entries are removed together because the whole block is inert, not because `cpal` was separately audited.
