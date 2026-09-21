# Design

## Context

See `proposal.md` for motivation and the full call-site inventory. Current state that shapes the approach:

- `frontend/src-tauri/src/lib.rs:611` registers every command in one `generate_handler!` call. Two attribute spellings are used for command functions: `#[tauri::command]` (used in most files) and the shorthand `#[command]` after `use tauri::command;` (used in `analytics/commands.rs`, `anthropic/anthropic.rs`, `audio/system_audio_commands.rs`, `audio/transcription/commands.rs`, `audio/word_alignment/commands.rs`, `groq/groq.rs`, `ollama/ollama.rs`, `openai/openai.rs`, `openrouter/openrouter.rs`, `parakeet_engine/commands.rs`, `whisper_engine/commands.rs` — 73 of the 251 total command functions). Any future audit of the Rust side must grep both spellings; the original brief's `grep "#\[tauri::command\]"` alone undercounts by 73.
- All commands return `Result<T, String>` (`map_err` to a plain string ad hoc per call site), so there is exactly one error shape to normalize on the frontend: a rejected promise whose reason is a `string`.
- Domain wrappers already exist and already compile cleanly against the current commands: `frontend/src/services/configService.ts` (model/transcript config, custom OpenAI config, recording preferences), `frontend/src/services/diarizationStatusService.ts` (telemetry snapshot types), `frontend/src/services/transcriptService.ts`, `frontend/src/services/alignmentService.ts`. None of them share an `invoke` wrapper or error type; each calls `invoke<T>(...)` from `@tauri-apps/api/core` directly and lets the rejection propagate as-is.
- `frontend/src/types/index.ts` holds domain model types (`Transcript`, `Summary`, `SummaryDataResponse`, `MeetingMetadata`, ...) that are the right return types for several commands (e.g. `api_get_summary` should return `Summary`-shaped data); it is not itself an IPC-payload module and this change does not restructure it, only imports from it where a type already matches.
- No `tauri-specta` or `ts-rs` dependency exists in `frontend/src-tauri/Cargo.toml` or the workspace root `Cargo.toml` (checked; absent). Adopting either now would mean introducing a Rust build-time codegen step as a side effect of a frontend-only change, and 173+ commands would need annotating before generation covers the surface this change migrates — out of proportion to the problem being solved here.
- `frontend/.eslintrc.json` is `{"extends": ["next/core-web-vitals", "next/typescript"]}` — no `rules` key today, so adding `no-restricted-imports` is a pure addition, not an override.
- `frontend/tests/lib/*.test.ts` already establishes the bun test convention (`import { describe, expect, test } from "bun:test"`, files under `frontend/tests/lib/`, importing from `../../src/lib/...`); this change's tests follow that convention and rely on the `test` script / `bun` devDependency landing in 02-frontend-test-pipeline-and-regressions.

## Goals / Non-Goals

**Goals:**

- One place (`lib/ipc/core.ts`) that turns every `Result<T, String>` rejection into one `IpcError` shape, so every call site can handle errors the same way instead of re-deriving `err instanceof Error ? err.message : String(err)` locally.
- One typed module per domain, so a command's argument and return types are declared once and reused by every caller, and command-name strings appear in exactly one place per command.
- Enforce the boundary with a lint rule, not just a convention, so a future PR cannot reintroduce a direct `invoke` call outside `lib/ipc/`.
- Zero behavior change for every command that currently works; the two already-broken calls keep failing (see Risks).
- Make the migration incremental and buildable at every step (no big-bang rewrite): each file's call sites move to `lib/ipc/*` in one self-contained task, verified by `tsc` + the file's existing behavior.

**Non-Goals:**

- Generating types from Rust (`tauri-specta`/`ts-rs`) — deferred, see Decisions.
- Fixing `api_get_auto_generate_setting` or adding `builtin_ai_get_models_directory` on the Rust side.
- Changing any command's argument shape, return shape, or the commands registered in `generate_handler!`.
- Splitting `ModelSettingsModal.tsx` or `VoiceprintBrowser.tsx` (change 09).
- Removing the 115 Rust commands that have no current frontend caller.

## Decisions

### D1: A single generic `invokeTyped<TArgs, TResult>(cmd: string, args?: TArgs, opts?: { debug?: boolean }): Promise<TResult>`

