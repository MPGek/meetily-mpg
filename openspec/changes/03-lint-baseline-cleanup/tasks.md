# Tasks

## 1. Baseline counts (record before changing anything)

- [ ] 1.1 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | tee clippy-before.txt | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify it prints `223` (or record the actual number if it has drifted, and use that as the baseline instead).
- [ ] 1.2 Run `cd frontend && npx next lint 2>&1 | tee ../lint-before.txt`; verify the summary line reads `268 problems (268 errors, 37 warnings)` (or record the actual numbers if drifted).

## 2. Rust: mechanical auto-fix

- [ ] 2.1 Run `cargo clippy --fix --allow-dirty -p meetily --all-targets`; verify with `cargo check -p meetily` (compiles) and `git diff --stat` showing only import/reference/closure/cast-level changes (no logic restructuring).
- [ ] 2.2 Run `cargo test -p meetily --lib` and confirm it passes with the same test count as before task 2.1; verify no test outcome changed.
- [ ] 2.3 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify the count dropped by roughly 100-130 from the task 1.1 baseline (the categories marked "Yes" in design.md's table).

## 3. Rust: manual `#[allow]` annotations

- [ ] 3.1 Add `#[allow(clippy::too_many_arguments)]` with a one-line reason comment above each of the 11 flagged functions: `analytics/analytics.rs:424`, `analytics/commands.rs:325`, `api/api.rs:557`, `api/api.rs:1373`, `audio/pipeline.rs:714`, `audio/pipeline.rs:1278`, `audio/online_diarization.rs:130`, `summary/commands.rs:329`, `summary/llm_client.rs:115`, `summary/processor.rs:326`, `summary/service.rs:300`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "too many arguments"` returning `0`.
- [ ] 3.2 Add `#[allow(clippy::module_inception)]` with a one-line reason comment above the `mod` item in each of the 10 flagged files: `analytics/mod.rs:1`, `anthropic/mod.rs:1`, `api/mod.rs:1`, `console_utils/mod.rs:2`, `groq/mod.rs:1`, `ollama/mod.rs:3`, `openai/mod.rs:1`, `openrouter/mod.rs:2`, `parakeet_engine/mod.rs:22`, `whisper_engine/mod.rs:6`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "same name as its containing module"` returning `0`.

## 4. Rust: test-file fixes

- [ ] 4.1 In `frontend/src-tauri/tests/db_inspect.rs`, replace the hardcoded `SqlitePool::connect("sqlite://C:/Users/vasiliy.kotov/...")` at line 5 with `std::env::var("MEETILY_DB_INSPECT_PATH").expect("set MEETILY_DB_INSPECT_PATH to a sqlite:// URL to inspect a local DB")`, and add `#[ignore = "manual DB inspection tool; run explicitly with cargo test --test db_inspect -- --ignored"]` above `#[tokio::test]`; verify with `cargo test -p meetily --test db_inspect` reporting the test as `ignored` (not run, not failing) with no env var set, and running successfully with `MEETILY_DB_INSPECT_PATH=sqlite://<path> cargo test -p meetily --test db_inspect -- --ignored` against a real local DB.
- [ ] 4.2 In `frontend/src-tauri/tests/repro_full_stop.rs:6`, remove `OnlineClusterEmbeddings` from the `use app_lib::audio::online_diarization::{...}` import; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "repro_full_stop.rs.*unused import"` returning `0`.
- [ ] 4.3 In `frontend/src-tauri/tests/repro_online_diarization.rs:260`, rename `let sys = right.unwrap_or_default();` to `let _sys = right.unwrap_or_default();`; verify with `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "repro_online_diarization.rs.*unused variable"` returning `0`.

## 5. Rust: final count

- [ ] 5.1 Run `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'`; verify the result is ≤ 40.

## 6. Frontend: mechanical auto-fix

- [ ] 6.1 Run `cd frontend && npx next lint --fix`; verify with `git diff --stat` showing only entity-escaping and unused-import/variable removals, and `npx tsc --noEmit -p .` still passing (from change 02).
- [ ] 6.2 Run `npx next lint 2>&1 | grep -c "no-unescaped-entities"` and verify it returns `0`.
- [ ] 6.3 For any `no-unused-vars` site `--fix` could not resolve automatically (e.g. an unused function parameter that can't be blindly deleted without changing a callback signature), remove the unused identifier or prefix it with `_` by hand, file by file; verify with `npx next lint 2>&1 | grep -c "no-unused-vars"` returning `0`.

## 7. Frontend: exhaustive-deps triage

- [ ] 7.1 Apply the 9 trivial dependency-array fixes listed in design.md D6 (`MessageToast.tsx:18`, `ModelSettingsModal.tsx:314`, `DownloadProgressStep.tsx:251`, `DownloadProgressStep.tsx:289`, `Sidebar/index.tsx:331`, `ConfigContext.tsx:227`, `RecordingStateContext.tsx:81`, `TranscriptContext.tsx:646`, `useRecordingStop.ts:521`); verify with `bun test tests/` still passing and manually smoke-testing the affected component/context still renders (no infinite-render warning in the browser console).
- [ ] 7.2 For each of the remaining 28 `react-hooks/exhaustive-deps` sites, add `// eslint-disable-next-line react-hooks/exhaustive-deps -- <reason>` immediately above the hook call, with a reason naming the specific non-memoized value that would cause a loop/behavior change if added (e.g. "fetchModels is recreated every render; adding it would refetch in a loop"); verify with `npx next lint 2>&1 | grep -c "react-hooks/exhaustive-deps"` returning `0` (all 28 now explicitly suppressed, not silently unaddressed) and `git grep -c "eslint-disable-next-line react-hooks/exhaustive-deps"` returning `28`.

## 8. Final verification

- [ ] 8.1 Run `npx next lint`; verify the summary reports `0` errors for `no-unused-vars` and `no-unescaped-entities` combined, and that the only remaining errors are the 91 pre-existing `no-explicit-any` (carried forward to change 09).
- [ ] 8.2 Run `cargo clippy -p meetily --all-targets --message-format=short`, `cargo test -p meetily --lib`, `bun test tests/`, and `npx tsc --noEmit -p .`; verify all pass and the Rust warning count is ≤ 40.
- [ ] 8.3 Run `openspec validate 03-lint-baseline-cleanup --strict`; verify it passes.
