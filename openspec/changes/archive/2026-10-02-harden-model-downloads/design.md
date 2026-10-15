# Design

## Context

See `proposal.md` (Why) for the failure modes. The current state and constraints that shape the approach are below. Line references are against `feat/merge_0.4.1` at `f9919e4`.

**Three hand-rolled downloaders share one flawed protocol.**
- `ParakeetEngine` (`parakeet_engine/parakeet_engine.rs`), `WhisperEngine` (`whisper_engine/whisper_engine.rs`) and `AlignmentModelManager` (`audio/word_alignment/download.rs`) each keep:
  - a global `cancel_download_flag: RwLock<Option<String>>` (`parakeet_engine.rs:123`, `whisper_engine.rs:52`, `download.rs:32`)
  - an `active_downloads: RwLock<HashSet<String>>` (`parakeet_engine.rs:125`, `whisper_engine.rs:54`, `download.rs:33`)
- Each starting download clears the global flag (`parakeet_engine.rs:659-663`, `whisper_engine.rs:1235-1239`, `download.rs:91`), so a fast cancel then retry can lose the cancel.
- Cancel removes the active entry immediately, then waits a fixed time and deletes files while the worker may still be writing:
  - Parakeet waits 100 ms, then runs `remove_dir_all` (`parakeet_engine.rs:1207-1234`).
  - Whisper waits 100 ms, then runs `remove_file` (`whisper_engine.rs:1449-1477`).
  - Alignment polls 50 × 100 ms, then deletes files below `min_bytes` (`download.rs:333-352`).
- `parakeet_retry_download` force-removes the active entry and resets status (`parakeet_engine/commands.rs:525-547`). The result is a second writer.

**Parakeet trusts approximate sizes and ignores range headers.**
- Sizes are approximate tables (`parakeet_engine.rs:742-780`). Minimum-size validation runs at 89-90% (`:303-317`) and the skip tolerance is 0.99 (`:833`, `:884`).
- A `206` response is trusted from `Content-Length` alone (`:866-870`).
- `clean_incomplete_model_directory` (`:349-398`, called at `:724`) deletes all files when the directory fails validation.
- `load_model` holds `available_models.read()` across the synchronous `ParakeetModel::new` (`:402-437`).

**Fork-only code in `parakeet_engine.rs` must survive.** `transcribe_audio_with_tokens` and its `catch_unwind` guard (`:495-556`) were added in commits `82fe7c5` and `fe437a1`. Upstream v0.4.1 has neither.

**Whisper leaks its active entry on errors.**
- Every `?` in `download_model` after the reservation returns with the model still in `active_downloads` and still `Downloading`, which blocks retry with "already in progress" until restart. The early returns are at:
  - `:1295` (send)
  - `:1321` (create)
  - `:1356` (chunk)
  - `:1360` (write)
  - `:1418` (flush)
- The client is `Client::new()` (`:1288`) with no User-Agent.
- The file is marked Available with no header or size check (`:1422-1429`).
- A user cancel surfaces as `Err("Download cancelled by user")`, so `whisper_download_model` emits `model-download-error` after a cancel (`whisper_engine/commands.rs:473-486`).

**Word alignment has the same weaknesses.**
- Readiness and skip use `min_bytes` ≈ 92% of expected (`catalog.rs:36-50`, `download.rs:156`). A truncated 95% `model_fp16.onnx` is reported Available.
- `206` is not validated (`download.rs:176-179`).
- The URL is `resolve/main` (`download.rs:115`).
- `list_alignment_models` reads disk only (`catalog.rs:140-153`), so the settings UI shows Corrupted/Missing during an active download and offers a second Download.

**Frontend.**
- All IPC goes through `frontend/src/lib/ipc/models.ts`. `ModelStatus` (`:59-64`) declares `{ Downloading: number }`. Rust serializes `Downloading { progress }` as `{ "Downloading": { "progress": n } }` (`parakeet_engine.rs:28-30`, `whisper_engine.rs:20-22`). This drift is documented as deliberately kept at `:54-57`.
- The managers build `{ Downloading: n }` locally:
  - `ParakeetModelManager.tsx:106,260`, read at `:449-450`
  - `WhisperModelManager.tsx:103,154,305`, read at `:524-525`
  - `ModelDownloadProgress.tsx:15` (the component is currently unused)