Implementation: call `invoke<TResult>(cmd, args as InvokeArgs)` from `@tauri-apps/api/core`; on rejection, construct `new IpcError(cmd, normalizeMessage(reason))` and throw it; on success, optionally log `[ipc] <cmd> ok` when the module-level debug switch is on.

- `IpcError extends Error` carries `command: string` and `cause: unknown` (the original rejection value), so `instanceof Error` checks already scattered through the codebase (e.g. `useSummaryGeneration.ts:373` `error instanceof Error ? error.message : 'Unknown error'`) keep working unchanged after migration.
- `normalizeMessage(reason: unknown): string` handles the actual shapes seen today: a plain `string` (the common case, since Rust returns `Result<T, String>`), an `Error`, and a fallback `String(reason)` for anything else (e.g. a rejected `AbortController` signal) — this is the function change 08's error-normalization tests target directly.
- Alternative considered: return a `Result<T, IpcError>`-style discriminated union instead of throwing. Rejected because every existing call site already uses `try/catch` or `.catch(...)`; switching to a returned-error style would touch the control flow of all 188 call sites instead of just the command name and types, multiplying the size of this change for no behavior gain.

### D2: `listenTyped<TPayload>(event: string, handler: (payload: TPayload) => void): Promise<UnlistenFn>`

Thin wrapper over `@tauri-apps/api/event`'s `listen<TPayload>`, typed per event name in the owning domain module (e.g. `listenTranscriptionComplete`, `listenRecordingStarted`). No behavior change: `listen` already returns a `Promise<UnlistenFn>` and every current call site already awaits it and stores the unlisten function.

- The 13 current `listen(` call sites split across `app/layout.tsx` (3, plus 2 non-domain Tauri drag events which are left as direct `@tauri-apps/api/event` calls — see Non-Goals-adjacent note below), `BuiltInModelManager.tsx`, `DeviceSelection.tsx`, `MeetingDetails/RetranscribeDialog.tsx`, `RecordingControls.tsx` (3), `hooks/useImportAudio.ts`, `hooks/useRecordingStop.ts`, and `services/recordingService.ts` / `services/transcriptService.ts` (4 combined, already behind a service).
- `app/layout.tsx:111` (`request-recording-toggle`) and `:168`/`:180` (`tauri://drag-enter`/`tauri://drag-leave`) are app-shell-level, not domain events; they stay on the raw `listen` import in `app/layout.tsx`, which is added to the eslint rule's allow-list alongside `lib/ipc/` (see D4) rather than forcing a one-off `lib/ipc/shell.ts` for two generic window events.

### D3: Hand-written TypeScript types now; revisit generation later

