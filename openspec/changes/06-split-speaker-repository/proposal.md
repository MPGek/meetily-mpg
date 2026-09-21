# Proposal

## Why

`frontend/src-tauri/src/database/repositories/speaker.rs` is 4664 lines: a 1621-line `impl SpeakerRepository` block (29-1765) covering eight distinct responsibilities, followed by 2898 lines of tests in one `mod tests`. Any change to one responsibility (e.g. voiceprint rejection) requires scrolling past unrelated code (cluster binding, merge, overrides), and review diffs are noisy. The file also hides a real N+1 query: `replace_speaker` (1305-1439) runs one `COUNT(*)` per affected cluster inside a loop (1327-1338) instead of one aggregate query, and `preview_replace_speaker` (1442-1472) repeats the identical pattern.

## What Changes

- Split `speaker.rs` into `database/repositories/speaker/{mod,crud,enrollment,binding,voiceprints,merge,overrides,stats,test_support}.rs`. `SpeakerRepository` remains a single unit struct with one `impl SpeakerRepository` block per file; `mod.rs` owns the struct definition, the shared constants, and re-exports so every existing call path (`crate::database::repositories::speaker::{SpeakerRepository, SpeakerStorageStats, ...}`) is unchanged.
- Move each of the 37 associated functions (35 `pub`, 2 private) to the module matching its responsibility (see design.md for the full mapping); move each `#[tokio::test]` alongside the function(s) it exercises; move the 7 shared test-setup helpers (`setup_pool`, `insert_meeting`, `emb`, `insert_transcript`, `insert_transcript_window`, `insert_prototype_with_clip`, `insert_cache_row`) into `speaker/test_support.rs`.
- Widen the visibility of the one private helper called across the new module boundary (`enforce_prototype_cap`, currently private, called from both `enrollment.rs` and `voiceprints.rs`) to `pub(super)`. No other visibility changes are needed because every other cross-file call target is already `pub`.
- Fix the N+1 in `replace_speaker` (1327-1338) and the identical pattern in `preview_replace_speaker` (1456-1466): replace the per-cluster `COUNT(*)` loop with a single query that groups by `(meeting_id, speaker)` over all affected pairs at once, using a `VALUES` list to keep exact-pair matching (not a broader `IN`/`IN` cross product). Row counts returned to callers are identical; this is an internal query-plan change only.
- Keep the post-commit best-effort re-match loop in `replace_speaker` (1387-1432) exactly where it is (outside the transaction), and add a code comment explaining why: the loop calls other repository methods that take `&SqlitePool` (not a transaction handle), it already tolerates partial failure (`unwrap_or_default()`, `let _ = ...`), and folding it into the transaction would turn a best-effort downstream convenience into something whose failure rolls back a successful `replace_speaker`.
- Fix the 12 clippy "unnecessary use of `clone` to create a slice from a reference" warnings in this file's test code (`std::slice::from_ref(&x.id)` instead of `&[x.id.clone()]`), mechanically, as each affected test moves to its new file. The file's other ~6 clippy warnings (a redundant cast at line 779, 3 doc-indentation warnings at 1010-1012, 2 `useless_vec` warnings at 3461/3477) are out of scope — they belong to `03-lint-baseline-cleanup`.
- No public API, database schema, or SQL result changes.

## Capabilities

### New Capabilities
<!-- None. -->

### Modified Capabilities
<!-- None: no spec-level behavior changes. See `skip_specs: true` in .openspec.yaml. -->

## Impact

- Code: `frontend/src-tauri/src/database/repositories/speaker.rs` → `frontend/src-tauri/src/database/repositories/speaker/{mod,crud,enrollment,binding,voiceprints,merge,overrides,stats,test_support}.rs`. `frontend/src-tauri/src/database/repositories/mod.rs` keeps `pub mod speaker;` unchanged (a directory module resolves the same path).
- Callers unaffected (verified by `grep -rn "SpeakerRepository" frontend/src-tauri/src | grep -v repositories/speaker`): `frontend/src-tauri/src/audio/diarization.rs`, `frontend/src-tauri/src/audio/online_diarization.rs`, `frontend/src-tauri/src/audio/recording_commands.rs`, `frontend/src-tauri/src/database/speaker_commands.rs` all call `SpeakerRepository::method(...)` or import `speaker::{...}` — both keep working unchanged through the `mod.rs` struct definition and re-exports.
- No dependency, migration, or IPC changes.
- Out of scope: the other ~6 non-clone clippy warnings in this file (03's scope); any behavior change to the re-match loop; the `04-recording-lock-hardening` and `05-...-diarization-engine` changes (unrelated files).
