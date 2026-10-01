# Design

## Context

See `proposal.md` for motivation. Both counts reproduced 2026-09-18 on `feat/diarization` (Rust toolchain 1.97.1, bun 1.4.2 installed for this investigation):

**Rust — `cargo clippy -p meetily --all-targets --message-format=short`: 223 warnings, 0 errors.** Grouped by message text (each line below is a distinct lint category, not a lint-name-exact grep, since `--message-format=short` did not emit bracketed `[clippy::lint_name]` suffixes in this environment — categorization here is by the diagnostic's own wording, cross-checked against clippy's lint docs where the fix differs from what the wording implies):
| Count | Category | Auto-fixable via `cargo clippy --fix`? |
|---|---|---|
| 34 | redundant reference in `info!`/`error!`/`format!` argument (`&x` where `x` is already `Copy`/cheap) | Yes |
| 12 | very complex type (`type_complexity`) — 9 in `src/audio/{diarization,online_diarization,recording_commands,recording_state}.rs`, 3 in `tests/db_inspect.rs` | No (needs a named `type` alias — a real, if small, code change) |
| 12 | unused import | Yes |
| 12 | unnecessary `.clone()` to build a one-element slice (`database/repositories/speaker.rs`, all `std::slice::from_ref(&x)`) | Yes |
| 12 | redundant closure (`\|x\| f(x)` → `f`) | Yes |
| 11 | `&PathBuf` where `&Path` suffices | Yes |
| 10 | `module_inception` (a `mod.rs` whose module has the same name as its parent directory) — `analytics`, `anthropic`, `api`, `console_utils`, `groq`, `ollama`, `openai`, `openrouter`, `parakeet_engine`, `whisper_engine` | No (requires a rename or an `#[allow]`) |
| 7 | useless `.into()`/cast to the same type | Yes |
| 7 | reference immediately dereferenced by the compiler | Yes |
| 7 | casting to the same type (`as f32` on an `f32`, etc.) | Yes |
| 11 | `this function has too many arguments` (8-20 params against clippy's default max of 7) — `analytics.rs:424` (13), `analytics/commands.rs:325` (12), `api/api.rs:557` (8), `api/api.rs:1373` (8), `audio/pipeline.rs:714` (10), `audio/pipeline.rs:1278` (11), `audio/online_diarization.rs:130` (8), `summary/commands.rs:329` (12), `summary/llm_client.rs:115` (14), `summary/processor.rs:326` (20), `summary/service.rs:300` (9) | No (needs `#[allow]` or a params struct) |
| ~2 | `MutexGuard` held across an `.await` — `audio/recording_commands.rs:1765`, `:2321` | No, and **not touched here** — this is exactly what change 04 (recording-lock-hardening) is for; fixing it is a real concurrency change, not a mechanical lint fix |
| remainder (~98) | assorted single/low-count categories: unneeded `return`, `unnecessary_mut`, `Default` derivation, manual `RangeInclusive::contains`, `div_ceil`, clamp-pattern, dead code (`never used`/`never read`), doc-comment formatting, `.get(0)` → `.first()`, `Iterator::last` → `.next_back()`, etc. | Mostly yes |

**Frontend — `next lint`: 268 errors, 37 warnings.** By rule:
| Count | Rule | In scope this change? |
|---|---|---|
| 111 | `@typescript-eslint/no-unused-vars` | Yes — `next lint --fix` handles unused imports; unused locals/params need per-site removal |
| 91 | `@typescript-eslint/no-explicit-any` | **No** — change 09 |
| 64 | `react/no-unescaped-entities` | Yes — `next lint --fix` handles all of these (HTML-entity substitution is mechanical and behavior-preserving) |
| 37 | `react-hooks/exhaustive-deps` (warnings, not errors) | Partially — 9 of 37 are trivial (see Decisions D4); the other 28 are suppressed with a reason, not fixed |

`db_inspect.rs:5` hardcodes `sqlite://C:/Users/vasiliy.kotov/AppData/Roaming/com.meetily.ai/meeting_minutes.sqlite?mode=ro` — this is `#[tokio::test] async fn inspect_meeting_0818_1322()`, a developer-authored, print-only DB inspection tool (no assertions, just `println!`s of table contents) that only works on the one developer's machine and is not gated behind `#[ignore]`, so `cargo test` (and any future CI test run) would fail on every other machine.

`tests/repro_full_stop.rs:6` imports `OnlineClusterEmbeddings` and never uses it. `tests/repro_online_diarization.rs:260` binds `sys` (`let sys = right.unwrap_or_default();`) inside one specific test function and never reads it there (the identifier is used in four *other* test functions in the same file, which is why the bindings there are fine).

No `.github/workflows/*.yml` runs `cargo clippy` today (`git grep -n "clippy" .github/workflows` returns nothing) — clippy is a purely local/manual check right now.

## Goals / Non-Goals

**Goals:**
- Reduce both warning counts using only mechanical, behavior-preserving fixes, verified by `cargo test -p meetily --lib` / `bun test tests/` still passing and no diff outside formatting/import/reference-level changes.
- State exact, reproducible before/after numbers and a numeric exit criterion, so a future change can safely turn on `-D warnings`/lint-as-error without guessing at "is this low enough yet."
- Leave a clear, auditable trail for what was suppressed (`#[allow(...)]` / `eslint-disable-next-line`) versus fixed, with a reason on every suppression.

**Non-Goals:**
- Restructuring any flagged function's signature (`too_many_arguments`) or splitting any flagged module (`module_inception`) — both are `#[allow]`'d with a comment pointing at the future change that legitimately owns that refactor (04-10).
- `no-explicit-any` (change 09) and anything touching `summary/processor.rs`'s signature (out of scope per the brief; owned by later summary-service work, not named in the 11-change plan but explicitly excluded here).
- Enabling `-D warnings` (Rust) or making `next lint` block CI (frontend) — see D6/Open Questions: this change reduces the count and states the target; flipping the switch is left to whoever owns CI policy, since it's a one-line change with a much bigger blast radius (blocks every future PR) than anything else in this change.

## Decisions

### D1: `cargo clippy --fix --allow-dirty -p meetily --all-targets` for the mechanical majority, applied then hand-verified
Run the auto-fixer for the categories marked "Yes" in the Context table (roughly 125 of 223 warnings: redundant references, unused imports, redundant closures, `&PathBuf`→`&Path`, useless conversions/casts, dereference-simplification, and the low-count categories). After it runs:
- Re-run `cargo clippy -p meetily --all-targets` and confirm the fixed categories are gone and the count dropped by exactly the expected amount.
- Run `cargo test -p meetily --lib` to confirm no behavior changed (all of these lints are non-semantic by clippy's own classification — they're `style`/`complexity`/`pedantic`-level rewrites of equivalent code).
- `--allow-dirty` is required because the working tree has an unrelated in-progress change on this branch; this change's diff is still scoped to the files clippy actually touches.

### D2: `#[allow(clippy::too_many_arguments)]` at the 11 flagged functions, with a one-line reason each — no params-struct refactor
Add `#[allow(clippy::too_many_arguments)] // <n> params; owned by change NN, not a mechanical lint fix` directly above each of the 11 function signatures listed in the Context table.
- A params-struct refactor changes every call site's shape — a real API change, not hygiene. Several of the flagged functions are inside files explicitly slated for structural work: `audio/pipeline.rs` (04/05), `audio/online_diarization.rs` (05), `summary/processor.rs`/`llm_client.rs`/`service.rs`/`commands.rs` (change 10's shared-LLM-client work touches all four). Fixing signatures here would conflict with or be redone by that work.
- `analytics.rs:424`/`analytics/commands.rs:325`/`api/api.rs:557,1373` are not named in any of changes 04-11; their `#[allow]` reason says "no owning change yet" rather than pointing at a specific one, so a future auditor doesn't assume it's covered elsewhere.
- Alternative considered: introduce a small params struct only for the two smallest offenders (`api/api.rs:557` at 8/7, `audio/online_diarization.rs:130` at 8/7) since the brief allows "a params struct only where trivial." Rejected after reading both call sites — `api/api.rs:557` is called from 6 places with different subsets of optional trailing args already defaulted inline, and `online_diarization.rs:130` is a constructor called from `diarization.rs` at 4 sites that change 05 (diarization engine unification) will rewrite anyway; a struct now would be redone or conflict with that.