- Three listeners treat `progress >= 100` as completion:
  - `OnboardingContext.tsx:262`
  - `DownloadProgressStep.tsx:220`
  - `DownloadProgressToast.tsx:240`

**Upstream references.**
- v0.4.1 `parakeet_engine.rs`:
  - catalog `:84-145`
  - owner state `:147-173`
  - `Content-Range` parsing `:190-230`
  - discovery with revision retry `:336-411`
  - serialized load `:432-530`
  - transfer `:692-1131`
  - commit `:1133-1193`
  - cancel `:1196-1229`
  - tests `:1233-2016`
- `9f24062` `whisper_engine.rs`: owner `:38-66`, `finish_download` `:948-1071`, transfer `:1118-1237`, cancel `:1239-1271`, tests `:1275+`.
- The fork has rustfmt'd and diverged both files, so every port is applied by hand.

**Verified live on 2026-10-02.**
- The v2 HF repo head is `0bbb45a3365852604aef28b538a8f066f4ccaa85`, which is upstream's pin. Its sizes match upstream's `PARAKEET_V2_ARTIFACTS`.
- The v3 CDN (`meetily.towardsgeneralintelligence.com`) sends `Content-Length` equal to upstream's `PARAKEET_V3_ARTIFACTS`. It answers `Range` with `206` and a correct `Content-Range`.
- The alignment repo head is `2d48b01b6429d9018f81914550565112d56f6ba7`. Its tree sizes equal the fork's `expected_bytes` (`catalog.rs:60-65`). `resolve/<sha>` → 302 → CDN answers `Range` with a correct `206`.

## Goals / Non-Goals

**Goals:**
- One ownership and cancellation protocol for all three downloaders. A download's claim on a model is released only by its own worker, after cleanup.
- Parakeet and alignment readiness and skip decisions are exact-size. Resume is accepted only on a validated `Content-Range`.
- Keep upstream identifiers where the code is ported, so future upstream diffs stay recognizable: `ArtifactSpec`, `ModelSpec`, `PARAKEET_MODEL_SPECS`, `find_model_spec`, `discover_models_from_specs`, `download_model_detailed_from_source`, `finish_download`, `cancel_download_with_timeout`, `DownloadCancelled`, `is_download_cancelled`, `CancelDownloadOutcome`, `CANCEL_DOWNLOAD_CLEANUP_TIMEOUT`, and the test names.
- Command and event names stay unchanged. Every payload or return change is listed in D6.
- Port upstream's loopback tests so each engine's contract is pinned by tests that need no network.

**Non-Goals:**
- Checksums, temp-file-plus-rename, or parallel per-file downloads. Upstream does none of these, and exact size plus a pinned revision is the agreed integrity bar.
- Resumable Whisper downloads. `WHISPER_MODEL_CATALOG` (`config.rs:18`) has only MB sizes, and upstream #737 deliberately deletes Whisper partials.
- `summary/summary_engine/model_manager.rs`, which has the same single-flag pattern. It is left for a follow-up.
- The #608 import-duration part of #737, and #767's `ensure_onnx_runtime_available`.
- A 30 s stall timeout for Whisper. Cancellation is responsive through `select!`, and upstream did not add one.

## Decisions

### D1: A shared `crate::model_download` module for the protocol; engine specifics stay in place

New module `frontend/src-tauri/src/model_download/` (`pub mod model_download;` in `lib.rs`) with two files.

**`owners.rs`**: ownership and cancellation, used by Parakeet, Whisper and alignment. It is lifted from upstream's Parakeet `ActiveDownload`/`ActiveDownloadState`.

