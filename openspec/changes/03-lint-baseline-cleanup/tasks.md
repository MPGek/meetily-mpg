# Tasks

> Re-measured 2026-10-01 on `feat/re-work` after change 01 landed. The 2026-09-18 numbers in proposal/design are stale; the real baselines and exit numbers are recorded per task below.
> Before: clippy **216** warnings; `next lint` **268 errors / 37 warnings** (111 `no-unused-vars`, 91 `no-explicit-any`, 64 `no-unescaped-entities`, 2 `prefer-const`; 37 `exhaustive-deps`); `cargo test -p meetily --lib` 511 passed / 0 failed / 9 ignored; `bun test tests/` 71 passed.
> After: clippy **32** warnings; `next lint` **91 errors / 0 warnings** (only `no-explicit-any`, carried to change 09).

## 1. Baseline counts (record before changing anything)

- [x] 1.1 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | tee clippy-before.txt | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify it prints `223` (or record the actual number if it has drifted, and use that as the baseline instead).
  - Note: drifted to **216** (change 01 removed dead code). Used 216 as the baseline.
- [x] 1.2 Run `cd frontend && npx next lint 2>&1 | tee ../lint-before.txt`; verify the summary line reads `268 problems (268 errors, 37 warnings)` (or record the actual numbers if drifted).
  - Note: still 268 errors / 37 warnings, but the mix differs from design.md: 111 `no-unused-vars` + 91 `no-explicit-any` + 64 `no-unescaped-entities` + **2 `prefer-const`** (not listed in the design table).

## 2. Rust: mechanical auto-fix

- [x] 2.1 Run `cargo clippy --fix --allow-dirty -p meetily --all-targets`; verify with `cargo check -p meetily` (compiles) and `git diff --stat` showing only import/reference/closure/cast-level changes (no logic restructuring).
  - Note: the first run rolled back every fix in `app_lib` because the `manual_saturating_arithmetic` rewrite in `whisper_engine.rs::calculate_repetition_ratio` left an ambiguous integer type (E0689). Fixed by hand (`or_insert(0usize)` + `saturating_sub(1)`, same result for counts >= 1) and re-ran. Hand corrections after `--fix`: `audio/devices/fallback.rs` imports used only by macOS code were re-added behind `#[cfg(target_os = "macos")]` instead of deleted (they only look unused on Windows); leftover blank lines/parens/spacing from rustfix tidied; the orphaned `assign_live_speaker` doc block in `recording_commands.rs` moved onto that command.
- [x] 2.2 Run `cargo test -p meetily --lib` and confirm it passes with the same test count as before task 2.1; verify no test outcome changed.
  - Note: baseline before any change was 511/0/9. Later in the session the machine lost its audio output device: `playback_monitor::tests::test_get_output_device` started failing and the full run crashed with STATUS_ACCESS_VIOLATION in `system_audio_commands` device enumeration. Both happen on unmodified HEAD too (checked with `git stash`), so they come from the environment, not this change. With those 3 device tests skipped (`-- --skip audio::playback_monitor --skip audio::system_audio_commands`): **508 passed / 0 failed / 9 ignored / 3 filtered = 511**. The 2 `system_audio_commands` tests pass on their own with this change applied.