Types are written by hand in each `lib/ipc/*.ts` module, matching the current Rust `Result<T, String>` signatures read from source (e.g. `frontend/src-tauri/src/ollama/ollama.rs:91` `get_ollama_models(endpoint: Option<String>) -> Result<Vec<OllamaModel>, String>` becomes `getOllamaModels(endpoint?: string): Promise<OllamaModel[]>` with `OllamaModel` hand-typed to match the Rust struct's `#[derive(Serialize)]` fields).

- Why not `tauri-specta`/`ts-rs` now: neither is a dependency today (checked both Cargo.toml files); adopting one means annotating some subset of 251 command functions with a derive/macro before any type is generated, plus a new build step that runs before `tsc`/`next build`. That is a larger, separate, Rust-side change with its own risk (touching every command file) — reasonable to propose on its own once the frontend-side boundary this change builds exists to receive generated types.
- Why hand-written is acceptable now: this change's whole point is to move 188 existing, already-working call sites behind one boundary without changing their argument/return shapes — the types are a transcription of what the call sites already assume, not new design.
- Recommendation for later: once `lib/ipc/*` is the only place commands are called from, introducing `tauri-specta` becomes a strictly additive change (swap hand-written interfaces for generated ones, module by module) instead of touching 27+ call-site files again.

### D4: `no-restricted-imports` on `@tauri-apps/api/core` and `@tauri-apps/api/event`, scoped to `frontend/src/lib/ipc/`

Add to `frontend/.eslintrc.json`:

```json
{
  "extends": ["next/core-web-vitals", "next/typescript"],
  "rules": {
    "no-restricted-imports": [
      "error",
      {
        "paths": [
          { "name": "@tauri-apps/api/core", "message": "Import from '@/lib/ipc/...' instead of calling invoke() directly." },
          { "name": "@tauri-apps/api/event", "message": "Import from '@/lib/ipc/...' instead of calling listen() directly." }
        ]
      }
    ]
  },
  "overrides": [
    {
      "files": ["src/lib/ipc/**/*.ts", "src/app/layout.tsx"],
      "rules": { "no-restricted-imports": "off" }
    }
  ]
}
```

- `src/app/layout.tsx` is in the override allow-list per D2 (two generic Tauri window events plus one app-level custom event, none domain-specific enough to justify their own `lib/ipc` module).
- Alternative considered: a custom eslint rule keyed on directory depth instead of `no-restricted-imports`' `paths`+`overrides`. Rejected — `no-restricted-imports` is a core ESLint rule already available with the installed `eslint@8.57.1`, needs no new dependency, and the `overrides` block is the standard way to scope an eslintrc rule to a subtree.

### D5: Migration order and unit of work

One file (or one tightly-coupled pair, e.g. a component and the hook it inlines `invoke` calls into) per task, in the order given in the proposal: highest-invoke-count components first, then services, then `lib/*.ts`. Each task only changes import statements and call expressions (`invoke('cmd', args)` → `recordingIpc.startRecording(args)`), not surrounding logic.

- Why components before services: change 09 (sequenced immediately after this one) rewrites `ModelSettingsModal.tsx` and `VoiceprintBrowser.tsx`; doing their IPC migration here means 09 starts from components that already import `lib/ipc/*`, so splitting them into `hooks/useModelSettings.ts` / `hooks/useVoiceprintActions.ts` is a pure extraction, not an extraction-plus-retyping.
- Why `lib/analytics.ts` and `lib/whisper.ts`/`lib/parakeet.ts` last: they are already single-purpose files with a consistent internal style (static-method classes), so wrapping their existing `invoke` calls is the most mechanical, lowest-risk step and benefits least from being done early.

## Risks / Trade-offs

- **The two dangling calls stay broken** (`api_get_auto_generate_setting`, `builtin_ai_get_models_directory`) → Accepted: fixing either is a Rust-side or product decision outside this change's scope; flagged here and in the proposal so the owner can pick it up as a follow-up. Migrating them still normalizes their (unchanged) failure through `IpcError` instead of a bespoke `catch`.
- **`no-restricted-imports` could block an unrelated future PR that needs a one-off Tauri API call** (e.g. `@tauri-apps/api/path`, which is not restricted, or a genuinely new domain) → Mitigation: the rule only names `@tauri-apps/api/core` and `@tauri-apps/api/event`; a new domain gets a new `lib/ipc/<domain>.ts` file rather than an exception, which is the intended path.
- **Hand-written types can drift from the Rust struct if a field is renamed on one side only** → Same risk that exists today (every current call site already hand-assumes the shape); unchanged by this migration. Flagged as the reason to eventually adopt generation (D3), not fixed here.
- **Large mechanical diff across 27+ files raises review-conflict risk with in-flight changes on `feat/diarization`** (`add-online-diarization-eval`, `live-word-level-diarization`, `tags-persistence-and-palette`) → Mitigation: migration is ordered file-by-file (D5) so it can land in small PRs, and files owned by an in-flight change (e.g. anything `live-word-level-diarization` is actively editing) can be migrated last or skipped and picked up after that change merges.

## Migration Plan

- Purely additive/mechanical: `lib/ipc/*` is added first (task group 1), then call sites move over file-by-file (task groups 2+), each independently buildable and revertible.
- No data migration, no IPC contract change, no user-visible change for a working command.
- Rollback: revert the call-site commits for the affected files; `lib/ipc/*` and the eslint rule can be left in place or reverted together since nothing else depends on them until call sites are migrated.

## Open Questions

- Should the two dangling commands be fixed (add `api_get_auto_generate_setting` back to `generate_handler!` and implement it, or implement `builtin_ai_get_models_directory`) in a small follow-up change, or is the current (broken, silently-caught) behavior intentional/dead? `builtin_ai_get_models_directory` has no caller of its own caller (`BuiltInAI.getModelsDirectory()` in `lib/builtin-ai.ts` is itself unused in `frontend/src`), so it may be safe to delete instead of fix.
