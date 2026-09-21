# Tasks

## 1. Fix the two failing tests

- [ ] 1.1 In `frontend/tests/lib/diarization-status-lines.test.ts:226`, change `alignment({ queued_jobs: 0, requested: false })` to `alignment({ loaded: true, queued_jobs: 0, requested: false })`; verify with `bun test tests/lib/diarization-status-lines.test.ts` passing (19/19) and no change to `src/lib/diarization-status-lines.ts`.
- [ ] 1.2 In `frontend/tests/lib/live-speaker-labels.test.ts:85`, replace `expect(out[1]).toMatchObject({ display_name: undefined, matched_by: undefined });` with `expect(out[1].display_name).toBeUndefined();` followed by `expect(out[1].matched_by).toBeUndefined();`; verify with `bun test tests/lib/live-speaker-labels.test.ts` passing (10/10) and no change to `src/lib/live-speaker-labels.ts`.
- [ ] 1.3 Run `bun test tests/` and verify the full suite reports `50 pass, 0 fail`.

## 2. Convert the last test file to `bun:test`

- [ ] 2.1 Rewrite `frontend/tests/lib/onboarding-summary-model.test.mjs` as `frontend/tests/lib/onboarding-summary-model.test.ts`, importing `describe`/`expect`/`test` from `bun:test` and the four functions (`resolveOnboardingSummaryModelStatus`, `getSummaryModelSizeMb`, `getSummaryModelSizeLabel`, `getDownloadTotalMb`) directly from `../../src/lib/onboarding-summary-model`, porting each of the 8 existing `assert.equal` checks to an `expect(...).toEqual(...)`/`toBe(...)` inside a named `test(...)`; delete the old `.mjs` file; verify with `bun test tests/lib/onboarding-summary-model.test.ts` passing and `bun test tests/` still reporting all tests green.

## 3. Wire up the test script and types

- [ ] 3.1 Add `"test": "bun test tests/"` to `frontend/package.json` `scripts`; verify with `cd frontend && bun run test` passing.
- [ ] 3.2 Add `"@types/bun": "1.4.2"` to `frontend/package.json` `devDependencies`, run `pnpm install` inside `frontend/` so `pnpm-lock.yaml` picks up `@types/bun` and its `bun-types` dependency; verify with `npx tsc --noEmit -p .` reporting zero errors (currently 5× `TS2307`).

## 4. Wire up CI

- [ ] 4.1 In `.github/workflows/pr-main-check.yml`, add a `pull_request: branches: [main]` trigger alongside the existing `workflow_dispatch:` under `on:`; verify by confirming the YAML parses (`gh workflow view "Validation Check" --yaml` or a local YAML lint) and both trigger keys are present.
- [ ] 4.2 Add a new `frontend-checks` job to `.github/workflows/pr-main-check.yml`: checkout, `oven-sh/setup-bun@v2` (version `1.4.2`), `actions/setup-node` + `pnpm/action-setup` (matching the pattern already used in `build.yml`'s frontend steps), `cd frontend && pnpm install`, then three steps: `pnpm exec tsc --noEmit -p .`, `pnpm exec next lint` (with `continue-on-error: true` per design.md D4), and `bun test tests/`; verify by pushing to a branch and confirming the job appears and runs on a test PR, or by validating the workflow YAML locally.
- [ ] 4.3 Confirm the job fails when a test fails: temporarily reintroduce the bug from task 1.1 in a scratch branch, push, observe the `frontend-checks` job go red, then revert the scratch change; verify by observing the CI run status (not merged).

## 5. Final verification

- [ ] 5.1 Run `cd frontend && npx tsc --noEmit -p .`, `bun test tests/`, and `npx next lint` (informational only, not a gate for this change); verify tsc is clean, tests are 50/50 green, and note the lint count for change 03's baseline.
- [ ] 5.2 Run `openspec validate 02-frontend-test-pipeline-and-regressions --strict`; verify it passes.