```rust
pub struct DownloadOwner { cancellation: CancellationToken, completion: watch::Sender<bool>, progress: AtomicU8 }
pub struct DownloadOwners { state: tokio::sync::Mutex<OwnersState> }        // map + revision: u64
impl DownloadOwners {
    pub async fn reserve(&self, key: &str) -> Result<Arc<DownloadOwner>>;  // Err "Download already in progress for model: {key}"
    pub async fn lock(&self) -> OwnersGuard<'_>;                            // is_owner(key,&owner), contains(key), owner(key), release(key) (removes + bumps revision), revision()
    pub async fn cancel_with_timeout(&self, key: &str, t: Duration) -> Result<CancelDownloadOutcome>;
}
impl DownloadOwner { pub fn cancellation(&self) -> &CancellationToken; pub fn set_progress(&self, p: u8); pub fn progress(&self) -> u8; pub fn signal_done(&self); }
#[derive(thiserror::Error)] #[error("Download cancelled by user")] pub struct DownloadCancelled;
pub fn is_download_cancelled(e: &anyhow::Error) -> bool;
#[derive(Serialize)] #[serde(rename_all = "lowercase")] pub enum CancelDownloadOutcome { Cancelled, Pending }
pub const CANCEL_DOWNLOAD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
```

**`transfer.rs`**: the exact-size resumable multi-file transfer, used by Parakeet and alignment. It contains:
- `ArtifactSpec { remote: &'static str, local: &'static str, exact_bytes: u64 }`, with `const fn ArtifactSpec::same(name, bytes)` for Parakeet, where remote and local names coincide.
- `parse_content_range`, `validate_full_response`, `validate_partial_response`, `validate_unsatisfied_response`. These are upstream `:190-230,712-810`, verbatim apart from the name split.
- `download_artifacts(client, base_url, dir, artifacts, owner, on_progress: &(dyn Fn(TransferProgress) + Send + Sync)) -> Result<TransferProgress>`, which is upstream `:849-1131`.
- `TransferProgress` carries `{ confirmed_bytes, total_bytes, speed_mbps, percent }`, with in-flight percent capped at 99.
- Behind `#[cfg(test)] pub(crate) mod test_server`: upstream's loopback `ExpectedResponse`/`serve_requests` (`:1257-1342`), shared by all three engines' tests.

**What stays in each engine:**
- **Parakeet** keeps its catalog (`ModelSpec`, `PARAKEET_MODEL_SPECS`), revision-retried discovery, `finish_download` (the atomic commit described in D3), the load/unload lifecycle lock and its `#[cfg(test)]` hooks.
- **Whisper** keeps its single-file, non-resumable transfer and its `finish_download` validation. It uses only `owners.rs`.
- **Alignment** keeps its catalog and status, and uses both files.

**Why a shared module rather than three separate ports:**
- `download.rs:1-4` already says it "structurally mirrors" the Parakeet downloader. Porting separately would copy the `Content-Range` parser, the three response validators and the owner/cancel protocol twice.
- Whisper's owner code in upstream is a third near-identical copy (`9f24062` `:38-66`, `:932-946`, `:1239-1271`).
- A single owner type also gives one `CancelDownloadOutcome` and one `DownloadCancelled` for the IPC layer. Two identically named types would otherwise be re-exported through `whisper_engine::*` and `parakeet_engine::*`.

**Cost to future upstream merges:**
- Upstream edits to the transfer loop or the owner struct will no longer apply textually and must be re-applied in `model_download/`.
- That cost is small. The fork's files already differ from upstream by rustfmt and fork-only code, so no upstream hunk in these regions applies cleanly today.
- The code upstream is most likely to keep changing stays in `parakeet_engine.rs` under upstream names: the catalog (new models, sizes, URLs), discovery, commit and the load lifecycle.
- Upstream's tests drive the transfer through `ParakeetEngine` (`download_model_detailed_from_source`). They therefore port into `parakeet_engine.rs` almost verbatim and keep testing the shared transfer through its main consumer.

**Alternatives considered:**
- **(a) Port each engine separately, upstream-faithful.** The upstream diff would map 1:1, but three copies of the owner protocol and two copies of the range validation would drift independently. That is the failure mode this change fixes.
- **(b) Also move Whisper onto the resumable transfer.** Rejected. It needs exact byte sizes for all 12 Whisper models (new catalog data, not a port) and reverses #737's delete-on-failure choice. It can be added later by adding sizes to `WHISPER_MODEL_CATALOG`.
- **(c) Move discovery and commit into the helper as well.** Rejected. Parakeet, Whisper and alignment each have their own status types and caches (`ModelStatus` ×2 and `AlignmentModelStatus`). A generic commit would need a trait for little gain and would move the upstream-churning code out of its upstream file.

