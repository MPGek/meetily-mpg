# Tasks

Commands run from the repo root unless they say `(frontend/)`. "Prescribed skips" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`. Each numbered group 1-3 is one commit and lands its own tests; group 4 only re-runs integration checks.

## 0. Preconditions and baseline

- [x] 0.1 Record the baseline before any edit:
  - `git rev-parse HEAD` (the "group-0 base")
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^warning"` (expect `32` per `docs/CODEBASE_MAP_OPERATIONS.md`)
  - the prescribed-skips test summary line
  - `bun test tests/` pass count (frontend/)
  - `pnpm exec tsc --noEmit -p .` (frontend/), expected clean

  verify: all five results are written into this task's note.
  - Note (2026-10-02): group-0 base `b1acbf1`; clippy 32 warnings; Rust 529 passed / 0 failed / 9 ignored; `bun test tests/` 96 pass; tsc clean; `next lint` 42 findings in 20 files (pre-existing; `OnboardingContext.tsx` is among them).
- [x] 0.2 Re-check the live facts design.md relies on, which were verified 2026-10-02:
  - `curl -s https://huggingface.co/api/models/istupakov/parakeet-tdt-0.6b-v2-onnx | grep -o '"sha":"[0-9a-f]*"'` equals `0bbb45a3365852604aef28b538a8f066f4ccaa85`
  - `curl -sI https://meetily.towardsgeneralintelligence.com/models/parakeet-tdt-0.6b-v3-onnx/<file>` `Content-Length` equals upstream's `PARAKEET_V3_ARTIFACTS` for all 4 files (652183999 / 18202004 / 139764 / 93939)
  - the alignment repo tree at `2d48b01b6429d9018f81914550565112d56f6ba7` lists the six sizes in `audio/word_alignment/catalog.rs:60-65`

  verify: every value matches; if any differs, stop and update design.md D3/D5 before coding.
  - Note (2026-10-02): all match: v2 sha `0bbb45a…`; v3 Content-Length 652183999 / 18202004 / 139764 / 93939; alignment tree at `2d48b01…` gives model_fp16.onnx 651760843, config.json 2280, vocab.json 146914, preprocessor_config.json 214, special_tokens_map.json 96, tokenizer_config.json 1132.
- [x] 0.3 Re-confirm the fork-only Parakeet code to preserve: `git log --oneline 0281737..HEAD -- frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs` lists `82fe7c5` (`transcribe_audio_with_tokens`) and `fe437a1` (`catch_unwind`); `grep -n "fn transcribe_audio_with_tokens\|catch_unwind" frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs` finds `:495` and `:515`. verify: both commits and both lines are present; note the current line numbers if they moved.
  - Note: both commits present; lines unchanged (`:495`, `:515`).

## 1. Parakeet download hardening (upstream #682 + #749) and the shared `model_download` module

- [x] 1.1 Create `frontend/src-tauri/src/model_download/{mod.rs,owners.rs}` and add `pub mod model_download;` to `frontend/src-tauri/src/lib.rs` (next to `pub mod parakeet_engine;` at `:49`).
  - `owners.rs` contains `DownloadOwner` (with `progress: AtomicU8`), `DownloadOwners` (`reserve`, `lock` → guard with `is_owner`/`contains`/`owner`/`release`/`revision`, `cancel_with_timeout`), `DownloadCancelled`, `is_download_cancelled`, `CancelDownloadOutcome` (lowercase serde) and `CANCEL_DOWNLOAD_CLEANUP_TIMEOUT` (5 s), lifted from upstream v0.4.1 `parakeet_engine.rs:147-173,656-672,1196-1229` per design D1.
  - The `mod.rs` doc comment names those upstream line ranges.
  - Add unit tests: `reserve_rejects_a_second_owner`, `cancel_without_owner_reports_cancelled`, `cancel_times_out_to_pending_and_keeps_owner`, `release_bumps_revision_and_signals_completion`.

  verify: `cargo test -p meetily --lib model_download::owners` passes 4 tests.
