# Design

## Context

See `proposal.md` for the motivation and the full list of items. This change bundles 9 independent upstream ports. They share no code, so each one is designed, tested and committed on its own (see `tasks.md`). The fork-specific facts below shape how each port differs from the upstream patch:

- **Summary chunking (#603).**
  - `chunk_text` (`summary/processor.rs:193-258`) is unchanged from upstream v0.4.0, so the upstream patch applies verbatim (`git apply --check` passed).
  - Its only caller is `generate_meeting_summary` (`:393`), which calls `chunk_text(text, token_threshold - 300, 100)`. A 100-token overlap is about 286 characters, so any snap-back past the last `". "` that is longer than about 286 characters drops text.
- **Claude response parsing (#694).**
  - The fork routes the request through `crate::llm::send_with_retry` (`summary/llm_client.rs:286-290`), with a 300 s request timeout (`llm/client.rs:7`).
  - Upstream's patch touches only the response struct, `max_tokens`, and the response read, so it ports without touching the retry layer.
  - The Claude request sends no `thinking` field, so models that think by default (Opus 5.5, Sonnet 5.5, and later) return a `thinking` block before the `text` block.
  - The model list comes from `/v1/models` (`anthropic/anthropic.rs:74-130`) and falls back to `FALLBACK_MODELS` (`:41-46`), so any model the account can list is selectable.
- **HE-AAC decoding (#608 and #737).** One Symphonia decoder (`audio/decoder.rs:446`) serves:
  - import (`audio/import.rs:364`, and the fallback at `:168`);
  - retranscription (`audio/retranscription.rs:188`);
  - the batch-diarization fallback, used when ffmpeg is unavailable or the channel layout is unknown (`audio/diarization/batch/orchestrator.rs:319`);
  - the eval binaries (`bin/diarize_eval.rs:93`, `bin/online_eval.rs:333`).

  The main batch-diarization path decodes through ffmpeg at `-ar 16000` (`audio/diarization/batch/pcm.rs:173`). ffmpeg supports SBR, so that path is already correct. For an HE-AAC import, the ASR timeline is therefore 2x compressed compared with the diarization timeline and with the webview audio player.
  - The retranscription metadata writer (`retranscription.rs:1046-1089`) already writes `duration_seconds` when it creates a new `metadata.json`. When the file already exists, it updates `retranscribed_at`, `status` and `transcript_file` and removes `detected_summary_language`, but it does not touch the duration (`:1060-1068`).
- **Live confidence gate (#681).** The fork's worker returns `(transcript, tokens_opt, confidence_opt, is_partial)` (`audio/transcription/worker.rs:258`), and on emit pushes the update to the live-alignment queue (`:386-398`). Upstream's worker has neither the tokens nor the queue, so the patch does not apply and the fix is re-implemented by hand. The low-confidence `else if` log branch (`:401-406`) becomes dead once the gate is gone.
- **VAD init failure (#767, pipeline part).** Two facts shape the fix:
  - `AudioPipelineManager::start` already returns `anyhow::Result<()>` (`audio/pipeline.rs:1306-1318`), and `RecordingManager::start_recording` already propagates it with `?` (`audio/recording_manager.rs:121-132`).
  - Both lifecycle entry points map the error to `"Failed to start recording: {e}"` and return it to the frontend (`audio/recording/lifecycle.rs:202-205,429-432`).

  So only `AudioPipeline::new` (`pipeline.rs:715`) panics. One more problem: `RecordingManager::start_recording` sets `state.start_recording()` (`recording_manager.rs:83`) before the pipeline starts, and `AudioPipelineManager::start` publishes the audio sender into the shared state (`pipeline.rs:1336`) before it builds the pipeline.
- **Analytics deadlock (#784).** The fork's `end_session` (`analytics/analytics.rs:208-224`) and `track_event` (`:134-188`, the session lock is at `:166`) match upstream's pre-fix code, and `posthog-rs` is the same version (0.3.7, which has `api_endpoint` and `request_timeout_seconds`). The upstream fix and its local-TCP-listener test therefore port verbatim.
- **Frontend.**
  - The fork wraps IPC rejections in `IpcError` (`frontend/src/lib/ipc/core.ts:15-31`). The rejection is an `Error`, and its `message` is the Rust error string, which for a start failure is `"Failed to start recording: <cause>"`.
  - Frontend unit tests run under `bun test tests/` with no DOM library (`frontend/package.json:30`), so only pure functions can be unit-tested.
- **CI.** `whisper-rs-sys` 0.15.0 forwards every `GGML_*`, `WHISPER_*` and `CMAKE_*` env var to CMake as `-D` (`~/.cargo/registry/src/.../whisper-rs-sys-0.15.0/build.rs:279-287`). It emits no `cargo:rerun-if-env-changed` for those vars, though, so an existing `rust-cache` keeps reusing the old native build. The Windows builds are:
  - `build-windows.yml` (rust-cache key `windows-x64-vulkan-v2`, `:507`);
  - `build.yml`, reused by `build-test.yml` and `release.yml` (key `${{ inputs.platform }}-${{ inputs.target }}-vulkan-v2`, `:114`);
  - `build-devtest.yml` (key `${{ matrix.platform }}-${{ matrix.target }}`, `:100`).

  The `llama-helper` sidecar built in the same jobs (`llama-cpp-sys-2` 0.1.146) already defines `GGML_NATIVE=OFF` unless `target-cpu=native` (its `build.rs:557-563`), so a job-level `GGML_NATIVE=OFF` does not change it.

  Six workflows pin `pnpm/action-setup@v4` to `version: 8` and run a plain `pnpm install`: `build-devtest`, `build-linux`, `build-macos`, `build-windows`, `build`, and `pr-main-check`. The lockfile is `lockfileVersion: '9.0'`.

## Goals / Non-Goals

**Goals:**
- Each item ships as one self-contained commit with its own regression test where the behavior is unit-testable.
- Where the fork's code still matches upstream, ports stay byte-close to the upstream patch (#603, #694, #608, #784), so a later upstream merge conflicts as little as possible.
- No IPC contract change: command names, argument shapes and return types stay the same.

**Non-Goals:**
- Making the live pipeline's start transactional beyond the state reset (see D5).
- Adding a confidence-based or hallucination filter to replace the removed gate.
- Re-processing meetings that were already imported with the wrong duration. Retranscribing such a meeting fixes it, and nothing does that automatically.
- Adding DOM or component tests to the frontend test pipeline.

## Decisions

### D1: Chunking — apply the upstream patch verbatim

The upstream patch:
- remembers `emitted_end_char`;
- accepts a sentence or word boundary only if the prefix before it is longer than `overlap_chars`;
- starts the next chunk at `emitted_end_char.saturating_sub(overlap_chars).max(start_char + 1)`.

The `.max(start_char + 1)` guarantees forward progress even when the overlap is as large as the chunk size. Accepting a boundary only beyond the overlap guarantees that the next start is never before the current start. Together these give the "no text dropped" and "terminates" scenarios in `specs/summary-service/spec.md`.

*Alternative considered:* keep the fixed `step` and refuse to snap back further than `overlap_chars`. Rejected: it still emits a chunk that cuts mid-word whenever no boundary falls inside a short window, and it diverges from upstream.

### D2: Claude — `type` + `Option<String>` text, pick the first `type == "text"` block, `max_tokens` 8192

`ClaudeChatContent` becomes `{ #[serde(rename = "type")] kind: String, text: Option<String> }`. A helper on `ClaudeChatResponse` returns the first block with `kind == "text"` and `Some(text)`.

Upstream picks the first block that has any `text`. That works today, because only `text` blocks carry a `text` field. Keying on `type` as well (the caller's requirement) means a future block type that happens to carry `text` cannot be mistaken for the answer.

`kind` is a `String`, not an enum, so unknown block types (`redacted_thinking`, server tool blocks) still deserialize. The error for a response with no text block becomes `"No text content in LLM response"`, matching upstream.

**`max_tokens` = 8192 is safe for every Claude model the app can list today.** Checked on 2026-10-02 against the models reference in the Claude API skill:
- Every remaining model has an output cap of at least 32K. Opus 4 and 4.1 cap at 32K; Sonnet 4, Sonnet 4.5, Haiku 4.5 and Opus 4.5 at 64K; the 4.6+ and 5.x models at 128K.
- The only listed model whose output cap was below 8192, Claude 3 Haiku (4096), retired on 2026-04-19, as did the Claude 3.5 models (cap 8192).

8192 is a shared budget: it covers the thinking tokens and the answer. At ~50-100 tokens/s, a full 8192-token response finishes well inside the 300 s request timeout, so the request stays non-streaming.

*Alternatives considered:*
- 16000 (the API skill's default for non-streaming calls): it would leave more room for thinking on long chunks, but diverges from upstream for little gain on summary-sized outputs.
- Sending `thinking: {type: "disabled"}`: it returns a 400 on Opus 5.5 and Sonnet 5.5, so it is not an option.

If thinking uses up the whole budget and no text block comes back, the user sees the "no text content" error. That is the same visible failure as today's parse error, but with a correct message.

### D3: HE-AAC — trust the decoded spec's rate; metadata write-back on retranscription

In `decode_audio_file_with_progress`, `sample_rate` becomes `mut`. Inside the existing first-packet block (`decoder.rs:570-583`), next to the channel-count correction, the code sets `sample_rate = spec.rate` and logs the correction when it differs. `duration_seconds` (`:622`) and the returned `DecodedAudio.sample_rate` then follow automatically, and so does every downstream resample (`to_whisper_format`, `extract_channels`).

`expected_samples` for progress (`:537-542`) still uses the container rate. That only affects the 10%-step progress callbacks: an HE-AAC file reports up to ~50% and then jumps to the forced final 100% (`:611-613`). This is acceptable and left as-is, to stay close to upstream.

`probe_audio_metadata`/`probe_header_metadata` (`:680-725`) stay header-only by contract, and the import preview duration (`import.rs:157-170`) comes from the container `n_frames / sample_rate`, which is already consistent for HE-AAC in MP4. Neither changes.

`write_retranscription_metadata` adds `obj.insert("duration_seconds", json!(duration_seconds))` in the existing-file branch. This matches #737 exactly, and so does its test, including the existing `detected_summary_language` removal that the fork already does.

The upstream `vad.rs` hunk from #608 is not ported. It fixes a double-counted offset in upstream's `ContinuousVadProcessor`, a code path the fork replaced with Silero v6 and a rolling buffer. Batch-diarization timing (ffmpeg) is unchanged.

### D4: Live worker — emit any non-empty transcript; keep confidence only in logs

The fix adds `fn should_emit_transcript(text: &str) -> bool { !text.trim().is_empty() }` to `worker.rs`, as upstream does. The code then:
- deletes `confidence_threshold` and `meets_threshold` (`:259-276`);
- changes the emit condition to `if should_emit_transcript(&transcript)`;
- removes the now-unreachable low-confidence `else if` branch (`:401-406`);
- drops `threshold=` from the result log line and keeps `confidence=`.

The emitted `TranscriptUpdate.confidence` is unchanged (`confidence_opt.unwrap_or(0.85)`, `:351`).

*Alternatives considered:*
- Fix Whisper's confidence (for example, the mean token probability): it changes a value the import and online-diarization paths also read, and is a larger, separate change.
- Lower the threshold: it would still drop one-word replies in Cyrillic or CJK, where bytes per character differ, so it treats the symptom only.

This aligns the live path with import and retranscription, which never gated on confidence (`import.rs:572-597`).

### D5: VAD failure — `AudioPipeline::new` returns `Result<Self>`; publish the sender only after success; reset state on failure

- `AudioPipeline::new` returns `anyhow::Result<Self>`. Each VAD creation becomes `ContinuousVadProcessor::new(...).map_err(|e| anyhow!("Failed to initialize voice activity detection for {mic|system audio}: {e}"))?`, keeping the `error!` log.
- In `AudioPipelineManager::start`, `state.set_audio_sender(...)` moves to after `AudioPipeline::new(...)?` succeeds, as upstream does. Capture callbacks then never see a sender whose receiver was dropped.
- In `RecordingManager::start_recording`, a pipeline start error calls `self.state.stop_recording()` before it is returned. Upstream does the same, so the shared state does not report "recording" after a failed start.
- The error reaches the frontend through the existing lifecycle `map_err`, as `"Failed to start recording: Failed to initialize voice activity detection for microphone: <cause>"`.

*Alternative considered:* port upstream's `RecordingStartError` enum and move `recording_saver.start_accumulation` after the pipeline start. Rejected for now:
- the enum exists in upstream only to tag ONNX-runtime errors for its runtime-installer UI, which is out of scope;
- moving `start_accumulation` changes `recording_saver.rs`'s API.

The cost is in D5's risk below.

### D6: Analytics — take the session out of the lock, then track

Port upstream's change: `let session = { self.current_session.lock().await.take() };`, then call `track_event` with no guard held. Port upstream's `ending_session_completes_and_emits_an_event` test unchanged. It needs `tokio::net::TcpListener`, which the fork already has through `tokio = { features = ["full"] }`. The PostHog key, event dedupe and consent behavior stay untouched.

### D7: Frontend — a pure message helper for the alerts; plain `div`; logo `<button>`

- **Alert message.** Add `formatRecordingStartError(error: unknown): string` to a new `frontend/src/lib/recording-start-error.ts`. It returns `` `Failed to start recording.\n\n${message}` `` and takes the message from `normalizeMessage` in `lib/ipc/core.ts`, falling back to the plain sentence when the message is empty. Both `alert(...)` sites use it. It is a separate module, not inline code, because two call sites share it and `bun test` can only test pure functions. It gets a `tests/lib/recording-start-error.test.ts`.
- **Homepage wrapper.** `app/page.tsx` replaces `<motion.div initial/animate/transition className=...>` with `<div className="flex flex-col h-screen bg-gray-50">` and drops the `framer-motion` import. Nine other components still use `framer-motion`, so the dependency stays.
- **Logo.** In `Logo.tsx`, the expanded trigger becomes `<button type="button" aria-label="About Meetily" className="... w-full">`, keeping the current classes. `w-full` is needed because a block-level `<button>` shrinks to its content, unlike the `block` `<span>` it replaces. The collapsed button gets the same `type`/`aria-label`.

### D8: CI — env var plus cache-key bump plus a pre-bundle CMakeCache check; pnpm 9.15.9 with `--frozen-lockfile`

- **`GGML_NATIVE`.** Set `GGML_NATIVE: "OFF"` at job `env:` level in `build-windows.yml`, and as a Windows-only `$GITHUB_ENV` write in `build.yml` and `build-devtest.yml`, whose jobs are cross-platform matrices.

  This uses the plain env var, not upstream's `CMAKE_PROJECT_INCLUDE` hook. The fork's `whisper-rs-sys` 0.15.0 forwards `GGML_*` directly, and with `GGML_NATIVE` OFF on a non-cross-compile, ggml's own defaults keep AVX/AVX2/FMA/F16C ON and leave the `GGML_AVX512*` options OFF. This preserves the x64 baseline the app is already built for.
- **Cache key.** Bump each Windows `rust-cache` `key` with a `-portable-v1` suffix. Without the bump, a cached `whisper-rs-sys` build output is reused, because no `rerun-if-env-changed` covers `GGML_*`.
- **CMakeCache check.** Add upstream's `.github/verify-portable-ggml.cjs`. It requires `GGML_NATIVE:BOOL=OFF` and `GGML_AVX512{,_VBMI,_VNNI,_BF16}:BOOL=OFF` in every `whisper-rs-sys-*/out/build/CMakeCache.txt`.

  The check runs through tauri-action's bundle hook (a `--config` override that sets `beforeBundleCommand`, as upstream does) or as a step after the build. A post-build step is simpler, but a failure there only flags the run; it does not stop a bad artifact from being uploaded.

  Decision: use the `beforeBundleCommand` override in `build-windows.yml` and `build.yml` (the release path). `build-devtest.yml` gets only the env var and the key bump, since its artifacts are not released.
- **pnpm.** Set `version: 9.15.9` (same as upstream) in all six `pnpm/action-setup` blocks, and use `pnpm install --frozen-lockfile` at all six install sites.

  Before committing, verify locally with pnpm 9 that the lockfile matches `package.json`: `npx -y pnpm@9.15.9 install --frozen-lockfile` in `frontend/`. The local pnpm is 11.x, so pnpm 9 is invoked explicitly. If it fails, regenerate the lockfile with pnpm 9.15.9 in the same commit.

## Risks / Trade-offs

- **[Risk] Removing the live confidence gate lets more short Whisper hallucinations reach the live transcript** ("Thank you.", "you", "Субтитры..."), mainly on silence the VAD lets through.
  → Mitigation: VAD (Silero v6, `VadConfig::live()`) already gates silence before ASR. Whisper's repetition filter still runs inside the engine (`clean_repetitive_text`, `whisper_engine/whisper_engine.rs:391`, applied at `:690`). The same audio already produces these lines in import and retranscribe, so live now matches what the saved meeting shows. If it turns out to be noisy, a targeted hallucination filter is a follow-up, not a confidence threshold.
- **[Risk] With `auto_save`, `recording_saver.start_accumulation` creates the meeting folder** (`recording_saver.rs:189-209`) before the pipeline starts. A VAD init failure therefore leaves an empty meeting folder on disk.
  → Accepted: the folder has no `metadata.json` and no DB row, so it never appears as a meeting. VAD init failure is rare (a missing or corrupt Silero model). Moving folder creation after the pipeline start is upstream's larger restructure, deferred.
- **[Risk] Previously imported HE-AAC meetings keep half-length durations and 2x-compressed timestamps.**
  → Retranscribing the meeting rewrites both, via D3's metadata write-back and the new transcript rows. The tasks include a manual check.
- **[Risk] The fixture is a binary file fetched from upstream during apply.** If upstream rewrites history, it could change.
  → Fetch it from the immutable `v0.4.1` tag and check its size (32,131 bytes) in the task's `verify:` clause.
- **[Risk] `--frozen-lockfile` fails CI if `package.json` and `pnpm-lock.yaml` have drifted.**
  → Verified locally with pnpm 9.15.9 before the CI commit (D8). Upstream's commit had to regenerate its lockfile; the fork's may not need to.
- **[Risk] With `GGML_NATIVE` OFF, whisper.cpp loses AVX-512 kernels on CPUs that have them,** so Whisper may be slightly slower on those machines.
  → Accepted: portability matters more than peak speed. The AVX2 baseline is kept, and the Vulkan GPU path, which is the Windows release feature, is unaffected.
- **[Trade-off] Claude `max_tokens` 8192 caps very long single-chunk summaries** when the model also spends tokens on thinking.
  → The summary chunker already caps input per call, and summary outputs are far below 8K tokens in practice. Raising the cap later is a one-line change.

## Migration Plan

No data migration. Each task group is a separate commit and can be reverted on its own:
- The CI commit only affects workflow runs.
- The analytics, decoder and worker commits are pure Rust behavior changes.
- The frontend commit is UI-only.

The first Windows CI run after the CI commit rebuilds whisper.cpp from scratch because of the new cache key, so expect a longer build.