### D2: Progress lives on the owner; caches are written only at reserve and commit

Upstream writes `available_models` (a `tokio::RwLock`) once per progress report from inside the transfer loop (`:906,931,1084`). With the transfer in a shared, engine-agnostic module, the loop only calls a synchronous `on_progress`.

Each engine's callback does two things:
- It stores `percent` with `owner.set_progress(..)`.
- It emits the engine's existing event. Parakeet emits `DownloadProgress` through the existing callback chain in `commands.rs:401-427`. Alignment emits through `commands.rs:86-97`.

Discovery and status reads overlay `Downloading { progress: owner.progress() }` for any key present in `DownloadOwners`. That covers Parakeet `discover_models_from_specs`, Whisper `discover_models`, and alignment `status()` and `list_alignment_models`.

Whisper keeps its inline loop and also calls `owner.set_progress`.

Effects:
- No async lock in the hot loop.
- Remounting the settings page during a download shows the real percent. Today Parakeet discovery reports `progress: 0` (`parakeet_engine.rs:209`) and alignment reports `progress: 0` (`download.rs:57`).
- `check_active_transcription_model_ready` (`audio/transcription/commands.rs:41-50,66-75`) keeps matching `Downloading { .. }` unchanged.

Alternative considered: an async progress sink (`FnMut -> BoxFuture`). Rejected because it adds complexity for a value that only status reads need.

### D3: Parakeet commit and cancel semantics (upstream #749)

**Order:**
1. `reserve` the owner, then set the status to `Downloading{0}`.
2. Run `download_artifacts`.
3. Call `finish_download`.

**`finish_download`:**
1. If the transfer succeeded and was not cancelled, re-run `validate_model_directory` with exact sizes.
2. Take the owners lock, then the `available_models` write lock.
3. If this owner is still the registered one:
   - release the owner (which bumps the revision)
   - set `Missing` if cancelled or failed, otherwise `Available`
4. Drop both locks, then `signal_done()`.
5. Emit the final 100% progress (status `completed`) only after the commit.

**Partial files survive** cancel, error and stall. The worker flushes its `BufWriter` before returning `DownloadCancelled` or the error (upstream `:1001,1010,1020,1037`). Nothing in the cancel path deletes files. `clean_incomplete_model_directory` (`:349-398`) is deleted together with its call at `:724`.

**Discovery** (`discover_models_from_specs`, upstream `:336-411`):
- It scans disk without locks, then re-takes the owners lock. If the revision changed during the scan, it retries.
- It overlays owned models as Downloading.
- The retry closes the race where a scan sees a half-written directory, the download commits Available, and the stale Corrupted result then overwrites it (upstream test `discovery_retries_when_download_finalizes_after_disk_scan`).

**Load:**
- `load_model` clones the `ModelInfo` and drops the catalog read lock.
- It takes `model_lifecycle_lock: tokio::sync::Mutex<()>`, unloads under that lock, and runs `ParakeetModel::new` in `spawn_blocking`.
- `unload_model` takes the same lock.
- `transcribe_audio_with_tokens` does not take the lifecycle lock; it only takes `current_model.write()`, as today. On a caught panic it calls `unload_model()` after `drop(model_guard)` (`:538-539`). That waits for any in-flight load and cannot deadlock, because transcription never holds the lifecycle lock.

**Commands** (`parakeet_engine/commands.rs`):
- `parakeet_download_model` maps `Err(e) if is_download_cancelled(&e)` to `Ok(())`. It emits `parakeet-model-download-progress {modelName, progress: 0, status: "cancelled"}` and no error event. This is upstream `commands.rs` after #749.
- `parakeet_cancel_download` drops its `app_handle` parameter and its own emit (`:479-509`). It returns `CancelDownloadOutcome`.
- `parakeet_retry_download` loses the force-reset block (`:525-547`) and calls `parakeet_download_model`. The reservation is the guard against a second writer.
- The `pub(crate) active_downloads` field (`parakeet_engine.rs:125`) is removed. Its only external user was the retry block.