- [x] 1.2 Add `frontend/src-tauri/src/model_download/transfer.rs`. It holds:
  - `ArtifactSpec { remote, local, exact_bytes }` with `const fn same(name, bytes)`
  - `parse_content_range` and `validate_full_response` / `validate_partial_response` / `validate_unsatisfied_response` (upstream `:190-230,712-810`)
  - `TransferProgress`
  - `download_artifacts(client, base_url, dir, artifacts, &DownloadOwner, on_progress)`, a port of upstream `:849-1131`. It must: skip only on an exact size match; send `Range` only when `0 < local < exact`; validate every `206`/`200`/`416`; reject overlong bodies; use `tokio::select! { biased; cancelled, timeout(30s, next) }`; flush the writer before returning an error or `DownloadCancelled`; cap in-flight percent at 99; set `owner.set_progress`.
  - `#[cfg(test)] pub(crate) mod test_server` with upstream's `ExpectedResponse`/`response`/`read_request`/`serve_requests` (`:1257-1342`)
  - unit tests `content_range_parses_range_and_unsatisfied_forms` and `content_range_rejects_malformed_and_inverted`

  verify: `cargo test -p meetily --lib model_download::transfer` passes 2 tests.
  - Note (2026-10-02): `on_progress` is `&mut (dyn FnMut(TransferProgress) + Send)` instead of `&(dyn Fn + Send + Sync)`: the engines' callbacks are `Box<dyn Fn + Send>` (not `Sync`), and a shared reference held across `.await` made the Tauri command future non-`Send`. The transfer also logs every request (`Requesting <url> (Range: bytes=N-)`) and every skip/resume for the manual checks, and keeps the fork's friendly stream-error prefixes.
- [x] 1.3 In `frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs`, replace the catalog and validation:
  - Add upstream's `ModelSpec`, `PARAKEET_V3_ARTIFACTS`, `PARAKEET_V2_ARTIFACTS` (using `ArtifactSpec::same`), `PARAKEET_MODEL_SPECS` (with the v2 URL pinned at `resolve/0bbb45a3365852604aef28b538a8f066f4ccaa85`) and `find_model_spec`.
  - Replace `discover_models` (`:174-278`) with `discover_models` → `discover_models_from_specs`, which does the revision retry and overlays `Downloading { progress: owner.progress() }` (design D2).
  - Replace `validate_model_directory` (`:281-345`) with the exact-size `fn validate_model_directory(dir, artifacts)`.
  - Delete `clean_incomplete_model_directory` (`:347-398`) and its call (`:722-727`), the approximate size table (`:740-780`), the 0.99 tolerances (`:832-842`, `:884-892`) and the FP32 file lists.

  verify: `cargo check -p meetily`, and `grep -n "clean_incomplete_model_directory\|0\.99\|resolve/main" frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs` returns nothing.
- [x] 1.4 In the same file, replace `cancel_download_flag`/`active_downloads` (`:123-125`, `:167-169`) with `downloads: DownloadOwners`. Then:
  - Rewrite `download_model_detailed` (`:631-1195`) as upstream's `download_model_detailed` → `download_model_detailed_from_source` (reserve owner, then `transfer::download_artifacts`, then `finish_download`).
  - Port `finish_download` (upstream `:1133-1193`): exact re-validation, then the owners lock, then the `available_models` write lock, then the owner check; set `Available`/`Missing`; `release`; `signal_done()`; the final `completed` progress only after commit.
  - Replace `cancel_download` (`:1197-1237`) with `cancel_download` → `cancel_download_with_timeout(.., CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)` returning `CancelDownloadOutcome`, with no sleep and no file removal.
  - Keep the `download_model` wrapper (`:615-628`) and `delete_model` (`:564-612`).
  - Re-export `CancelDownloadOutcome` and `is_download_cancelled` from `parakeet_engine/mod.rs:28-30` (which re-exports them from `model_download`).

  verify: `cargo check -p meetily`, and `grep -n "cancel_download_flag\|sleep(" frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs` returns nothing.
