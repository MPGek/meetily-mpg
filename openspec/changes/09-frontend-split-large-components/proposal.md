# Proposal

## Why

`frontend/src/components/ModelSettingsModal.tsx` (1408 lines: ~40 `useState`/`useRef` declarations at lines 126-176, 15 `useEffect` hooks at 190-771, `handleSave` at 617, `handleInputClick` at 694, and 604 lines of JSX from 804-1408) and `frontend/src/components/VoiceprintBrowser.tsx` (1000 lines: 4 inline dialog components at 70-335, ~15 `useState` at 337-482, handlers at 422-620, render at 649-1000) mix state, IPC calls, and deeply nested JSX in single files, which makes either file risky to touch and impossible to unit test. Change 08 (frontend-typed-ipc-layer, sequenced immediately before this one) moves their direct `invoke` calls behind `frontend/src/lib/ipc/*`; this change extracts the state/orchestration into hooks and the JSX into focused components so each piece becomes independently readable and, where it is pure, testable. Separately, `hooks/` carries 15 `: any` and 17 `as any` sites (32 total, verified) that defeat TS strict mode at exactly the layer that owns IPC payloads, and `contexts/TranscriptContext.tsx` (870 lines) and `hooks/meeting-details/useSummaryGeneration.ts` (672 lines) have zero test coverage despite each containing pure, testable logic today.

## What Changes

- Split `ModelSettingsModal.tsx` into `hooks/useModelSettings.ts` (state + IPC, calling the change-08 `lib/ipc/settings.ts`/`lib/ipc/models.ts` modules) and `components/ModelSettings/{index.tsx, ApiKeyField.tsx, providers/{HostedProviderFields.tsx, Ollama.tsx, CustomOpenAI.tsx, BuiltIn.tsx}}`. The 7-provider `Select` (`builtin-ai`, `claude`, `custom-openai`, `groq`, `ollama`, `openai`, `openrouter`) and its handler stay in `index.tsx`; `openai`/`claude`/`groq`/`openrouter` share one `HostedProviderFields.tsx` because the current JSX already shares one model combobox and one `ApiKeyField` block (gated by `requiresApiKey`, `ModelSettingsModal.tsx:237-240,1072`) across those four providers — see design.md for why this deviates from a strict one-file-per-provider split.
- Split `VoiceprintBrowser.tsx`'s 4 inline dialogs (`PersonPickerDialog` 70-201, `ConfirmReplaceDialog` 203-250, `ConfirmClearAllDialog` 251-288, `ConfirmPurgeCachesDialog` 289-335) into `components/Voiceprints/dialogs/*.tsx`, and its handlers (`handlePlay` 422, `handleReject` 467, `handleVerifyRow`/`handleVerifySpeaker`/`handleVerifyMeeting` 484-528, `handleReconfirmPick`/`handleReconfirmCreate` 530-551, `handleReplacePick`/`handleReplaceSelect`/`handleReplaceCreate`/`handleReplaceAnonymous` 552-604, `handleClearAllConfirm`/`handlePurgeCachesConfirm` 605-620, plus the `load`/state at 336-421) into `hooks/useVoiceprintActions.ts`.
- Remove all 32 `any` sites in `frontend/src/hooks/` (15 `: any`, 17 `as any` — see design.md for the full file list), typing them against the change-08 `lib/ipc/*` return types instead. `@typescript-eslint/no-explicit-any` is **already** `"error"` for this project (inherited from the `next/typescript` ESLint preset that `frontend/.eslintrc.json` extends — confirmed by running `next lint`, which already reports every current `any` as an `Error`, not a warning); no `.eslintrc.json` change is needed to raise its severity. What is missing, and stays missing after this change, is CI enforcement: no `.github/workflows/*.yml` runs `next lint`, so today's 268 lint errors (91 of them `no-explicit-any`, none in `hooks/`) never fail a build. This change fixes only the `hooks/` sites; the remaining `any` in `components/`/`app/` (the majority) is listed as follow-up, not fixed here.
- Extract two pure helpers out of `contexts/TranscriptContext.tsx` into `frontend/src/lib/transcript-formatting.ts`: the dedupe-and-sort step inside `addTranscript`'s `setTranscripts` updater (`TranscriptContext.tsx:675-686`) and the `[MM:SS]`-prefixed clipboard formatter inside `copyTranscript` (`TranscriptContext.tsx:702-712`). Add their first bun tests. The rest of `TranscriptContext.tsx` already delegates its other non-trivial logic to `frontend/src/lib/live-speaker-labels.ts`, which is tested (with 1 pre-existing failing test tracked in change 02); this change does not re-touch that module.
- Extract two pure helpers out of `hooks/meeting-details/useSummaryGeneration.ts` into `frontend/src/lib/summary-formatting.ts`: `isLegacySummaryEmpty` (the `allEmpty` check at `useSummaryGeneration.ts:300`) and `formatLegacySummaryData` (the section-formatting loop at `useSummaryGeneration.ts:317-349`). Add their first bun tests.
- Not breaking: `ModelSettingsModal` and `VoiceprintBrowser` keep their current external props/exports (`ModelSettingsModalProps`, default export of `VoiceprintBrowser`), so no caller (`app/meeting-details/page.tsx`, `SummaryModelSettings.tsx`, etc.) changes; only their internal file layout changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None.

`skip_specs: true` — this is a structural refactor (file/module boundaries, `any` removal, first unit tests) with no change to what the settings modal, voiceprint browser, transcript context, or summary generation do from a user's perspective. See design.md Risks for the one behavior-adjacent item found (`autoGenerateEnabled` default-on-error, unchanged by this split) and .openspec.yaml.

## Impact

- Code: new `frontend/src/hooks/useModelSettings.ts`, `frontend/src/hooks/useVoiceprintActions.ts`, `frontend/src/components/ModelSettings/**`, `frontend/src/components/Voiceprints/dialogs/**`, `frontend/src/lib/transcript-formatting.ts`, `frontend/src/lib/summary-formatting.ts`; edits to the 8 `hooks/` files with `any`; `ModelSettingsModal.tsx` and `VoiceprintBrowser.tsx` become thin composition files re-exporting/rendering the new pieces.
- Tests: new `frontend/tests/lib/transcript-formatting.test.ts` and `frontend/tests/lib/summary-formatting.test.ts` using `bun:test` (pure-function tests, no DOM/React rendering — see design.md for why this is chosen over `@testing-library/react` + `happy-dom`, neither of which is a current devDependency).
- Explicitly out of scope: `frontend/src/components/VirtualizedTranscriptView.tsx` (1083 lines) — it has uncommitted work in progress belonging to the in-flight `drop-turn-grouping` change; do not touch it here.
- Depends on 08-frontend-typed-ipc-layer landing first: `useModelSettings.ts` and `useVoiceprintActions.ts` are written to call `lib/ipc/settings.ts`/`lib/ipc/models.ts`/`lib/ipc/speakers.ts`, not `invoke` directly.
