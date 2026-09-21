# Design

## Context

See `proposal.md` for motivation. Verified 2026-09-18 (bun 1.4.2 installed locally to reproduce; it is not present in every environment by default):

- `frontend/package.json` `scripts` has `dev`, `build`, `start`, `lint`, several `tauri:*` scripts, but no `test`.
- `frontend/tests/lib/*.test.ts` (6 files) import from `bun:test`. `frontend/tests/lib/onboarding-summary-model.test.mjs` instead hand-compiles `src/lib/onboarding-summary-model.ts` with `ts.transpileModule` and runs it in a `vm` context, asserting with `node:assert/strict` — a working but bespoke pattern, out of step with the other 6 files.
- `frontend/pnpm-lock.yaml` exists; `pnpm install` is what every `.github/workflows/build*.yml` uses (`Install frontend dependencies` step: `cd frontend && pnpm install`). Bun is used only as the *test runner*, not the package manager — nothing in this change touches that split.
- `npx tsc --noEmit -p .` fails with 5 errors, one per file importing `bun:test`: `Cannot find module 'bun:test' or its corresponding type declarations.` `tsconfig.json`'s `include` covers `**/*.ts`/`**/*.tsx` repo-wide (tests included) and has no `types` array, so any `@types/*` package is auto-included once present.
- Installing `@types/bun@1.4.2` alone is not sufficient: its `index.d.ts` is just `/// <reference types="bun-types" />` and it declares `"dependencies": { "bun-types": "1.4.2" }`. A package manager (`pnpm add -D @types/bun@1.4.2`) resolves that transitive dependency automatically; manually vendoring only `@types/bun` into `node_modules` (as this investigation did to verify) does not. After adding both (as `pnpm`/`npm` would), `npx tsc --noEmit -p .` reports **zero** errors — verified locally.
- `bun test tests/` (having installed bun 1.4.2 via `bun.sh/install.ps1` for this investigation) reports **48 pass, 2 fail** exactly as the prior audit recorded:
  1. `diarization-status-lines.test.ts:232` (`blink states > a loaded model without work is steady, not blinking`): `expect(indicator.state).toBe("healthy")` receives `"idle"`.
  2. `live-speaker-labels.test.ts:85` (`rewriteTurnsInWindow > rewrites only the turn(s) overlapping the window and same channel`): `expect(out[1]).toMatchObject({ display_name: undefined, matched_by: undefined })` fails with a diff showing `out[1]`'s actual keys (`end_time`, `source_device`, `speaker`, `start_time`) instead.
- Applying the two one-line fixes described in Decisions below (verified in a local, reverted edit) brings the suite to **50 pass, 0 fail**.
- No `.github/workflows/*.yml` currently triggers on `push` or `pull_request`. `pr-main-check.yml` is `on: workflow_dispatch` only and its one job checks the version string in `tauri.conf.json` matches semver — it does not build, lint, or test anything, despite the name suggesting a PR gate. `build.yml`/`build-devtest.yml`/etc. are `workflow_call`/`workflow_dispatch` only (full native builds, too slow/heavy for a PR gate). No workflow anywhere runs `next lint` or `tsc --noEmit`.

## Goals / Non-Goals

**Goals:**
- Every pull request against `main` automatically runs the frontend type check and unit tests, and a failure in either blocks the check.
- The two failing tests are fixed by identifying and correcting whichever side (test or `src/`) actually deviates from the governing spec, not by loosening an assertion to make it pass.
- `tsc --noEmit` is clean with the test files included, matching how they're actually authored (no need to carve `tests/` out of the TS project).

**Non-Goals:**
- Making `next lint` blocking — that's change 03's job, once the warning count is mechanically reduced; this change adds the lint step non-blocking so it's visible but not yet a merge gate.
- Any `src/` behavior change. Both investigated failures are test-authoring bugs (below); no production code in `src/lib/diarization-status-lines.ts` or `src/lib/live-speaker-labels.ts` is touched.
- Switching the frontend's package manager from `pnpm` to `bun`, or changing how native builds install dependencies.
- Writing new tests for previously-untested code (hooks, contexts, components) — out of scope; change 09 is where component splitting adds first tests for `TranscriptContext`/`useSummaryGeneration`.