- [x] 1.5 Serialize the model lifecycle (design D3):
  - Add `model_lifecycle_lock: tokio::sync::Mutex<()>`.
  - `load_model` (`:401-459`) clones `ModelInfo`, drops the catalog read guard, takes the lifecycle lock, unloads through `unload_model_locked`, and builds `ParakeetModel::new` in `tokio::task::spawn_blocking`.
  - `unload_model` (`:462-473`) takes the lifecycle lock.
  - Add the `#[cfg(test)]` `ModelLifecycleTestHook` and `DownloadStateTestHook` fields and hook points from upstream `:175-188,321-329,378-382,463-472,515-518,1147-1151`.
  - Leave `transcribe_audio`, `transcribe_audio_with_tokens` and its `catch_unwind` block (`:486-556`) unchanged.

  verify: `cargo check -p meetily`, and `git diff -U0 HEAD -- frontend/src-tauri/src/parakeet_engine/parakeet_engine.rs | grep -n "transcribe_audio_with_tokens\|catch_unwind"` shows no removed (`-`) lines.
- [x] 1.6 Confirm `spawn_blocking` compiles with the fork's `ParakeetModel` (`parakeet_engine/model.rs`). If `ParakeetModel` is not `Send`, use the design Risks fallback: keep `ParakeetModel::new` inline after dropping the catalog guard and under the lifecycle lock. Record which path was taken in this task's note. verify: `cargo check -p meetily` succeeds with the chosen path.
  - Note (2026-10-02): `spawn_blocking` path; the fork's `ParakeetModel` is `Send`, no fallback needed.
- [x] 1.7 In `frontend/src-tauri/src/parakeet_engine/commands.rs`:
  - `parakeet_download_model` (`:388-476`): add an `Err(e) if is_download_cancelled(&e)` arm returning `Ok(())` and emitting `parakeet-model-download-progress {modelName, progress: 0, status: "cancelled"}`, with no error event.
  - `parakeet_cancel_download` (`:479-509`): drop its `app_handle` parameter and its emit, and return `Result<CancelDownloadOutcome, String>`.
  - `parakeet_retry_download` (`:512-558`): delete the force-reset block (`:524-550`) and call `parakeet_download_model`.

  verify: `cargo check -p meetily`, `grep -n "active_downloads\|ModelStatus::Missing" frontend/src-tauri/src/parakeet_engine/commands.rs` returns nothing, and `lib.rs:666-667` still registers both commands unchanged.
- [x] 1.8 Port upstream's 10 Parakeet tests (v0.4.1 `parakeet_engine.rs:1232-2016`) into `parakeet_engine.rs` `#[cfg(test)] mod tests`, using the names listed in design D8.
  - Adapt them to `downloads`/`ArtifactSpec::same`/`model_download::transfer::test_server`.
  - Add `cancelled_download_leaves_partial_files_on_disk`, which asserts the seeded encoder prefix is still on disk after `cancel_download` returns `Cancelled`.

  verify: `cargo test -p meetily --lib parakeet_engine::parakeet_engine::tests` passes 11 tests, and runs offline (loopback only).
  - Note: 11 pass, loopback only. A directory with missing files now reports Corrupted rather than Missing (upstream behavior, design D3).
- [x] 1.9 Fix the frontend `ModelStatus` shape (design D6). In `frontend/src/lib/ipc/models.ts:50-64`:
  - set `{ Downloading: { progress: number } }`
  - delete the "Known drift" paragraph (`:54-57`)
  - add `export type CancelDownloadOutcome = 'cancelled' | 'pending'`
  - make `parakeetCancelDownload` (`:225-227`) return `Promise<CancelDownloadOutcome>`

  Update every reader and writer of the old shape:
  - `ParakeetModelManager.tsx:106,260,449-450`
  - `WhisperModelManager.tsx:103,154,305,524-525`
  - `ModelDownloadProgress.tsx:15` (unused component; type-correctness only, not deleted)

  Change `ParakeetAPI.cancelDownload` (`frontend/src/lib/parakeet.ts:184-186`) to return the outcome.

  verify: `pnpm exec tsc --noEmit -p .` (frontend/) is clean, `grep -rn "{ Downloading: progress }\|{ Downloading: 0 }" frontend/src` returns nothing, and every hit of `grep -rn "status.Downloading" frontend/src` reads `.Downloading.progress`.
