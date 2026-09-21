# Tasks

## 1. Doc-link checker script and CI wiring

- [ ] 1.1 Write `scripts/check-doc-links.sh`: extract every backticked string matching `*.rs`, `*.ts`, `*.tsx`, `*.js`, or `*.jsx` from `docs/*.md` and `AGENTS.md`; for each, check (via `find`) whether a file with that basename exists anywhere under `frontend/src-tauri/src/`, `frontend/src/`, `frontend/`, `scripts/`, or the repo root, excluding `node_modules`, `target`, `.git`, `graphify-out`, `.next`, `dist`, `build`; print each unresolved reference with its source file, and exit `1` if any are found, `0` otherwise; verify by running `bash scripts/check-doc-links.sh` locally against the *current* (pre-cleanup) docs and confirming it reports the known dead reference (`core.rs` in `docs/PROJECT_OVERVIEW_FULL.md`) and does not report the two illustrative examples (`PascalCase.tsx`, `snake_case.rs` in `docs/CODEBASE_MAP_CONVENTIONS.md`) as false negatives being silently accepted — i.e. confirm those two are either excluded by the extraction pattern (they are not real backticked-as-a-reference filenames in prose, e.g. appearing in a sentence like "components use `PascalCase.tsx`") or accepted as a known, documented limitation of a mechanical checker.
- [ ] 1.2 Add a `Check documentation links` step to `.github/workflows/pr-main-check.yml`, after the `Validate version format` step, running `bash scripts/check-doc-links.sh` with `continue-on-error: true`; verify by confirming the YAML parses (`cat .github/workflows/pr-main-check.yml` and checking indentation) and, if `act` or a similar local runner is unavailable, by visual review that the step matches the existing steps' style (`name`, `run`).

## 2. Fix dead references repo-wide

- [ ] 2.1 In `docs/PROJECT_OVERVIEW_FULL.md:103`, remove the dead link to `AUDIO_MODULARIZATION_PLAN.md` (never existed in git history) and rewrite the sentence to describe the current state without naming a single `core.rs` (the audio system has been repeatedly re-split since; point at the architecture overview in `docs/CODEBASE_MAP_ARCHITECTURE.md` instead of a specific file); verify with `grep -rn "AUDIO_MODULARIZATION_PLAN\|core\.rs" docs/PROJECT_OVERVIEW_FULL.md` returning no matches.
- [ ] 2.2 Run `bash scripts/check-doc-links.sh` against `docs/BUILDING.md`, `docs/GPU_ACCELERATION.md`, `docs/architecture.md`, `docs/building_in_linux.md` and fix or remove any reported dead reference in those files; verify the script reports zero unresolved references for these four files.

## 3. Shrink the codebase maps (Recommendation B)

- [ ] 3.1 Delete the 13 superseded files: `docs/CODEBASE_MAP_MODULES.md`, `docs/CODEBASE_MAP_MODULE_AI_PROVIDERS.md`, `docs/CODEBASE_MAP_MODULE_ANALYTICS.md`, `docs/CODEBASE_MAP_MODULE_AUDIO.md`, `docs/CODEBASE_MAP_MODULE_DATABASE.md`, `docs/CODEBASE_MAP_MODULE_FRONTEND_APP.md`, `docs/CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md`, `docs/CODEBASE_MAP_MODULE_FRONTEND_HOOKS.md`, `docs/CODEBASE_MAP_MODULE_NOTIFICATIONS.md`, `docs/CODEBASE_MAP_MODULE_PARAKEET.md`, `docs/CODEBASE_MAP_MODULE_SUMMARY.md`, `docs/CODEBASE_MAP_MODULE_WHISPER.md`, `docs/CODEBASE_MAP_DATA_FLOW.md`, `docs/CODEBASE_MAP_NAVIGATION.md`; verify with `git rm` and `ls docs/CODEBASE_MAP*.md` showing only the 4 remaining files.
- [ ] 3.2 Trim `docs/CODEBASE_MAP.md` to: the system-overview paragraph, the core-capabilities bullet list, and links to only the 4 surviving files plus a pointer to `graphify-out/wiki/index.md` for file/module-level navigation; remove the `sub_files` frontmatter list entries for the 13 deleted files and the per-run `total_files`/`total_tokens`/`last_mapped` frontmatter (these are exactly the fields that go stale); verify by reading the file and confirming no remaining link targets a deleted file.
- [ ] 3.3 Trim `docs/CODEBASE_MAP_ARCHITECTURE.md`: keep the system overview and the Mermaid architecture diagram, remove any "recent work"/dated prose and per-file token-count content; verify by reading the file and confirming it contains no `last_mapped`-relative dated claims ("since 2026-08-...") left over.
- [ ] 3.4 Leave `docs/CODEBASE_MAP_CONVENTIONS.md` largely as-is (naming/pattern conventions are stable) but fix the `PascalCase.tsx`/`snake_case.rs` illustrative examples flagged by the link check if they are phrased ambiguously enough to be mistaken for real file references (e.g. rephrase as "PascalCase filenames like `Button.tsx`" using a real existing file as the example); verify with `bash scripts/check-doc-links.sh` reporting zero issues for this file.
- [ ] 3.5 Extend `docs/CODEBASE_MAP_OPERATIONS.md` with two new entries in its commands table/section: the frontend test command added by change 02 (confirm the exact script name in `frontend/package.json` at implementation time — the plan describes it as a `test` script wrapping `bun test tests/`) and the clippy CI gate added by change 03 (confirm the exact invocation in `.github/workflows/*.yml` at implementation time — the plan describes `cargo clippy -p meetily --all-targets` treated as a gate); verify by reading the file and confirming both commands are present and match what changes 02/03 actually landed (not a guess).

## 4. Rewrite AGENTS.md

- [ ] 4.1 Rewrite `AGENTS.md` lines 5-21 ("Codebase Map" section) to link only to the 4 surviving `CODEBASE_MAP*.md` files plus `graphify-out/wiki/index.md` (once it exists, per task 5); verify by grepping `AGENTS.md` for any of the 13 deleted filenames and confirming zero matches.
- [ ] 4.2 Replace the "Recent additions" paragraph (`AGENTS.md` line 23) with a pointer: "For a changelog of what has shipped, see `openspec/changes/archive/` (browse newest-first)."; verify by reading `AGENTS.md` and confirming the paragraph no longer names individual change slugs or dated feature lists.
- [ ] 4.3 Run `bash scripts/check-doc-links.sh` against the final `AGENTS.md`; verify it reports zero unresolved references.

## 5. Generate and wire in the graphify wiki

- [ ] 5.1 Run `graphify update . --wiki` (or the current equivalent flag per the installed graphify skill) against the repo root; verify `graphify-out/wiki/index.md` exists and is non-empty, and that it covers at minimum the modules removed in task 3.1 (audio, summary/AI providers, database, frontend components/hooks).
- [ ] 5.2 Decide whether `graphify-out/wiki/` is committed to the repo or left generated-on-demand (see design.md Open Questions), and act accordingly (either `git add graphify-out/wiki/` or add a `.gitignore` entry with a comment explaining it's regenerated via `graphify update . --wiki`); verify with `git status` showing the intended outcome.

## 6. Final verification

- [ ] 6.1 Run `bash scripts/check-doc-links.sh` against the full, final `docs/*.md` + `AGENTS.md`; verify it exits `0`.
- [ ] 6.2 Run `openspec validate 11-docs-refresh --strict`; verify it passes.
