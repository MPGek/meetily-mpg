# Design

## Context

See `proposal.md` for motivation. Ground truth verified 2026-09-18 on `feat/diarization`:

- `frontend/src-tauri/src/lib_old_complex.rs`, `src/audio/core-old.rs`, `src/audio/recording_saver_old.rs`, and `src/audio_v2/*` (9 files) have zero `mod` declarations in `src/lib.rs` or `src/audio/mod.rs`, and `git grep` for each filename/module name across `frontend/src-tauri`, `build.rs`, and every `Cargo.toml` returns nothing outside the dead files themselves and the docs called out in `proposal.md`.
- `Cargo.toml` (workspace root) declares `[workspace.package] rust-version = "1.77"`. `frontend/src-tauri/Cargo.toml` separately declares `rust-version = "1.77"` as a literal string in `[package]` (not `rust-version.workspace = true`), so it does not inherit a workspace-level bump.
- `frontend/src-tauri/src/lib.rs:68-69` uses `std::sync::LazyLock`, stable since Rust 1.80.0. `cargo clippy` already emits three `unknown_lints`-adjacent MSRV warnings pointing at exactly these two lines: `current MSRV (Minimum Supported Rust Version) is 1.77.0 but this item is stable since 1.80.0` (twice) and `...stable in a const context since 1.80.0` (once). No `rust-toolchain.toml`/`rust-toolchain` file exists, and all five `.github/workflows/build*.yml` use `dtolnay/rust-toolchain@stable`, i.e. CI always builds with whatever the latest stable toolchain is (currently far newer than 1.80), so raising the declared MSRV cannot break CI.
- `frontend/src-tauri/Cargo.toml:160` declares `ffmpeg-sidecar = { git = "https://github.com/nathanbabcock/ffmpeg-sidecar", branch = "main" }`. `Cargo.lock` resolves it to `source = "git+https://github.com/nathanbabcock/ffmpeg-sidecar?branch=main#b0f48a514235d1278d860f4a0af66aa9d6e1618d"` — i.e. commit `b0f48a514235d1278d860f4a0af66aa9d6e1618d` is what the project actually builds against today. `git ls-remote` shows `main` has since moved to `f56a5e127b938c4fc41700101cd485b7bfd7e579`, confirming the branch is not stable.
- `frontend/src-tauri/Cargo.toml:228-230` declares `[patch.crates-io] cpal = { git = ..., rev = "51c3b43" }` and `esaxx-rs = { git = "https://github.com/thewh1teagle/esaxx-rs.git", branch = "feat/dynamic-msvc-link" }`. **This patch block has no effect.** `cargo check -p meetily` prints: `warning: patch for the non root package will be ignored, specify patch at the workspace root: package: .../frontend/src-tauri/Cargo.toml workspace: .../Cargo.toml`. Cargo only honors `[patch]` at the workspace root manifest; a member crate's `[patch]` table is silently dropped. `Cargo.lock` confirms it: both `cpal` (`source = "registry+...crates.io-index"`, not git) and `esaxx-rs` (same) resolve from crates.io, not from either git fork. Additionally, `https://github.com/thewh1teagle/esaxx-rs` returns `404 Not Found` (verified via `git ls-remote` and the GitHub API) — the fork the patch points to no longer exists.
- `baseline-oldcore-timing.txt` (repo root, tracked, last touched by commit `89fb4db`) is referenced only in prose by `eval/reports/spike-2026-09-07-v2.md` ("Old core (sparse AHC, `baseline-oldcore-timing.txt`): 54.46 s total."); nothing in `scripts/`, `eval/`, or any Rust/TS source reads the file at runtime or in a test.

## Goals / Non-Goals

**Goals:**
- Remove dead code and stale docs with zero behavior change, verified by `cargo check`/`cargo clippy` passing and `git grep` returning nothing for the deleted names.
- Make the declared MSRV match what the code already requires, so the MSRV lint stops firing and a future contributor on a pinned 1.77-1.79 toolchain gets a clear compile error instead of a confusing one.
- Make dependency resolution reproducible: no unpinned git branch, no dead `[patch]` block masquerading as a pin.
- Stop tracking a benchmark-run artifact that changes on every profiling run.