### D4: Whisper semantics (upstream #737, download part)

**Ownership and status:**
- `active_downloads` becomes `DownloadOwners` and `cancel_download_flag` is removed.
- `discover_models` replaces the partial-file heuristic (`whisper_engine.rs:217-246`) with the owner overlay from D2.

**Download path:**
- `download_model` splits into `download_model_from_url(name, url, cb)` and `download_model_with_owner`.
- `download_model_from_url` reserves the owner, runs the transfer, and always passes its result through `finish_download`. This fixes the five leaking `?` returns, because every error now reaches `finish_download`.
- The URL table stays as it is (`:1242-1262`). An unsupported model returns before reserving.
- The client is built with `Client::builder().user_agent(concat!("Meetily/", env!("CARGO_PKG_VERSION")))`.
- Chunk reads use `tokio::select! { biased; _ = owner.cancellation().cancelled() => …, chunk = stream.next() => … }`.

**`finish_download`** (upstream `9f24062` `:948-1071`):
1. On success, check the header with `validate_model_file` and require `len >= (size_mb * 0.9) MiB` from `WHISPER_MODEL_CATALOG`.
2. On any error or cancel, remove the file.
3. Release the owner under both locks. Set `Available` or `Missing`; if a cancel won, set `Missing`.
4. Call `signal_done()`.

**Late cancel:** a cancel that finds no owner returns `Cancelled` and touches no file. This covers a download that completed before the cancel reached it (upstream test `late_cancellation_preserves_a_completed_model_state`).

**Commands:**
- `whisper_download_model` maps a cancelled result to `Ok(())`.
- It emits `model-download-progress {modelName, progress: 0, status: "cancelled"}` and no error event.
- This is a deliberate deviation from upstream. Upstream's UI instead polls `getAvailableModels` every 1 s (`reconcileCancellation` in `WhisperModelManager.tsx` at `9f24062`). Emitting one event keeps a single source of UI truth, with no timers, consistent with Parakeet. It also works when the settings page is remounted mid-cancel, which a promise-based signal would not.

### D5: Word-alignment semantics

**Catalog** (`catalog.rs`):
- `ModelFile` becomes `transfer::ArtifactSpec`. `min_bytes` and the `model_file` const fn (`:36-50`) are removed, and the existing `expected_bytes` values are kept as `exact_bytes`.
- `resolve_status` requires `len == exact_bytes`.
- `AlignmentModelSpec` gains `revision: &'static str` = `"2d48b01b6429d9018f81914550565112d56f6ba7"`.
- `download.rs:115` builds `https://huggingface.co/{hf_repo}/resolve/{revision}`.

**Download** (`download.rs`):
- `download_model_inner` becomes a call to `transfer::download_artifacts`. Afterwards the integrity gate `resolve_status == Available` runs, as today (`:303-309`).
- Cancel is `owners.cancel_with_timeout(id, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)`. Partials are kept, consistent with Parakeet. The deletion loop (`:341-362`) is removed.
- `delete_model` keeps rejecting while an owner exists (`:64-66`).

**Commands** (`commands.rs`):
- `download_alignment_model` maps a cancel to `Ok(())` and emits `alignment-model-download-progress {modelId, progress: 0, status: "cancelled"}` instead of `alignment-model-download-failed`.
- `cancel_alignment_download` returns `CancelDownloadOutcome`.
- `list_alignment_models` overlays owner status, as `check_alignment_models` already does (`:63-73`).

### D6: IPC contract changes (names unchanged)