- [x] 2.3 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify the count dropped by roughly 100-130 from the task 1.1 baseline (the categories marked "Yes" in design.md's table).
  - Note: 216 -> 77 (-139), slightly more than expected.

## 3. Rust: manual `#[allow]` annotations

- [x] 3.1 Add `#[allow(clippy::too_many_arguments)]` with a one-line reason comment above each of the 11 flagged functions: `analytics/analytics.rs:424`, `analytics/commands.rs:325`, `api/api.rs:557`, `api/api.rs:1373`, `audio/pipeline.rs:714`, `audio/pipeline.rs:1278`, `audio/online_diarization.rs:130`, `summary/commands.rs:329`, `summary/llm_client.rs:115`, `summary/processor.rs:326`, `summary/service.rs:300`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "too many arguments"` returning `0`.
  - Note: still 11 sites, but the list moved. `audio/online_diarization.rs` no longer exists; its slot went to `audio/diarization/telemetry.rs:183` (`resolve_channel_state`, 8 params). `pipeline.rs:1278` is now `:1304` and `llm_client.rs:115` is now `:113`. Changes 04, 05 and 10 are already archived and did not change these signatures, so no reason comment points at them: Tauri commands say "each param is a separate IPC argument", the rest say "no owning change yet". Verified: 0.
- [x] 3.2 Add `#[allow(clippy::module_inception)]` with a one-line reason comment above the `mod` item in each of the 10 flagged files: `analytics/mod.rs:1`, `anthropic/mod.rs:1`, `api/mod.rs:1`, `console_utils/mod.rs:2`, `groq/mod.rs:1`, `ollama/mod.rs:3`, `openai/mod.rs:1`, `openrouter/mod.rs:2`, `parakeet_engine/mod.rs:22`, `whisper_engine/mod.rs:6`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "same name as its containing module"` returning `0`.
  - Note: same 10 sites and lines. Verified: 0.

## 4. Rust: test-file fixes

- [x] 4.1 In `frontend/src-tauri/tests/db_inspect.rs`, replace the hardcoded `SqlitePool::connect("sqlite://C:/Users/vasiliy.kotov/...")` at line 5 with `std::env::var("MEETILY_DB_INSPECT_PATH").expect("set MEETILY_DB_INSPECT_PATH to a sqlite:// URL to inspect a local DB")`, and add `#[ignore = "manual DB inspection tool; run explicitly with cargo test --test db_inspect -- --ignored"]` above `#[tokio::test]`; verify with `cargo test -p meetily --test db_inspect` reporting the test as `ignored` (not run, not failing) with no env var set, and running successfully with `MEETILY_DB_INSPECT_PATH=sqlite://<path> cargo test -p meetily --test db_inspect -- --ignored` against a real local DB.
  - Note: with no env var it reports `1 ignored`. With the env var set to the local DB (`?mode=ro`) it connects and runs the meeting, column and migration queries, then panics at `query settings` because the tool queries an `app_settings` table that no migration has ever created. That stale SQL predates this change and is out of scope here; the env-var gating itself works.
- [x] 4.2 In `frontend/src-tauri/tests/repro_full_stop.rs:6`, remove `OnlineClusterEmbeddings` from the `use app_lib::audio::online_diarization::{...}` import; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "repro_full_stop.rs.*unused import"` returning `0`.
  - Note: `cargo clippy --fix` in 2.1 already applied this. Verified: 0.
- [x] 4.3 In `frontend/src-tauri/tests/repro_online_diarization.rs:260`, rename `let sys = right.unwrap_or_default();` to `let _sys = right.unwrap_or_default();`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "repro_online_diarization.rs.*unused variable"` returning `0`.
  - Note: `cargo clippy --fix` in 2.1 already applied this. Verified: 0.

## 5. Rust: final count

- [x] 5.1 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify the result is ≤ 40.
  - Note: after 3.x the count was 56, because `type_complexity`, dead code and similar were still there. Hand-fixed these purely mechanical ones to get under 40: `&PathBuf` -> `&Path` (15 signatures, including follow-on ones clippy found; callers coerce, plus `.to_path_buf()` where an owned value was built), 3 `clamp`, 3 doc-list indents, 2 file-header `///` -> `//!`, 1 unused `CommandExt` import (tokio's `Command` has its own `creation_flags`), 1 `.to_vec()`, 1 `?`, 1 `sort_by_key`. **Final: 32.** Left alone on purpose: 12 `type_complexity`, 4 `should_implement_trait`, 4 dead-code, 3 `needless_range_loop`/loop-counter, 2 `drop(&ref)` in `pipeline.rs` (these do nothing and may hide a real bug), 1 `MutexGuard` across await in `recording_commands.rs:1711` (a real concurrency fix, not lint hygiene), and a few singletons.

## 6. Frontend: mechanical auto-fix

- [x] 6.1 Run `cd frontend && npx next lint --fix`; verify with `git diff --stat` showing only entity-escaping and unused-import/variable removals, and `npx tsc --noEmit -p .` still passing (from change 02).
  - Note: the premise was wrong. `next lint --fix` fixes neither `react/no-unescaped-entities` nor `no-unused-vars`; it only fixed the 2 `prefer-const` (2 files). tsc passes.
- [x] 6.2 Run `npx next lint 2>&1 | grep -c "no-unescaped-entities"` and verify it returns `0`.
  - Note: all 64 escaped by a script that edits the exact lint positions (`'` -> `&apos;`, `"` -> `&quot;`, which render identically). Verified: 0.
- [x] 6.3 For any `no-unused-vars` site `--fix` could not resolve automatically (e.g. an unused function parameter that can't be blindly deleted without changing a callback signature), remove the unused identifier or prefix it with `_` by hand, file by file; verify with `npx next lint 2>&1 | grep -c "no-unused-vars"` returning `0`.
  - Note: all 111 done by hand (this repo's config does not exempt `_` prefixes). Unused imports were removed. Unused destructured props/hook fields were dropped from the destructuring (the prop types are unchanged). For `useState` values that are never read, the code now uses `[, setX]`. Unused `catch (e)` became `catch {`. Unused locals and dead handlers were deleted, along with what that left unused (`convertToMarkdown`, `startAudioLevelMonitoring`, Sidebar's `settingsSaveSuccess`). 8 sites that drop a key via rest destructuring, plus `OnboardingFlow`'s `onComplete` prop, got `eslint-disable-next-line ... -- <reason>` instead. Verified: 0.

## 7. Frontend: exhaustive-deps triage

- [ ] 7.1 Apply the 9 trivial dependency-array fixes listed in design.md D6 (`MessageToast.tsx:18`, `ModelSettingsModal.tsx:314`, `DownloadProgressStep.tsx:251`, `DownloadProgressStep.tsx:289`, `Sidebar/index.tsx:331`, `ConfigContext.tsx:227`, `RecordingStateContext.tsx:81`, `TranscriptContext.tsx:646`, `useRecordingStop.ts:521`); verify with `bun test tests/` still passing and manually smoke-testing the affected component/context still renders (no infinite-render warning in the browser console).
  - Note: rechecked all 9 against current code. **6 applied:** `ModelSettingsModal.tsx:299` +`setModelConfig` (ConfigContext `useState` setter), `DownloadProgressStep.tsx:251/289` +setters (raw `useState` setters passed through OnboardingContext), `Sidebar/index.tsx:271` -`expandedFolders` (useMemo), `RecordingStateContext.tsx:81` -`isPaused`/`isRecording` (useCallback), `useRecordingStop.ts:521` -`meetings`/`setMeetings` (useCallback, plus their now-unused destructuring). **3 not trivial, suppressed instead:** `MessageToast` `setShow` is a prop, not a local setter. `ConfigContext:227` is a mount-only IPC sync that would re-fire on every language change. `TranscriptContext:651` would re-fetch history whenever transcripts are cleared mid-recording. Side effect to know about: `handleRecordingStop` now changes identity less often, so `RecordingPostProcessingProvider` re-registers its `recording-stop-complete` listener less often. `bun test tests/` 71/0. **Left unchecked:** the manual smoke test (no infinite-render warning in the console) needs the desktop app running with a person watching.
- [x] 7.2 For each of the remaining 28 `react-hooks/exhaustive-deps` sites, add `// eslint-disable-next-line react-hooks/exhaustive-deps -- <reason>` immediately above the hook call, with a reason naming the specific non-memoized value that would cause a loop/behavior change if added (e.g. "fetchModels is recreated every render; adding it would refetch in a loop"); verify with `npx next lint 2>&1 | grep -c "react-hooks/exhaustive-deps"` returning `0` (all 28 now explicitly suppressed, not silently unaddressed) and `git grep -c "eslint-disable-next-line react-hooks/exhaustive-deps"` returning `28`.
  - Note: **31** suppressions, not 28 (28 plus the 3 from 7.1). Each one sits above the reported line (the deps array, or the declaration/cleanup line for `ConfigContext.tsx` `modelOptions` and `useAudioPlayer.ts`) and gives a site-specific reason. Verified: lint reports 0 `exhaustive-deps`; `git grep` total = 31.

## 8. Final verification

- [x] 8.1 Run `npx next lint`; verify the summary reports `0` errors for `no-unused-vars` and `no-unescaped-entities` combined, and that the only remaining errors are the 91 pre-existing `no-explicit-any` (carried forward to change 09).
  - Note: 91 errors, all `no-explicit-any`; 0 warnings.
- [x] 8.2 Run `cargo clippy -p meetily --all-targets --message-format=short`, `cargo test -p meetily --lib`, `bun test tests/`, and `npx tsc --noEmit -p .`; verify all pass and the Rust warning count is ≤ 40.
  - Note: clippy 32 warnings / 0 errors; `cargo check --all-targets` clean; lib tests 508/0/9 with the 3 device tests skipped (see 2.2, environment-caused); bun 71/0; tsc 0 errors.
- [x] 8.3 Run `openspec validate 03-lint-baseline-cleanup --strict`; verify it passes.