## Decisions

### D1: `bun test tests/` as the `test` script, not `vitest`
`package.json` gains `"test": "bun test tests/"`.
- 6 of 7 existing test files already import `bun:test` directly; switching the runner to Vitest would require rewriting every one of them (different mock/assertion APIs) for no behavioral gain.
- Bun 1.4.2 is the version the audit records as already expected/installed in the working environment, and `oven-sh/setup-bun` makes it trivial to provision in CI (a single action step, no separate Node test-runner config).
- Alternative considered: Vitest with a `bun:test`-compatible shim. Rejected — adds a dependency and a shim layer to work around a runner the tests weren't written for, when the native runner already works.

### D2: Fix `tsc --noEmit`'s `bun:test` resolution with `@types/bun`, not a tsconfig exclude
Add `"@types/bun": "1.4.2"` to `devDependencies` (which pulls in `bun-types` transitively — see Context) rather than excluding `tests/` from `tsconfig.json`.
- Verified: with `@types/bun` (and its `bun-types` dependency) present, `npx tsc --noEmit -p .` reports zero errors, including for the test files.
- Excluding `tests/` from `tsconfig.json` was the brief's suggested alternative; rejected because it would also stop `tsc` from type-checking the tests at all (not just silencing the `bun:test` resolution error), losing real type-safety coverage on the test files themselves.
- Version pinned to `1.4.2` to match the bun runtime version this change standardizes on (D1); an unpinned `^1.x` range risks a types/runtime mismatch on a future bun major bump going unnoticed.

