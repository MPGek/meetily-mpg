# Proposal

## Why

`cargo clippy -p meetily --all-targets` reports 223 warnings and `next lint` reports 268 errors / 37 warnings (both counts reproduced 2026-09-18). Neither is enforced in CI, so both only grow. Most are mechanical (unused imports, redundant references, auto-derivable impls, unescaped JSX entities) and can be fixed with zero behavior change; clearing them now, before changes 04-10 touch this same code, avoids compounding the diff noise in every later PR's review.

## What Changes

- Run `cargo clippy --fix --allow-dirty -p meetily --all-targets` and commit the mechanical subset it can safely apply (redundant references, redundant closures, unneeded `return`, `&PathBuf`→`&Path`, useless conversions/casts, manual `RangeInclusive::contains`, `div_ceil`, clamp-pattern, `Iterator::last`→`next_back`, `.get(0)`→`.first()`, module-inception renames excluded — see design.md).
- Manually apply `#[allow(clippy::too_many_arguments)]` with a one-line justification comment at the 11 flagged function definitions, rather than restructuring their signatures (explicitly out of scope — `summary/processor.rs`'s 20-argument function is changes 04-10 territory, not this one).
- Add `#[allow(clippy::module_inception)]` at the 10 flagged `mod` declarations rather than renaming inner modules (avoids call-site churn across the crate for a purely stylistic lint).
- Delete the developer-hardcoded-path test `frontend/src-tauri/tests/db_inspect.rs`'s absolute path; gate the whole test behind an environment variable (`MEETILY_DB_INSPECT_PATH`) with `#[ignore]`, so it stays available for manual DB inspection without running in `cargo test`/CI on any other machine.
- Fix the 2 test-binary warnings: unused import in `tests/repro_full_stop.rs:6`, unused variable in `tests/repro_online_diarization.rs:260`.
- Frontend: run `next lint --fix` for the auto-fixable subset of `react/no-unescaped-entities` (64) and `@typescript-eslint/no-unused-vars` (111) where safe (import-only removals; unused function params/locals with no side effects).
- Frontend: of the 37 `react-hooks/exhaustive-deps` sites, fix the 9 that are trivially missing (or trivially carrying) a referentially-stable dependency (a `useState` setter, or a primitive field read); suppress the remaining 28 with `// eslint-disable-next-line react-hooks/exhaustive-deps` plus a one-line reason, since each of those involves a non-memoized function/object whose inclusion risks changing render/effect timing.
- Define numeric exit criteria for both linters (below) so a follow-up change can safely flip CI enforcement on; this change does not itself add a blocking `-D warnings`/lint-error CI gate — see design.md's Decisions and Open Questions for why that is deferred.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
<!-- None: purely mechanical, behavior-preserving lint fixes and `#[allow]` annotations; no spec-level behavior changes. skip_specs: true. -->

## Impact

- Code: broad but shallow diff across `frontend/src-tauri/src/**/*.rs` (clippy --fix output) and `frontend/src/**/*.{ts,tsx}` (next lint --fix output + manual exhaustive-deps triage); `frontend/src-tauri/tests/db_inspect.rs`, `tests/repro_full_stop.rs`, `tests/repro_online_diarization.rs`.
- Exit criteria (measured the same way as this proposal's baseline): `cargo clippy -p meetily --all-targets` warning count ≤ 40 (down from 223); `next lint` error count = 0 (down from 268), warning count ≤ 28 (down from 37, after the 9 trivial exhaustive-deps fixes).
- Out of scope: `@typescript-eslint/no-explicit-any` (91 errors) — explicitly change 09. `too-many-arguments`/`type_complexity` structural fixes beyond `#[allow]` — changes 04-10 (recording-lock hardening, diarization engine split, speaker-repository split all touch the flagged functions directly). Enabling `-D warnings` in CI — left as an explicit follow-up (Open Questions in design.md), not this change's job.