| Command / event | Before | After |
|---|---|---|
| `parakeet_cancel_download` | `Result<(), String>`; emits the `cancelled` progress event itself | `Result<"cancelled" \| "pending", String>`; emits nothing |
| `parakeet_download_model` | on cancel: `Err` + `parakeet-model-download-error` | on cancel: `Ok(())` + `parakeet-model-download-progress {status:"cancelled", progress:0}` |
| `parakeet-model-download-progress` | `status:"completed"` whenever `percent == 100` (`commands.rs:423`) | in-flight percent is capped at 99; `completed` (100) is emitted only after the model is committed Available. Payload fields unchanged |
| `parakeet_retry_download` | force-clears the owner, then downloads | plain download; rejected with "Download already in progress" while a cancel is pending |
| `whisper_cancel_download` | `Result<(), String>` | `Result<"cancelled" \| "pending", String>` |
| `whisper_download_model` | on cancel: `Err` + `model-download-error` | on cancel: `Ok(())` + `model-download-progress {status:"cancelled", progress:0}` |
| `model-download-progress` (Whisper) | `{modelName, progress}` | adds optional `status?: "cancelled"`. The TS type splits from the Ollama payload, which shares `ModelDownloadProgressPayload` (`models.ts:24-28,428-430`) |
| `cancel_alignment_download` | `Result<(), String>` | `Result<"cancelled" \| "pending", String>` |
| `download_alignment_model` | on cancel: `Err` + `alignment-model-download-failed` | on cancel: `Ok(())` + `alignment-model-download-progress {status:"cancelled", progress:0}` |
| `alignment-model-download-progress` | `{modelId, progress, downloaded_bytes, total_bytes, speed_mbps}` | adds optional `status?: "cancelled"` |
| `list_alignment_models` | disk-only status | overlays `Downloading{progress}` for owned downloads |
| TS `ModelStatus` (`models.ts:59-64`) | `{ Downloading: number }` (wrong) | `{ Downloading: { progress: number } }`, matching Rust; the drift note at `:54-57` is removed |

Rust `ModelStatus` serialization is unchanged. All three cancel results serialize as lowercase strings through `#[serde(rename_all = "lowercase")]`.

### D7: Frontend state handling (upstream #749 UI, extended to Whisper and alignment)

**Parakeet and Whisper managers:**
- Each manager adds a `cancellingModels: Set<string>`.
- Clicking cancel adds the model to the set. A `pending` result shows an info toast ("still shutting down; retry will be available when cleanup completes").
- The set entry is cleared only by a terminal backend event: `status:"cancelled"`, `*-download-complete` or `*-download-error`. A failed cancel `invoke` also clears it.
- Download, Retry and Re-download are disabled while the model is in `downloadingModels` or `cancellingModels`.
- `ParakeetModelManager` also adopts upstream's `listenersReady` gate and `latestStatusByModelRef`. Listeners register before the first `getAvailableModels`, and an event that arrives first wins over the stale list.

**Onboarding and toast:**
- `OnboardingContext`, `DownloadProgressStep` and `DownloadProgressToast` mark Parakeet complete only on `status === 'completed'` and drop `progress >= 100`.
- `OnboardingContext` and `DownloadProgressStep` handle `cancelled`: reset progress, mark not downloaded, and show "Download cancelled" with the existing Retry button.

**Alignment:** `WordAlignmentSettings` calls `refresh()` on the `cancelled` progress event and disables Download while a cancel is pending.

**Constraints:** no polling and no new timers. The toast's existing auto-dismiss `setTimeout` (`DownloadProgressToast.tsx:154`) is UI dismissal, not state, and stays.

### D8: Tests

**Parakeet:** port upstream's 10 tests into `parakeet_engine.rs` `mod tests`, under upstream names:
- `directory_validation_requires_exact_artifact_sizes`
- `loading_releases_available_models_and_serializes_unload`
- `completed_sibling_survives_403_then_retry_resumes_partial`
- `cancelled_near_complete_artifact_is_resumed_on_retry`
- `range_ignored_replaces_partial_with_honest_progress`
- `range_416_retries_fresh_with_honest_progress`
- `invalid_or_short_response_never_publishes_available`
- `pending_cancellation_keeps_owner_and_blocks_retry`
- `cancellation_wins_before_terminal_commit`
- `discovery_retries_when_download_finalizes_after_disk_scan`

Adaptations: `engine.active_downloads.lock().await.downloads.contains_key(..)` becomes `engine.downloads.lock().await.contains(..)`, `ArtifactSpec { filename, .. }` becomes `ArtifactSpec::same(..)`, and the loopback server is imported from `model_download::test_server`.

**Whisper:** port upstream's 13 `9f24062` tests into `whisper_engine.rs` `mod tests`, with the same kind of adaptations. Add one fork test, `cancelled_download_is_reported_as_cancelled_not_error`, which checks `is_download_cancelled` on the worker result.

