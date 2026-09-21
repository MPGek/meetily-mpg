# Tasks

## 1. Delete dead code

- [ ] 1.1 Delete `frontend/src-tauri/src/lib_old_complex.rs`; verify with `git grep -n "lib_old_complex" -- frontend/src-tauri build.rs` returning nothing and `cargo check -p meetily`.
- [ ] 1.2 Delete `frontend/src-tauri/src/audio/core-old.rs`; verify with `git grep -n "core-old\|core_old" -- frontend/src-tauri` returning nothing (outside `docs/`) and `cargo check -p meetily`.
- [ ] 1.3 Delete `frontend/src-tauri/src/audio/recording_saver_old.rs`; verify with `git grep -n "recording_saver_old" -- frontend/src-tauri` returning nothing (outside `docs/` and `openspec/changes/archive/`) and `cargo check -p meetily`.
- [ ] 1.4 Delete the `frontend/src-tauri/src/audio_v2/` directory (9 files: `compatibility.rs`, `lib.rs`, `limiter.rs`, `mixer.rs`, `normalizer.rs`, `recorder.rs`, `resampler.rs`, `stream.rs`, `sync.rs`); verify with `git grep -n "audio_v2" -- frontend/src-tauri` returning nothing (outside `docs/` and `openspec/changes/archive/`) and `cargo check -p meetily`.
- [ ] 1.5 Run `cargo clippy -p meetily --all-targets --message-format=short` and confirm no new warnings appear (the total should drop, not rise) compared to the pre-change baseline; verify by comparing warning count before/after.

## 2. Delete stale planning docs

- [ ] 2.1 Delete `frontend/src-tauri/CLEANUP_PLAN.md` (superseded: it proposed exactly the deletions in task group 1), `frontend/src-tauri/LOGGING_OPTIMIZATIONS.md` (documents already-merged, checked-off logging work), and `frontend/src-tauri/NOTIFICATION_TESTING.md` (manual macOS QA script, not shipped source); verify with `git status` showing all three removed and no remaining reference via `git grep -rn "CLEANUP_PLAN\|LOGGING_OPTIMIZATIONS\|NOTIFICATION_TESTING" -- frontend README.md docs` (a hit would mean something still links to them; none is expected).

## 3. Fix MSRV declaration

- [ ] 3.1 Change `rust-version = "1.77"` to `rust-version = "1.80"` in the workspace root `Cargo.toml` (`[workspace.package]`); verify with `cargo check -p meetily`.
- [ ] 3.2 Change `rust-version = "1.77"` to `rust-version = "1.80"` in `frontend/src-tauri/Cargo.toml` (`[package]`, currently a literal, not workspace-inherited); verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "MSRV"` returning `0` (currently 3).

## 4. Pin and remove dead dependency configuration

- [ ] 4.1 In `frontend/src-tauri/Cargo.toml`, change the `ffmpeg-sidecar` dependency from `{ git = "https://github.com/nathanbabcock/ffmpeg-sidecar", branch = "main" }` to `{ git = "https://github.com/nathanbabcock/ffmpeg-sidecar", rev = "b0f48a514235d1278d860f4a0af66aa9d6e1618d" }`; verify with `cargo check -p meetily` and `git diff Cargo.lock` showing **no change** (the rev is already what's locked).
- [ ] 4.2 Delete the `[patch.crates-io]` block (`cpal` and `esaxx-rs` entries) from `frontend/src-tauri/Cargo.toml` — see `design.md` D2 for why pinning `esaxx-rs` is not possible (its fork 404s) and why moving/activating the block is out of scope; verify with `cargo check -p meetily` printing no "patch for the non root package will be ignored" warning and `git diff Cargo.lock` showing **no change** (both crates already resolved from crates.io, unaffected by the patch either way).

## 5. Untrack the benchmark artifact

- [ ] 5.1 Run `git rm --cached baseline-oldcore-timing.txt` and add `baseline-oldcore-timing.txt` to `.gitignore`; verify with `git status` showing the file as untracked-but-present on disk, and `git grep -n "baseline-oldcore-timing"` still finding only the prose mention in `eval/reports/spike-2026-09-07-v2.md` (report text is unaffected).

## 6. Final verification

- [ ] 6.1 Run `cargo check -p meetily` and `cargo clippy -p meetily --all-targets --message-format=short`; verify both complete with 0 errors and the warning count strictly ≤ the pre-change baseline (223).
- [ ] 6.2 Run `git grep -rn "lib_old_complex\|core-old\|core_old\|recording_saver_old\|audio_v2" -- frontend/src-tauri build.rs '*.toml'` and confirm it returns nothing; run `openspec validate 01-remove-dead-code-and-pin-toolchain --strict` and confirm it passes.