- [x] 1.10 Port the #749 Parakeet UI (design D7):
  - In `ParakeetModelManager.tsx`, add `cancellingModels`, `clearCancellingModel`, the `listenersReady` gate and `latestStatusByModelRef`. The progress listener (`:88-111`) handles `status === 'cancelled'` (→ Missing, clear sets, info toast) and `status === 'completed'` (→ Available). `cancelDownload` (`:212-246`) only marks cancelling and toasts on `pending`. `downloadModel` (`:248-285`) returns early while cancelling. `ModelCard` gets `isCancelling`, with download/retry/re-download disabled.
  - In `OnboardingContext.tsx:251-265`, `DownloadProgressStep.tsx:208-224` and `DownloadProgressToast.tsx:228-250`, drop `progress >= 100` for Parakeet only (leave the summary-model listeners at `OnboardingContext.tsx:308`, `DownloadProgressStep.tsx:273` and `DownloadProgressToast.tsx:311` alone).
  - Also in `OnboardingContext.tsx` and `DownloadProgressStep.tsx`, handle `cancelled` as in upstream `d1f7a11`.

  verify: `pnpm exec tsc --noEmit -p .` and `pnpm exec next lint` (frontend/) report no new findings in these files, and `grep -n "progress >= 100" frontend/src/contexts/OnboardingContext.tsx frontend/src/components/onboarding/steps/DownloadProgressStep.tsx frontend/src/components/shared/DownloadProgressToast.tsx` lists only the three summary-model lines.
