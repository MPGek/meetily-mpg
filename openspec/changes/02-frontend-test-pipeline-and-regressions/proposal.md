# Proposal

## Why

The frontend has 7 test files under `frontend/tests/lib` but `package.json` has no `test` script, `tsc --noEmit` fails with 5× `TS2307: Cannot find module 'bun:test'`, and no CI workflow runs any frontend check at all (`.github/workflows/pr-main-check.yml`, despite its name, is `workflow_dispatch`-only and only validates a version string). Two of the 50 existing tests fail today for reasons unrelated to the code under test. Regressions in `src/lib/*` can land silently. Fix the pipeline and the two failures together so the new CI step starts green.

## What Changes

- Add `"test": "bun test tests/"` to `frontend/package.json` `scripts`.
- Add `"@types/bun": "1.4.2"` to `frontend/package.json` `devDependencies` (pulls in `bun-types` transitively) so `bun:test` resolves for `tsc --noEmit`, matching the bun version the test script itself requires.
- Add a `frontend-checks` job to `.github/workflows/pr-main-check.yml`: install bun via `oven-sh/setup-bun`, `pnpm install` (the frontend's existing package manager), then run `pnpm exec tsc --noEmit -p .`, `pnpm exec next lint` (non-blocking for now — see design.md), and `bun test tests/`.
- Add a `pull_request` trigger (targeting `main`) to `pr-main-check.yml` alongside its existing `workflow_dispatch` — otherwise the new job would only ever run when someone manually dispatches the workflow, defeating the point of "CI catches regressions."
- Fix `frontend/tests/lib/diarization-status-lines.test.ts:226` — the alignment-model fixture in the "a loaded model without work is steady, not blinking" test omits `loaded: true`, so it exercises the *unloaded* path and fails against `buildModelIndicators`'s (spec-correct) idle state. **Test bug**, not a code bug.
- Fix `frontend/tests/lib/live-speaker-labels.test.ts:85` — `toMatchObject({ display_name: undefined, matched_by: undefined })` does not match an object that simply lacks those keys under bun's `toMatchObject` (confirmed by running `bun test`); the two adjacent assertions in the same test (lines 87, 89) already use the working `expect(x.field).toBeUndefined()` form. **Test bug**, not a code bug.
- Convert `frontend/tests/lib/onboarding-summary-model.test.mjs` (hand-rolled `ts.transpileModule` + `vm` harness) to a `bun:test` file matching the other 6, since its assertions are cheap to port 1:1.

## Capabilities

### New Capabilities
- `frontend-test-pipeline`: CI automatically runs the frontend's type check and unit tests on every pull request targeting `main`, and fails the check when either fails.

### Modified Capabilities
<!-- None: online-diarization-telemetry and live-speaker-labels' documented behavior is unchanged — only the two tests were wrong, not the code they exercise (see design.md for the verification against each spec). -->

## Impact

- Code: `frontend/package.json`, `.github/workflows/pr-main-check.yml`, `frontend/tests/lib/diarization-status-lines.test.ts`, `frontend/tests/lib/live-speaker-labels.test.ts`, `frontend/tests/lib/onboarding-summary-model.test.mjs` → `onboarding-summary-model.test.ts`.
- No production `src/` file changes; `src/lib/diarization-status-lines.ts` and `src/lib/live-speaker-labels.ts` are verified correct against `openspec/specs/online-diarization-telemetry/spec.md` and `openspec/specs/live-speaker-labels/spec.md` respectively and are not touched.
- CI: `pr-main-check.yml` gains an automatic trigger and a real check; this is the first frontend gate this repository has had.
- Out of scope: `next lint`'s 268 errors / 37 warnings (change 03) — the new CI lint step is added non-blocking (`continue-on-error: true`) in this change specifically so it doesn't fail every existing PR; change 03 is expected to flip it to blocking once the count reaches the target it defines.
- Out of scope: any `src/` behavior change, `no-explicit-any` (change 09), component splitting (change 09).