**Shared module:** `model_download/transfer.rs` gets unit tests for `parse_content_range` (valid, `*/N`, malformed, start > end). `model_download/owners.rs` gets tests for reserve/duplicate, cancel without an owner returning `Cancelled`, and cancel timing out to `Pending`.

**Alignment:** `download.rs` gets loopback tests for:
- `completed_file_is_skipped_and_partial_resumes_with_validated_range`
- `mismatched_content_range_fails_without_publishing_available`
- `cancel_keeps_partials_and_releases_owner`

The existing `catalog.rs` test `fake_complete_dir_reports_available_and_short_file_corrupted` (`:179-203`) switches from `min_bytes` to `exact_bytes`, and adds a "one byte short → Corrupted" assertion.

**Frontend:** `frontend/tests/lib/ipc/models.test.ts` follows the `tests/lib/ipc/analytics.test.ts` pattern and mocks both `@tauri-apps/api/core` and `@tauri-apps/api/event`. It asserts that the three cancel wrappers pass `'cancelled'`/`'pending'` through.

## Risks / Trade-offs

- **[A partial from a different source revision is resumed into a wrong file of the right length.]** Exact size without checksums cannot detect a prefix from other content. → Live check on 2026-10-02: v2 `main` equals the pinned sha, the v3 CDN sizes equal upstream's catalog, and the alignment `main` equals the pinned sha. Existing partials therefore come from identical content. A wrong-content Parakeet file still fails at ONNX load, which is caught (`load_model` error → UI error), and Delete plus Download recovers it.
- **[The cancelled-partial directory shows as "Corrupted" after a page refresh or restart.]** This is upstream behavior; discovery cannot tell resumable from damaged. → The Corrupted card already offers "Re-download" (`ParakeetModelManager.tsx:558-579`), which resumes. The `cancelled` event sets `Missing` in the live session.
- **[`ParakeetModel` may not be `Send`, which `spawn_blocking` requires.]** The fork's `model.rs` differs from upstream. → Task 1.6 checks this with `cargo check`. Fallback: keep the load inline but still drop the catalog lock first and serialize through `model_lifecycle_lock`. The spec requirement still holds; only one runtime worker is blocked during the load.
- **[Hugging Face may stop honoring `Range` after the redirect.]** reqwest follows `resolve/<sha>` 302s to a CDN, and dropping `Range` there would break resume. → Checked live: the CDN returns a correct `206`. If a server ignores `Range`, the `200` path restarts the file with honest progress (upstream test `range_ignored_replaces_partial_with_honest_progress`), so the download stays correct, only slower. Manual task 3.6 checks that the app log shows resume on the alignment model.
- **[A cancel can stay pending forever if a worker is stuck in a filesystem call.]** → The UI keeps retry disabled, and the stall timeout and `select!` cover network stalls. A stuck filesystem call is outside this change; restarting the app clears the in-memory owner.
- **[The UI can stay in "cancelling" when a cancel finds no owner and no terminal event follows.]** This happens only if the frontend thought a download was running when it was not. Upstream accepts the same edge. → Remounting the settings page re-reads backend status. This is not specified as behavior.
- **[Upstream merge cost from D1]** → Accepted. Upstream names are preserved, and a short module doc comment in `model_download/mod.rs` names the upstream line ranges each function came from.

## Migration Plan

- There is no data migration. Existing complete installs whose sizes match the exact catalog stay Available. Upstream's sizes were checked against both sources.
- An install that fails exact validation shows Corrupted. Re-download keeps the files that are already exact and resumes or refetches the rest.
- Each task group in `tasks.md` is one commit: Parakeet, Whisper, then word alignment. Rollback is a revert of the group's commit. Group 1 introduces `model_download/`, and groups 2 and 3 depend on it, so reverting group 1 requires reverting 2 and 3 first.

## Open Questions

- Should `summary/summary_engine/model_manager.rs` (same single-flag pattern, `:121-124`, `:808-820`) adopt `DownloadOwners` in a follow-up change? This does not affect this change's specs or tasks.