- [x] 1.11 Add `frontend/tests/lib/ipc/models.test.ts`, following `tests/lib/ipc/analytics.test.ts` and mocking both `@tauri-apps/api/core` and `@tauri-apps/api/event`. The test `parakeetCancelDownload passes the cancelled/pending outcome through` asserts that `invoke('parakeet_cancel_download', { modelName })` is called and both outcomes resolve unchanged. verify: `bun test tests/lib/ipc/models.test.ts` (frontend/) passes.
- [x] 1.12 Document the protocol: add one bullet under the async-patterns list in `docs/CODEBASE_MAP_ARCHITECTURE.md` (next to `:299`, "Summary cancellation uses `CancellationToken`"). It says that model downloads (Parakeet/Whisper/alignment) share `model_download::DownloadOwners` (per-model owner, `cancelled`/`pending` cancel, owner released only after cleanup) and that Parakeet/alignment use the exact-size resumable `model_download::transfer`. verify: `bash scripts/check-doc-links.sh` passes.
- [ ] 1.13 Manual Windows check, network loss after the encoder completes.
  1. Start with `RUST_LOG=info` via `frontend\dev-gpu.bat`, after deleting `%APPDATA%\com.meetily.ai\models\parakeet\parakeet-tdt-0.6b-v3-int8\`.
  2. Download v3 Int8.
  3. Disable Wi-Fi or Ethernet as soon as `encoder-model.int8.onnx` reaches 652183999 bytes (`Get-Item` in a second shell), and wait for the error.
  4. Re-enable the network and click Retry.

  verify: the encoder's `LastWriteTime` and size are unchanged after the retry completes, the console log shows no GET for `encoder-model.int8.onnx` on the retry, the model loads, and a short recording transcribes.
  - Note (2026-10-02): open; needs the desktop app on Windows. Left for the manual-check pass.
- [ ] 1.14 Manual Windows check, cancel near the end.
  1. Start a fresh v3 download.
  2. Click Cancel at ≥ 97% overall.
  3. Confirm the cancel toast and the Missing state.
  4. Click Download again.

  verify: the console log shows a `Range: bytes=<n>-` resume for the unfinished file (no re-fetch of finished files), progress continues from about the cancelled percent, not from 0, and the model becomes Available.
  - Note (2026-10-02): open; needs the desktop app on Windows. Left for the manual-check pass.
- [ ] 1.15 Manual Windows check, cancel then immediate retry.
  1. Start a download.
  2. Click Cancel, then immediately click Retry/Download several times (also try the onboarding Retry).

  verify: Retry stays disabled until the `cancelled` event arrives, no `Download already in progress` error toast appears, the console log shows exactly one request sequence after the cancel, and the final files match the exact sizes.
  - Note (2026-10-02): open; needs the desktop app on Windows. Left for the manual-check pass.
- [x] 1.16 Commit group 1 after running `cargo test -p meetily --lib model_download parakeet_engine`, `bun test tests/` and `pnpm exec tsc --noEmit -p .` (frontend/). verify: all pass; `git show --stat HEAD` lists only group 1 files.

## 2. Whisper download hardening (upstream #737, download part)

- [x] 2.1 In `frontend/src-tauri/src/whisper_engine/whisper_engine.rs`:
  - Replace `cancel_download_flag`/`active_downloads` (`:51-54`, `:179-182`) with `downloads: DownloadOwners`.
  - Replace the partial-file Downloading heuristic in `discover_models` (`:217-246`) with the owner overlay (`Downloading { progress: owner.progress() }`), as in upstream `9f24062` `:239-291`.

  verify: `cargo check -p meetily`.
  - Note (2026-10-02): discovery also uses the Parakeet-style owner-revision retry (not in upstream Whisper), so a scan that started before a commit cannot overwrite the committed status; the disk scan moved to `scan_models_on_disk` with the old Corrupted/Missing rules.
- [x] 2.2 Rewrite `download_model` (`:1210-1438`) as `download_model` (URL table `:1242-1262`, unsupported model rejected before reserving) → `download_model_from_url` → `download_model_with_owner` + `finish_download`, ported from upstream `9f24062` `:948-1237`. The port must:
  - use `Client::builder().user_agent(concat!("Meetily/", env!("CARGO_PKG_VERSION")))`
  - use `select!` on the owner token for send and for each chunk
  - call `owner.set_progress`
  - route every error through `finish_download`, which runs the header check `validate_model_file` plus the ≥ 90% `WHISPER_MODEL_CATALOG` size check, removes the file on error or cancel, and commits `Available`/`Missing` under the owners and catalog locks

  Then replace `cancel_download` (`:1440-1480`) with `cancel_download` → `cancel_download_with_timeout` returning `CancelDownloadOutcome`.

  verify: `cargo check -p meetily`, and `grep -n "cancel_download_flag\|Client::new()\|sleep(" frontend/src-tauri/src/whisper_engine/whisper_engine.rs` returns nothing.
  - Note (2026-10-02): `finish_download` takes the owners and catalog locks once (simpler than upstream's two passes); a cancel that lands after validation deletes the file while the owner is still held, so no retry can start before it is gone. Per D2 the cache is written only at reserve and commit. The final `callback(100)` is kept; completion is still the separate `model-download-complete` event.
- [x] 2.3 In `frontend/src-tauri/src/whisper_engine/commands.rs`:
  - `whisper_download_model` (`:426-490`) gets an `Err(e) if is_download_cancelled(&e)` arm that returns `Ok(())` and emits `model-download-progress {modelName, progress: 0, status: "cancelled"}`, with no `model-download-error` (design D4).
  - `whisper_cancel_download` (`:493-507`) returns `Result<CancelDownloadOutcome, String>`.

  verify: `cargo check -p meetily`; `lib.rs:653` is unchanged.
- [x] 2.4 Port upstream `9f24062`'s 13 Whisper tests (`whisper_engine.rs:1275+`, names per design D8) into `whisper_engine.rs` `#[cfg(test)] mod tests`, reusing `model_download::transfer::test_server` where the response shape fits and keeping upstream's `stalled_http_server` otherwise. Add `cancelled_download_is_reported_as_cancelled_not_error`. verify: `cargo test -p meetily --lib whisper_engine::whisper_engine::tests` passes 14 tests offline.
  - Note (2026-10-02): 14 pass, loopback only. Adaptations: the progress-42 test sets `owner.set_progress(42)`; two tests reuse `transfer::test_server`; upstream's `stalled_http_server` and `response_http_server` (needed to capture the User-Agent) are kept.