### D3: Add a `pull_request` trigger to `pr-main-check.yml` rather than only adding a step
The brief named `pr-main-check.yml` as where to add the test step. Verified: that workflow is `workflow_dispatch`-only today (Context), so a step added without also adding a trigger would never run automatically — defeating the purpose of "CI catches regressions." This change adds:
```yaml
on:
  workflow_dispatch:
  pull_request:
    branches: [main]
```
alongside a new `frontend-checks` job (independent of the existing `validation-check` job, so either can fail without blocking the other's diagnostics).
- Alternative considered: create a brand-new workflow file instead of extending `pr-main-check.yml`. Rejected — the brief and the workflow's own name both point at it as the intended PR gate; leaving it non-functional while adding a parallel file would be confusing, and nothing else in the repo currently fills that role.
- Alternative considered: trigger on `push` to any branch. Rejected — noisier than needed; `pull_request` targeting `main` is where a regression needs to be caught before merge.

### D4: `next lint` runs in CI but does not fail the job (`continue-on-error: true`)
The new `frontend-checks` job runs `pnpm exec next lint`, but that step is marked non-blocking.
- Today `next lint` reports 268 errors — making it blocking now would fail every existing and future PR until change 03 lands, which is explicitly a separate, larger effort.
- Running it (non-blocking) from this change onward means its count is visible in every PR from day one, and change 03 only has to flip `continue-on-error` once its numeric target is met, not also wire up the step.

### D5: `diarization-status-lines.test.ts:226` is a test bug — fix the fixture, not the code
Traced against `src/lib/diarization-status-lines.ts:143-190` (`buildModelIndicators`) and `openspec/specs/online-diarization-telemetry/spec.md`'s "Visual model state indicators" requirement ("...an indicator SHALL NOT be shown as healthy when the underlying model is not loaded" / scenario "Loaded and idle": "a model is loaded but has no work in flight → steady healthy color"):
- The test's `alignment()` fixture factory (`tests/lib/diarization-status-lines.test.ts:44-57`) defaults `loaded: false`. Every other test in the same `describe("blink states", ...)` block that means to exercise the "loaded" path passes `loaded: true` explicitly (lines 202, 213 in the surrounding tests) — the failing test at line 226 (`alignment({ queued_jobs: 0, requested: false })`) is the one place in the block that omits it, despite its own title ("a **loaded** model without work is steady, not blinking").
- With `loaded: false` (the fixture's default), `buildModelIndicators` correctly computes `alignmentState = 'idle'` (line 167-173: `!alignment.enabled ? 'idle' : alignment.dropped > 0 ? 'warning' : alignment.loaded ? 'healthy' : 'idle'`) — exactly matching the spec's "SHALL NOT be shown as healthy when the underlying model is not loaded." The code is correct; the test fixture doesn't set up the state its own title describes.
- Fix: add `loaded: true` to the `alignment({...})` call at that line. Verified locally (reverted after verification): with the fix, `bun test tests/` passes 50/50.

### D6: `live-speaker-labels.test.ts:85` is a test bug — bun's `toMatchObject` doesn't match `{ key: undefined }` against a missing key
Traced against `src/lib/live-speaker-labels.ts:72-87` (`rewriteTurnsInWindow`) and `openspec/specs/live-speaker-labels/spec.md`'s window-override requirements (a window-scoped rewrite applies only to the turn(s) overlapping the given window and channel; others are left unchanged):
- Manually walked all four input turns against `rewriteTurnsInWindow(turns, "SPEAKER_01", "System", 0, 5, "Alice")`: turn 0 (SPEAKER_01/System, [0,4)) overlaps the window and is correctly rewritten; turn 1 (SPEAKER_01/System, [8,10)) does not overlap `[0,5)` and is correctly left unchanged; turn 2 (SPEAKER_01/Microphone) is correctly skipped (wrong channel); turn 3 (SPEAKER_02) is correctly skipped (different cluster). This matches the spec and the three surrounding assertions (lines 83, 87, 89) all pass.
- The failure is specifically `expect(out[1]).toMatchObject({ display_name: undefined, matched_by: undefined })`. `out[1]` is the untouched original turn object, which never had a `display_name`/`matched_by` key at all (the `turn()` fixture factory only sets those when passed in `partial`). Running it under bun 1.4.2 shows the matcher does not treat "expected `undefined`" as satisfied by "key absent" the way Jest's `toMatchObject` does; it reports the actual object's real keys as a mismatch.
- The two adjacent, structurally identical checks in the very same test — `expect(out[2].display_name).toBeUndefined();` (line 87) and `expect(out[3].display_name).toBeUndefined();` (line 89) — already use the form that works under bun. Line 85 is the one outlier using `toMatchObject` with `undefined` values.
- Fix: replace line 85 with the same pattern as its neighbors: `expect(out[1].display_name).toBeUndefined(); expect(out[1].matched_by).toBeUndefined();`. Verified locally (reverted after verification): with the fix, `bun test tests/` passes 50/50.

### D7: Port `onboarding-summary-model.test.mjs` to `bun:test` (cheap, done in this change)
The `.mjs` file's 8 assertions map 1:1 onto `describe`/`test`/`expect` blocks importing directly from `../../src/lib/onboarding-summary-model` (no `vm`/`ts.transpileModule` needed — bun runs TypeScript natively). This also makes each assertion appear as a named test in `bun test`'s output instead of running silently as file-level code, and removes the file's `typescript` compiler-API dependency at test time.

## Risks / Trade-offs

- **Adding a `pull_request` trigger to `pr-main-check.yml` changes when that workflow runs, beyond just adding a step** → Intentional (D3); called out explicitly since it's a broader change than "add one step" and is worth the implementer double-checking against how the team actually wants PR gating to work.
- **`next lint` non-blocking (D4) means lint regressions can still merge** → Accepted for this change; the count is visible in CI output and change 03 defines the numeric bar for flipping it to blocking.
- **`@types/bun` pinned to an exact version (1.4.2) will drift from whatever bun version CI/developers actually run over time** → Low risk, easy fix (bump both together); flagged so it isn't forgotten silently.

## Migration Plan
- Additive: new script, new devDependency, new CI trigger/job, two one-line test fixes, one test file format conversion. No data migration, no runtime behavior change.
- Rollback: revert the commit. The two test fixes are independently safe to keep even if the CI wiring is reverted (they make the existing suite pass 50/50 either way).

## Open Questions
- None — CI trigger scope (D3) and lint blocking threshold (deferred to change 03, D4) are the two decisions with the widest blast radius, and both are made explicitly above rather than left open.