### D3: `#[allow(clippy::module_inception)]` at each `mod.rs`, not a rename
Add `#[allow(clippy::module_inception)]` to the top of each of the 10 flagged `mod.rs` files, with a comment: `// mod.rs re-exports the module of the same name by convention; renaming would require updating every call site.`
- Renaming (e.g. `whisper_engine::whisper_engine` → `whisper_engine::engine`) touches every `use` and qualified call across the crate for a purely stylistic lint with no runtime effect — high diff-churn, zero behavior benefit, and directly conflicts with changes that are about to touch several of these same modules (`whisper_engine` isn't separately owned, but `parakeet_engine`/`ollama`/`openrouter` etc. are exactly the modules change 10 unifies into a shared LLM client).
- This matches the brief's own recommendation ("recommend allow to avoid churn").

### D4: `db_inspect.rs` — env-var-gated path, `#[ignore]`, not deleted
Change `tests/db_inspect.rs` to read the DB path from `std::env::var("MEETILY_DB_INSPECT_PATH")`, `.expect("set MEETILY_DB_INSPECT_PATH to a sqlite:// URL to inspect a local DB")`, and add `#[ignore = "manual DB inspection tool; run explicitly with cargo test --test db_inspect -- --ignored"]` above the `#[tokio::test]` attribute.
- Deleting it outright (the brief's other suggested option) throws away a genuinely useful manual debugging tool (it prints meeting/transcript/speaker table contents for troubleshooting a real DB) for no gain, once it's no longer a landmine for `cargo test`/CI on other machines.
- `#[ignore]` alone (keeping the hardcoded path) would stop it breaking CI but would still be useless to anyone but the original author; the env var makes it actually reusable.
- This does not touch the file's clippy `type_complexity` warnings (3 of them, on the same lines as the query result tuples) — those stay as pre-existing style debt in a manual tool, not worth a `type` alias for.

### D5: The 2 test-binary warnings — delete the unused import, prefix the unused variable
- `tests/repro_full_stop.rs:6`: drop `OnlineClusterEmbeddings` from the `use app_lib::audio::online_diarization::{...}` import list (confirmed unused anywhere else in the file).
- `tests/repro_online_diarization.rs:260`: rename `let sys = right.unwrap_or_default();` to `let _sys = right.unwrap_or_default();` (confirmed genuinely unused in that specific test function, unlike the other 4 occurrences of `sys` in the same file which are used).

### D6: Frontend — `next lint --fix` first, then a per-site `exhaustive-deps` triage; defer enforcement
1. `next lint --fix` mechanically resolves all 64 `react/no-unescaped-entities` (HTML-entity substitution) and the subset of the 111 `no-unused-vars` that are pure removals (unused imports, unused destructured params where removing them doesn't change a function's call signature meaning).
2. Of the 37 `react-hooks/exhaustive-deps` warnings, exactly 9 are trivial — the missing/extra dependency is either a `useState` setter (React guarantees these are referentially stable across renders) or a plain primitive field read, so adding/removing it cannot introduce an infinite render loop or change effect timing:

   | File:line | Fix |
   |---|---|
   | `components/MessageToast.tsx:18` | add `setShow` |
   | `components/ModelSettingsModal.tsx:314` | add `setModelConfig` |
   | `components/onboarding/steps/DownloadProgressStep.tsx:251` | add `setParakeetDownloaded` |
   | `components/onboarding/steps/DownloadProgressStep.tsx:289` | add `setSummaryModelDownloaded` |
   | `components/Sidebar/index.tsx:331` | remove unnecessary `expandedFolders` |
   | `contexts/ConfigContext.tsx:227` | add `selectedLanguage` |
   | `contexts/RecordingStateContext.tsx:81` | remove unnecessary `state.isPaused`, `state.isRecording` |
   | `contexts/TranscriptContext.tsx:646` | add `transcripts.length` |
   | `hooks/useRecordingStop.ts:521` | remove unnecessary `meetings`, `setMeetings` |

3. The remaining 28 sites all involve a non-memoized function (e.g. `fetchModels`, `downloadModel`, `resetSelection`, `onOpenModelSettings`, `startPolling`, `updateProviderApiKey`, ...), a value that is a new object/array every render (`modelOptions`, `baseItems`), or a structural suggestion (wrap in `useMemo`, copy a ref before cleanup). Adding the dependency as suggested risks an infinite effect loop or a behavior change (effect firing far more often); each gets `// eslint-disable-next-line react-hooks/exhaustive-deps -- <specific reason, e.g. "fetchModels is recreated every render; adding it would refetch in a loop">` directly above the hook call, and the 28 are counted in the exit criterion below (down from 37 unaddressed today, to 28 explicitly suppressed with a reason — 0 silent).
   - Alternative considered: wrap every listed function in `useCallback` so it becomes safe to add. Rejected — several of these functions close over state that itself isn't memoized (e.g. `ModelSettingsModal.tsx`'s `fetchOllamaModels`), so `useCallback`-wrapping them correctly is a real refactor of component internals, which is exactly what change 09 (frontend-split-large-components) does to this same file; doing it piecemeal here would conflict.

### D7: Exit criteria are numeric and independently checkable; enforcing them is deferred
- Rust: `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -cE '\.rs:[0-9]+:[0-9]+: warning:'` ≤ 40 (down from 223: the 11 `too_many_arguments` + 10 `module_inception` `#[allow]`'d + ~12 `type_complexity` left as-is + a small remainder of low-count categories not covered by `--fix` and judged not worth a manual one-off = the residual budget).
- Frontend: `next lint` reports 0 errors (down from 268: 111 `no-unused-vars` + 64 `no-unescaped-entities` fixed, 91 `no-explicit-any` is a *warning-level exclusion from this count only because it's out of scope, not because it's silenced* — **correction, see below**) — ***`no-explicit-any` is configured as `Error` in this repo's `.eslintrc`, not `Warning`***, so it cannot be excluded from the "0 errors" bar without a config change. See Open Questions: the exit criterion as stated by the brief ("eslint 0 errors") is not achievable in this change without either fixing `no-explicit-any` (change 09's job) or demoting it to a warning in `.eslintrc` (a scope decision, not a mechanical fix). This change's actual, honest exit criterion is: **0 errors from `no-unused-vars` and `no-unescaped-entities`** (175 of 268 fixed), with `no-explicit-any` (91) explicitly carried forward to change 09, and `exhaustive-deps` warnings reduced from 37 unaddressed to 28 explicitly-suppressed-with-reason.
- CI enforcement (`-D warnings` / failing on any `next lint` error) is not added in this change. It belongs in `02-frontend-test-pipeline-and-regressions`'s `frontend-checks` job (already non-blocking for lint per that change's D4) or a Rust equivalent added to the same workflow — but flipping either to blocking today would immediately fail on the 91 `no-explicit-any` errors that are explicitly deferred to change 09. Enforcement is therefore correctly change 09's follow-up, not this change's.