- [x] 2.5 Frontend:
  - Add a Whisper-only `WhisperModelDownloadProgressPayload` (`ModelDownloadProgressPayload & { status?: 'cancelled' }`) for `listenModelDownloadProgress` (`frontend/src/lib/ipc/models.ts:163-167`), leaving the Ollama listener (`:427-431`) on the old type.
  - `whisperCancelDownload` (`:147-149`) and `WhisperAPI.cancelDownload` (`frontend/src/lib/whisper.ts:312-314`) return `CancelDownloadOutcome`.
  - In `WhisperModelManager.tsx`, add `cancellingModels`. The progress listener (`:136-160`) handles `status === 'cancelled'` (→ Missing, clear both sets, info toast). `cancelDownload` (`:259-292`) only marks cancelling and toasts on `pending`. `downloadModel` returns early while cancelling. `ModelCard` gets `isCancelling`. No timers or polling (unlike upstream's `reconcileCancellation`).

  verify: `pnpm exec tsc --noEmit -p .` is clean, and `grep -n "setTimeout\|setInterval" frontend/src/components/WhisperModelManager.tsx` shows no new occurrence vs `git show HEAD:frontend/src/components/WhisperModelManager.tsx`.
  - Note (2026-10-02): complete/error handlers now use the localStorage-persisted `updateDownloadingModels`, as upstream does; `setTimeout`/`setInterval` count 0 before and after.
- [x] 2.6 Extend `frontend/tests/lib/ipc/models.test.ts` with `whisperCancelDownload passes the cancelled/pending outcome through`. verify: `bun test tests/lib/ipc/models.test.ts` (frontend/) passes.
- [ ] 2.7 Manual Windows check: in settings, download Whisper `base`, cancel at about 50%, then immediately click Download. verify:
  - Download stays disabled until the cancelled toast appears.
  - `%APPDATA%\com.meetily.ai\models\ggml-base.bin` is absent between the cancel and the new start.
  - The second download completes, loads and transcribes.
  - No "Failed to download" toast appears after the cancel.
  - Note (2026-10-02): open; needs the desktop app on Windows. Left for the manual-check pass.
- [x] 2.8 Commit group 2 after `cargo test -p meetily --lib model_download whisper_engine`, `bun test tests/` and `pnpm exec tsc --noEmit -p .` (frontend/). verify: all pass; `git show --stat HEAD` lists only group 2 files.

## 3. Word-alignment download hardening (fork-only)

- [x] 3.1 In `frontend/src-tauri/src/audio/word_alignment/catalog.rs`:
  - Replace `ModelFile` and the `model_file` const fn (`:11-50`) with `model_download::transfer::ArtifactSpec`, keeping the existing byte values as `exact_bytes`.
  - Add `revision: "2d48b01b6429d9018f81914550565112d56f6ba7"` to `AlignmentModelSpec`.
  - `resolve_status` (`:102-125`) requires `len == exact_bytes`.
  - Update `fake_complete_dir_reports_available_and_short_file_corrupted` (`:179-203`) to write exact sizes and add a "one byte short → Corrupted" assertion.
  - Update `expected_sizes` (`download.rs:373-382`) to the new field.

  verify: `cargo test -p meetily --lib audio::word_alignment::catalog` passes 3 tests, and `grep -rn "min_bytes" frontend/src-tauri/src` returns nothing.
  - Note (2026-10-02): the catalog test creates files with `File::set_len` (sparse) instead of writing ~650 MB; truncated and one-byte-short checks both report Corrupted.
- [x] 3.2 In `frontend/src-tauri/src/audio/word_alignment/download.rs`:
  - Replace `cancel_flag`/`active_downloads` (`:32-41`) with `DownloadOwners`.
  - `download_model_inner` (`:105-323`) calls `transfer::download_artifacts` with base URL `https://huggingface.co/{hf_repo}/resolve/{revision}`, then keeps the integrity gate (`:303-309`).
  - `status()` (`:51-60`) overlays `owner.progress()`.
  - `cancel_download` (`:326-364`) becomes `cancel_with_timeout` returning `CancelDownloadOutcome` and deletes nothing.
  - `delete_model` keeps rejecting while owned.

  verify: `cargo check -p meetily`, and `grep -n "resolve/main\|cancel_flag\|sleep(" frontend/src-tauri/src/audio/word_alignment/download.rs` returns nothing.
  - Note (2026-10-02): `delete_model` also holds the owners lock while removing the directory, so a download cannot start between the ownership check and the delete. A private `download_spec_from_source` seam lets tests use a 2-file spec against a loopback server. Final 100% progress is reported only after the owner is released.
- [x] 3.3 In `frontend/src-tauri/src/audio/word_alignment/commands.rs`:
  - `download_alignment_model` (`:78-115`) maps a cancel to `Ok(())` plus `alignment-model-download-progress {modelId, progress: 0, status: "cancelled"}` instead of `alignment-model-download-failed`.
  - `cancel_alignment_download` (`:119-124`) returns `Result<CancelDownloadOutcome, String>`.
  - `list_alignment_models` (`:56-59`) overlays manager status, as `check_alignment_models` does (`:63-73`).

  verify: `cargo check -p meetily`; `lib.rs:673` is unchanged.
- [x] 3.4 Add loopback tests to `download.rs` `#[cfg(test)] mod tests` using `model_download::transfer::test_server` and a 2-file test spec: `completed_file_is_skipped_and_partial_resumes_with_validated_range`, `mismatched_content_range_fails_without_publishing_available`, `cancel_keeps_partials_and_releases_owner`. verify: `cargo test -p meetily --lib audio::word_alignment::download` passes 3 tests offline.
- [x] 3.5 Frontend:
  - In `frontend/src/lib/ipc/models.ts`, `AlignmentDownloadProgressPayload` (`:466-472`) gains `status?: 'cancelled'`, and `cancelAlignmentDownload` (`:496-498`) and `alignmentService.cancelDownload` (`frontend/src/services/alignmentService.ts:48-50`) return `CancelDownloadOutcome`.
  - In `WordAlignmentSettings.tsx`, the progress listener (`:64-72`) calls `refresh()` on `cancelled`, and `cancel` (`:120-128`) keeps Download disabled while the result is `pending`.
  - Extend `frontend/tests/lib/ipc/models.test.ts` with `cancelAlignmentDownload passes the outcome through`.

  verify: `pnpm exec tsc --noEmit -p .` and `bun test tests/lib/ipc/models.test.ts` (frontend/) pass.
  - Note (2026-10-02): the payload's byte/speed fields became optional (the cancelled event carries none; nothing reads them). A `cancelled` outcome clears the pending state immediately, so a cancel that finds no download cannot leave the card stuck.
- [ ] 3.6 Manual Windows check:
  1. Enable word alignment in settings and download `wav2vec2-xlsr-56`.
  2. Cancel at about 40%, then reopen settings.
  3. Download again.

  verify: after reopening, the card shows "Needs re-download" (not Downloading), `model_fp16.onnx` still holds its partial bytes, the console log shows a resume `Range` request that the HF CDN answers with `206`, and the final file is exactly 651760843 bytes and reported Ready.
  - Note (2026-10-02): open; needs the desktop app on Windows. Left for the manual-check pass.
- [x] 3.7 Commit group 3 after `cargo test -p meetily --lib model_download audio::word_alignment`, `bun test tests/` and `pnpm exec tsc --noEmit -p .` (frontend/). verify: all pass; `git show --stat HEAD` lists only group 3 files.

## 4. Final integration checks

- [x] 4.1 Run the prescribed-skips full Rust suite. verify: 0 failed; passed = task 0.1 baseline + the new tests (owners 4, transfer 2, Parakeet 11, Whisper 14, alignment download 3, alignment catalog net 0).
  - Note (2026-10-02): 563 passed / 0 failed / 9 ignored = 529 + 34 (owners 4, transfer 2, Parakeet 11, Whisper 14, alignment download 3).
- [x] 4.2 Run `cargo clippy -p meetily --all-targets --message-format=short`. verify: the warning count is ≤ the task 0.1 baseline, and none of the warnings points into `model_download/`, `parakeet_engine/`, `whisper_engine/whisper_engine.rs` or `audio/word_alignment/`.
  - Note: 32 warnings, none in the touched modules.
- [x] 4.3 Run `bun test tests/`, `pnpm exec tsc --noEmit -p .` and `pnpm exec next lint` (frontend/). verify: tests pass (baseline + 3 new), tsc is clean, and lint has no new findings vs task 0.1.
  - Note: 99 pass (96 + 3), tsc clean, lint 42 findings as at baseline.
- [x] 4.4 Confirm the IPC names are stable. verify: `git diff <group-0 base>..HEAD -- frontend/src-tauri/src/lib.rs` adds only the `pub mod model_download;` line (no `generate_handler!` edits), and `git grep -hoE '"[a-z-]+-download-(progress|complete|completed|error|failed)"' <ref> -- frontend/src-tauri/src | sort -u` prints the same list for `<ref>` = `HEAD` and `<ref>` = the group-0 base.
  - Note: lib.rs diff is only `+pub mod model_download;`; the 13 download event names are identical at HEAD and `b1acbf1`.
- [x] 4.5 Run `graphify update .`, then `openspec validate harden-model-downloads --strict`. verify: graphify finishes without error, and validate reports the change valid.