**Non-Goals:**
- Fixing the underlying MSVC dynamic-link issue that `esaxx-rs`'s `feat/dynamic-msvc-link` fork was presumably meant to address, or finding a replacement fork/patch. The patch has been inert (see Context) for an unknown period before this audit, so removing it changes nothing observable; sourcing a working replacement is separate follow-up work, not hygiene.
- Any change to `cpal`'s patch semantics beyond removing the same dead block it sits in.
- Splitting or refactoring any file that *is* referenced (`audio/recording_commands.rs`, `database/repositories/speaker.rs`, etc. are changes 06-07).
- Touching `docs/CODEBASE_MAP_*.md` (change 11).
- Re-enabling hardware-acceleration feature flags or any dependency version bump beyond the two git-ref pins named above.

## Decisions

### D1: Delete outright rather than `#[allow(dead_code)]` or archive
The four dead files/dirs are deleted, not gated behind a feature flag or moved to an `archive/` folder.
- They are already fully unreachable (D0 verification above), so there is nothing to preserve for a future migration; `audio_v2`'s own module doc and `CLEANUP_PLAN.md` both independently concluded this before this change.
- Git history keeps the content retrievable (`git log --diff-filter=D -- frontend/src-tauri/src/lib_old_complex.rs` etc. after this change merges) if anyone needs to consult it later.
- Alternative considered: keep `audio_v2/` behind `#[cfg(feature = "audio_v2_experimental")]`. Rejected — no feature flag exists today, and the module is 20+ TODOs deep per its own `CLEANUP_PLAN.md` assessment; gating it would keep it compiling (and bit-rotting) for no consumer.

### D2: Delete the dead `[patch.crates-io]` block instead of pinning `esaxx-rs` to a rev
The brief's original plan was to pin the `esaxx-rs` patch to a concrete `rev`. That is not possible as a behavior-preserving change:
1. The patch is already inert for both `cpal` and `esaxx-rs` (Context, above) — `Cargo.lock` proves neither dependency actually resolves through it today.
2. `esaxx-rs`'s patch source (`thewh1teagle/esaxx-rs`) is a 404 — there is no rev to pin to.
3. Moving the block to the workspace root (the "real" fix for the "patch ignored" warning) would be a **behavior change**, not hygiene: it would newly activate two forks (`cpal` and `esaxx-rs`) that are not exercised by the current build, on a change whose only goal is dead-code removal.

So: delete `frontend/src-tauri/Cargo.toml:227-230` (the `[patch.crates-io]` header and both entries) outright. `Cargo.lock` is unaffected (`cargo check` after the edit resolves both crates from crates.io exactly as before, because that is what was already happening). This removes dead configuration and the warning it causes, with a verified-zero behavior change.
- Alternative considered: move the patch block to the workspace root `Cargo.toml` and pin `cpal` (still resolvable) while dropping `esaxx-rs` (dead fork). Rejected for this change — it would make the `cpal` fork newly effective, which is a real dependency-resolution change that deserves its own review and testing (does the built binary still link/run correctly against the patched `cpal`?), not a rider on a hygiene change. Flagged as a follow-up (see Open Questions).
- Alternative considered: leave the block in place and just add a code comment noting it's inert. Rejected — dead configuration that looks live is worse than no configuration; the next person to touch it should not have to re-discover the "ignored: non-root patch" warning from scratch.

### D3: Pin `ffmpeg-sidecar` to the rev already in `Cargo.lock`, not to the branch's current tip
Change `frontend/src-tauri/Cargo.toml:160` to:
```toml
ffmpeg-sidecar = { git = "https://github.com/nathanbabcock/ffmpeg-sidecar", rev = "b0f48a514235d1278d860f4a0af66aa9d6e1618d" }
```
- `b0f48a514235d1278d860f4a0af66aa9d6e1618d` is what `Cargo.lock` already resolves `branch = "main"` to; pinning to it is a no-op for the build today and a verified-zero-diff change.
- The branch's tip has already moved past this commit (`f56a5e12...`), so pinning to "whatever `main` is now" would silently pull in unreviewed upstream changes as part of a hygiene PR — exactly what pinning is meant to prevent.
- Bumping to the newer tip, if wanted, is a separate, deliberate dependency-update change with its own testing, not part of this one.

