# Proposal

## Why

Parakeet is the fork's default transcription provider (`frontend/src-tauri/src/config.rs:12` `DEFAULT_PARAKEET_MODEL = "parakeet-tdt-0.6b-v3-int8"`, `frontend/src/contexts/ConfigContext.tsx:98-99`), and its downloader loses finished work. `clean_incomplete_model_directory` (`parakeet_engine/parakeet_engine.rs:349`, called at `:724`) deletes every file in the model directory when validation fails, so a finished 652 MB encoder is fetched again on every retry after a decoder failure (upstream issue #662). A partial file at 99% of its approximate size counts as complete (`:833`, `:884`). `206` responses are trusted without checking `Content-Range` (`:866-870`). Cancel uses one global flag (`:123`), drops the active entry, sleeps 100 ms and runs `remove_dir_all` (`:1198-1236`), so it can race a worker that is still writing. Retry force-removes the active entry (`parakeet_engine/commands.rs:525-534`), which allows two writers. Upstream v0.4.1 fixed this in #682 and #749 (merge `d1f7a11`). The Whisper downloader (#737, merge `9f24062`) and the fork-only word-alignment downloader (`audio/word_alignment/download.rs`) have the same ownership and cancellation flaws, and they mark files Available without checking them.

## What Changes

- **Parakeet (port of #682/#749)**:
  - A static catalog lists every artifact with its exact byte size. The v2 Hugging Face URL is pinned to a commit instead of `resolve/main` (`parakeet_engine.rs:689`).
  - A file is skipped only when its size matches exactly. A partial file resumes with `Range`, and every `206` is checked against `Content-Range`. A `200` truncates the file and restarts progress from honest byte counts. A `416` is checked, then the file is fetched fresh. An overlong body is rejected.
  - Completed sibling files are never deleted. `clean_incomplete_model_directory` and the 0.99 tolerances are removed.
  - Each download has its own owner (cancellation token plus a completion signal). The owner is held until cleanup finishes, so cancel returns `cancelled` or `pending` and a retry is rejected while cleanup runs.
  - Model load and unload are serialized, and the catalog lock is released before the native ONNX load. The fork-only `transcribe_audio_with_tokens` and its `catch_unwind` guard (`:495-556`) are preserved.
- **Whisper (download part of #737)**:
  - Each download has its own owner and cancellation token. A cancel finishes cleanup before a retry can start, and the cancel ack is bounded to 5 s with a `cancelled`/`pending` result.
  - Requests send an explicit `Meetily/<version>` User-Agent. A file becomes Available only after its GGML/GGUF header check and a minimum-size check pass. On error or cancel the partial file is removed and the status returns to Missing.
  - Discovery reports an owned download as Downloading.
  - The #608 duration fix is out of scope.
- **Word alignment (fork-only)**: the alignment downloader gets the same exact-size, resumable, validated, owned and cancellable behavior. The catalog's `min_bytes` threshold (`audio/word_alignment/catalog.rs:36-50`) becomes an exact size. The repo URL is pinned to a commit.
- **Shared helper**: one small backend module provides download ownership and cancellation for all three engines, plus the exact-size resumable transfer for Parakeet and word alignment (design D1).
- **IPC** (command and event names unchanged; payload/return changes listed in design D6):
  - **BREAKING (frontend-internal)**: `parakeet_cancel_download`, `whisper_cancel_download` and `cancel_alignment_download` return `"cancelled" | "pending"` instead of `()`.
  - The backend `status: "cancelled"` progress event becomes the single source of cancel state in the UI. Parakeet already has this event. Whisper's `model-download-progress` gains an optional `status: "cancelled"` field, which is additive.
  - `status: "completed"` (never `progress >= 100`) marks a Parakeet download complete.
  - The frontend `ModelStatus` type changes from `{ Downloading: number }` to `{ Downloading: { progress: number } }`. That is what Rust already serializes. This removes the drift documented at `frontend/src/lib/ipc/models.ts:54-57`.
- Retry stays disabled in the UI until cleanup ends. No polling and no timers are added.

## Capabilities

### New Capabilities
<!-- None. The shared download helper is an implementation detail; its behavior is specified per engine below. -->

### Modified Capabilities
- `parakeet-engine`: the download, resume, validation, cancellation and stall-timeout requirements change to exact-size validation, validated `Range` resume, preserved partials and owned cancellation with a `cancelled`/`pending` result. A new requirement keeps the model catalog usable during native loading.
- `whisper-engine`: the download requirement adds User-Agent and size/header validation before Available. The cancellation requirement adds owned cancellation with a bounded acknowledgement and blocks retry until cleanup ends.
- `ctc-word-alignment`: the "Alignment model catalog and download" requirement adds exact-size validation, validated resume, pinned source and owned cancellation.

## Impact

- **Backend**:
  - `frontend/src-tauri/src/parakeet_engine/{parakeet_engine.rs,commands.rs,mod.rs}`
  - `frontend/src-tauri/src/whisper_engine/{whisper_engine.rs,commands.rs}`
  - `frontend/src-tauri/src/audio/word_alignment/{catalog.rs,download.rs,commands.rs}`
  - new `frontend/src-tauri/src/model_download/` module, registered in `lib.rs`
  - No new crates: `tokio-util` (`CancellationToken`), `thiserror`, `crossbeam` and `tempfile` are already in `frontend/src-tauri/Cargo.toml:129-164,227`.
- **Frontend**:
  - `frontend/src/lib/ipc/models.ts`, `frontend/src/lib/{parakeet,whisper}.ts`, `frontend/src/services/alignmentService.ts`
  - `frontend/src/components/{ParakeetModelManager,WhisperModelManager,ModelDownloadProgress,WordAlignmentSettings}.tsx`
  - `frontend/src/contexts/OnboardingContext.tsx`, `frontend/src/components/onboarding/steps/DownloadProgressStep.tsx`, `frontend/src/components/shared/DownloadProgressToast.tsx`
- **On-disk behavior**:
  - Cancelling a Parakeet or alignment download now keeps its partial files for resume. Today the directory is removed.
  - A partial Parakeet directory is reported as `Corrupted`, and the existing "Re-download" button resumes it.
  - A Whisper partial is still removed.
- **Tests**:
  - Upstream's loopback-HTTP Parakeet tests (`mod tests`, upstream v0.4.1 `parakeet_engine.rs:1233-2016`) and Whisper tests (upstream `9f24062` `whisper_engine.rs:1275+`) are ported.
  - Equivalent word-alignment tests are added, plus IPC wrapper tests under `frontend/tests/lib/ipc/`.
- **Out of scope**:
  - `summary/summary_engine/model_manager.rs`, which has the same single-flag pattern (`:121-124`, `:808`)
  - upstream #767's `ensure_onnx_runtime_available` in `parakeet_engine/model.rs`
  - Whisper resumable download (no exact sizes in `WHISPER_MODEL_CATALOG`)
  - checksums and temp-file-plus-rename, which upstream does not do either
