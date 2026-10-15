# Proposal

## Why

`frontend/src` calls Tauri's `invoke(` 188 times and `listen(` 13 times across 27+ files (`frontend/src/lib/analytics.ts` 23, `frontend/src/services/recordingService.ts` 22, `frontend/src/components/ModelSettingsModal.tsx` 16, `frontend/src/lib/whisper.ts` and `frontend/src/lib/parakeet.ts` 13 each, down to single call sites in `frontend/src/hooks/*`). Every call site writes its own command-name string, argument object, and `Result<T, String>` unwrapping by hand; there is no compiler check that a command name still exists on the Rust side or that the argument shape matches. This audit found two calls that are already silently broken this way: `ModelSettingsModal.tsx:320` invokes `'api_get_auto_generate_setting'`, whose registration is commented out at `frontend/src-tauri/src/lib.rs:733`, and `frontend/src/lib/builtin-ai.ts:96` invokes `'builtin_ai_get_models_directory'`, which has no matching `#[tauri::command]`/`#[command]` function anywhere in `frontend/src-tauri/src`. Both failures are caught by ad hoc `try { } catch` blocks and never surface. Splitting the largest call sites in change 09 (`ModelSettingsModal.tsx`, `VoiceprintBrowser.tsx`) will multiply this duplication unless the IPC boundary is centralized first.

## What Changes

- Add `frontend/src/lib/ipc/core.ts` exporting `invokeTyped<TArgs, TResult>(cmd, args)`: a single wrapper over `@tauri-apps/api/core`'s `invoke` that normalizes every failure into one `IpcError` shape (Rust commands return `Result<T, String>`, so today's `String` rejection is wrapped as `IpcError { command, message, cause }`), and an equivalent `listenTyped<TPayload>(event, handler)` over `@tauri-apps/api/event`'s `listen`. Both take an optional debug-logging flag read from a single module-level switch (default off), replacing the scattered `console.log`/`console.error` calls at existing sites.
- Add one module per domain under `frontend/src/lib/ipc/`, each exporting typed async functions (thin wrappers around `invokeTyped`) and typed `listen` helpers for that domain's events: `recording.ts`, `transcript.ts`, `speakers.ts` (speaker + voiceprint commands), `models.ts` (whisper/parakeet/builtin-ai/ollama/openai/anthropic/groq/openrouter model listing and download commands), `summary.ts`, `settings.ts`, `meetings.ts` (meetings + tags), `analytics.ts`.
- Migrate call sites to the new modules in the order: components calling `invoke`/`listen` directly first (`Sidebar/index.tsx` 7, `MeetingTags/TagEditorPopover.tsx` 6, `VoiceprintBrowser.tsx` 5, `ModelSettingsModal.tsx` 16, plus the other 20 component files with 1-9 calls each), then `services/recordingService.ts` (22 invoke + 4 listen), then `lib/analytics.ts` (23), then `lib/whisper.ts` / `lib/parakeet.ts` (13 each), then the remaining `lib/*.ts` and `hooks/*` single/low-count sites. Existing domain wrappers that already exist (`services/configService.ts`, `services/diarizationStatusService.ts`, `services/transcriptService.ts`, `services/alignmentService.ts`) are re-pointed at `lib/ipc/*` internally; their public class/method surface does not change, so callers of those services are unaffected.
- Add an eslint rule forbidding `@tauri-apps/api/core` and `@tauri-apps/api/event` imports outside `frontend/src/lib/ipc/` (`no-restricted-imports` in `frontend/.eslintrc.json`, which today only has `"extends": ["next/core-web-vitals", "next/typescript"]` and no `rules` key), so the boundary is enforced going forward, not just achieved once.
- Payload types for this change are hand-written TypeScript interfaces colocated in each `lib/ipc/*.ts` module (or imported from `frontend/src/types/index.ts` where a matching domain type, e.g. `Transcript`, `Summary`, already exists there) — see design.md for why generation (`tauri-specta`/`ts-rs`) is not adopted now.
- No command is renamed, no argument shape changes, and no Rust code changes. The two dangling calls found above keep failing exactly as they do today (nothing here fixes them); their errors simply become visible through the same normalized `IpcError` path as every other call instead of being swallowed by a local `catch`, which is a debuggability improvement, not a behavior change.

## Capabilities

### New Capabilities

None. `skip_specs: true` — see `.openspec.yaml`.

### Modified Capabilities

None.

## Impact

- Code: new `frontend/src/lib/ipc/{core,recording,transcript,speakers,models,summary,settings,meetings,analytics}.ts`; edits to `frontend/.eslintrc.json`; call-site edits across the 27+ files listed above, none changing observable behavior for a working command.
- Tests: new `frontend/tests/lib/ipc/core.test.ts` (error normalization, mocked `invoke`) and at least one domain module test with a mocked `invoke`, run by `npx bun test tests/` (the `test` script and `bun` devDependency landed in change 02-frontend-test-pipeline-and-regressions, which this change depends on).
- Out of scope: fixing `api_get_auto_generate_setting` / `builtin_ai_get_models_directory` (Rust side unchanged; noted as a follow-up), the Rust-side `#[tauri::command]` registration surface, and any component split (that is change 09-frontend-split-large-components, which is sequenced after this one specifically so it can call the new `lib/ipc/*` modules instead of `invoke` directly).
- Dependency note: 115 `#[tauri::command]`/`#[command]` Rust functions have no current frontend caller (e.g. `api_get_profile`, `list_tags`, `get_recording_telemetry`, `preview_replace_speaker`) — left untouched; the ipc layer only wraps commands that already have a call site today.