### D4: Bump `rust-version` in both manifests, together
Both `Cargo.toml` (`[workspace.package]`) and `frontend/src-tauri/Cargo.toml` (`[package]`) go from `"1.77"` to `"1.80"` in the same commit.
- `frontend/src-tauri/Cargo.toml`'s `rust-version` is a literal, not `rust-version.workspace = true` (verified by reading the file), so bumping only the workspace value would leave the MSRV lint firing on `frontend/src-tauri` — the crate that actually contains the 1.80-only code.
- `1.80` (not something higher) because that is the exact version `LazyLock` needs per the clippy MSRV diagnostic; there is no other construct in the codebase currently requiring anything newer (no other MSRV warnings appear in the full `cargo clippy --all-targets` run captured for change 03).
- Not switching to `rust-version.workspace = true` for `frontend/src-tauri/Cargo.toml` in this change — that would be a reasonable follow-up (Open Questions) but is a structural manifest change beyond "bump the number", and this change's job is only to make the declared MSRV correct.

### D5: Untrack `baseline-oldcore-timing.txt` via `git rm --cached` + `.gitignore`, don't delete the working file
- `git rm --cached baseline-oldcore-timing.txt` removes it from version control while leaving the file on disk (so any local workflow that regenerates/reads it locally is undisturbed), then adding `baseline-oldcore-timing.txt` to `.gitignore` stops it from being re-added by accident.
- It is a benchmark-run artifact (a timing number from one profiling session), not a fixture: nothing in `scripts/`, `eval/`, or test code parses it, and its one consumer is a paragraph of prose in an already-written report (`eval/reports/spike-2026-09-07-v2.md`), which is unaffected by the file leaving version control.
- Not deleting the report's mention of the file — the report is a historical record of a specific benchmark run and stays accurate whether or not the raw file is tracked.

## Risks / Trade-offs

- **Raising `rust-version` to 1.80 could break a contributor pinned to an older toolchain** → Mitigated: no `rust-toolchain.toml` exists and every CI workflow uses `dtolnay/rust-toolchain@stable` (verified), so no automated build is pinned below 1.80; a locally-pinned contributor would get a clear `rust-version` mismatch error rather than the current confusing situation (declared MSRV already violated by `LazyLock`).
- **Deleting the `[patch.crates-io]` block changes what `cargo check`/`cargo build` reports as resolved, even though the resolution itself doesn't change** → Mitigated: `Cargo.lock`'s `esaxx-rs`/`cpal` entries are byte-for-byte unaffected (both already resolve from crates.io); only the manifest's dead intent and the "patch ignored" warning disappear. Verify with `git diff Cargo.lock` showing no change after `cargo check`.
- **Someone was relying on the MSVC-dynamic-link fix the dead `esaxx-rs` patch implied** → Unlikely: the patch has been non-functional (Context) since before this audit, meaning the currently-shipping build already uses the unpatched crates.io `esaxx-rs`. If a real MSVC linking issue exists, it exists today too, independent of this change.
- **Deleting `audio_v2/`, `lib_old_complex.rs`, `core-old.rs`, `recording_saver_old.rs` leaves three docs describing files that no longer exist** → Accepted; explicitly deferred to change 11 (docs-refresh), named in `proposal.md`'s Impact section so it isn't lost.

## Migration Plan
- Single commit/PR, no data migration. Order within the change: delete dead code/docs first (independent of the manifest edits), then the `Cargo.toml`/`frontend/src-tauri/Cargo.toml` edits, then `git rm --cached` the baseline file, verifying `cargo check -p meetily` and `cargo clippy -p meetily --all-targets` after each group.
- Rollback: revert the commit. Nothing downstream depends on the deleted files or the removed patch block (verified above), so rollback is unconditionally safe.

## Open Questions
- Should the `cpal` fork (`rev = "51c3b43"`, still resolvable) be moved to the workspace-root `[patch.crates-io]` and actually activated in a follow-up change, now that this change has surfaced that it's currently inert? (Requires its own build/runtime verification on all three platforms — out of scope here.)
- Should `frontend/src-tauri/Cargo.toml` switch `rust-version` (and `edition`) to `.workspace = true` so this class of drift can't recur?