## Risks / Trade-offs

- **`cargo clippy --fix` can occasionally change semantics for lints that look purely stylistic** (e.g. `redundant_clone` interacting with `Drop`) → Mitigated: none of the categories in the "Yes" column of the Context table touch ownership/`Drop` timing (they're references, imports, closures-to-function-pointers, and same-type casts); `cargo test -p meetily --lib` after the fix is the task-level gate.
- **The honest eslint exit criterion (0 errors *excluding* `no-explicit-any`) doesn't match the brief's literal "eslint 0 errors"** → Surfaced explicitly in D7 rather than silently redefining "0 errors" to mean something else; flagged for the change owner to confirm before merge.
- **28 suppressed `exhaustive-deps` warnings is a permanent-looking scar** → Accepted; each carries a specific reason so a future contributor (likely change 09) can revisit them once the components they're in are memoized properly, rather than rediscovering why they're disabled from scratch.
- **`#[allow(clippy::too_many_arguments)]` on `summary/processor.rs:326` (20 params) looks like it's condoning a real problem** → It is a real problem, and the `#[allow]` comment says so explicitly rather than hiding it; the brief is explicit that this change must not refactor that file.

## Migration Plan
- No data/runtime migration. Land as one PR (or a small stack): (1) `cargo clippy --fix` + manual `#[allow]`s + `db_inspect.rs`/test-binary fixes, verified by `cargo test -p meetily --lib`; (2) `next lint --fix` + manual `exhaustive-deps` triage, verified by `bun test tests/` (from change 02) and a manual smoke of any touched component.
- Rollback: revert the commit(s); nothing here is stateful.

## Open Questions
- Should `@typescript-eslint/no-explicit-any` be demoted from `error` to `warn` in `.eslintrc` now (unblocking a future "0 errors" CI gate before change 09 lands), or left as `error` with CI enforcement deferred until change 09 finishes? This change takes no position and leaves both eslint severity config and CI enforcement untouched.
- Should the Rust `-D warnings` / eslint-error CI gate be added to `02-frontend-test-pipeline-and-regressions`'s workflow once this change's numeric targets are met, as a small follow-up to that change instead of a new one?
