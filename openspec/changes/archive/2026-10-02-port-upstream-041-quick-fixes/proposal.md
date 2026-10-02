# Proposal

## Why

Upstream Meetily v0.4.1 fixed several bugs that are also present in this fork, which diverged from upstream at v0.4.0 (merge `0281737`). Six of them lose or corrupt user data without telling the user:
- long-transcript summaries skip text;
- Claude summaries fail on models that think by default;
- HE-AAC imports come out at half length;
- short live utterances ("Да", "Yes") are dropped;
- analytics hangs when the webview unloads;
- a VAD init failure panics the app instead of failing the start.

This change ports those fixes, plus small UI, accessibility and CI fixes from the same release, adapted to the fork's code. The fork's code has moved on since v0.4.0: it uses typed IPC, has split the recording commands, has bounded channels and per-channel VAD, and routes LLM calls through `llm::send_with_retry`.

## What Changes

- **Summary chunking keeps every character** (upstream #603, merge `84370b3`). `chunk_text` (`frontend/src-tauri/src/summary/processor.rs:193`) computes a fixed `step = chunk_size - overlap` (`:221`) and advances `start_char += step` (`:250`), even though the chunk's end can snap back to the last `". "`/`" "` boundary (`:234-240`). When the snap-back is longer than the overlap, the text in between goes into no chunk. Fix: start the next chunk at the end that was actually emitted, minus the overlap. Accept a boundary only if it lies beyond the overlap. Port upstream's 3 tests, including the `LOST_MARKER` test.
- **Claude responses with thinking blocks parse** (upstream #694, merge `4c450e1`). Today `ClaudeChatContent { text: String }` (`summary/llm_client.rs:61-63`) is required on every block, and the parser takes `.content.first()` (`:343-348`). Both break on models whose first block is `thinking`, such as Opus 5.5 and Sonnet 5.5, which think by default. Fix: make `text` an `Option<String>`, add a `type` field, and use the first `type == "text"` block. Raise the Claude `max_tokens` from 2048 to 8192 (`:259`), because thinking tokens count against it. HTTP still goes through `crate::llm::send_with_retry`, unchanged.
- **HE-AAC imports keep their real duration** (upstream #608, merge `61c4cdf`, plus the duration part of #737, merge `9f24062`). `decode_audio_file_with_progress` reads `sample_rate` from the container (`audio/decoder.rs:510`). It corrects only the channel count from the first decoded packet (`:570-582`) and computes `duration_seconds` from the container rate (`:622`). Symphonia has no SBR support: it decodes HE-AAC's AAC-LC core at half the declared rate, so the audio is treated as 2x speed and half its length. Fix: take the rate from the first decoded packet's spec. Port the upstream test and its 32 KB fixture `frontend/src-tauri/tests/fixtures/he_aac_48k_5s.m4a`. Retranscription also writes the corrected `duration_seconds` back into an existing `metadata.json` (`audio/retranscription.rs:1060-1068` does not today). Skip the PR's `vad.rs` hunk: the fork's Silero v6 VAD is different code.
- **Live transcription keeps short utterances** (upstream #681, merge `7c94aa6`). The live worker drops Whisper/Provider results with confidence below 0.3 (`audio/transcription/worker.rs:259-278`). Whisper's "confidence" is really a length heuristic, `len_bytes/100 + 0.1` (`whisper_engine/whisper_engine.rs:652-658`), so any segment under about 20 bytes is dropped live, yet kept by import and retranscribe, which do no gating. Fix: emit every non-empty transcript (upstream's `should_emit_transcript`) and keep confidence for logging only. Re-implemented by hand, because the fork's worker also carries tokens and the alignment queue.
- **VAD init failure fails the recording start instead of panicking** (idea from #767, merge `5ed1ba9`, `pipeline.rs` part only). `AudioPipeline::new` calls `panic!("VAD processor creation failed")` for the mic (`audio/pipeline.rs:763`) and system (`:775`) VAD. Change it to return `Result<Self>`, propagate the error through `AudioPipelineManager::start` (already returns `Result<()>`, `:1306`) and `RecordingManager::start_recording` (`audio/recording_manager.rs:121`), and reset the recording state on that failure. The start command then returns a readable error instead of crashing the process.
- **Ending an analytics session no longer deadlocks** (upstream #784, merge `c7bdb3b`, the `end_session` fix only). `AnalyticsClient::end_session` (`analytics/analytics.rs:208-224`) holds the `current_session` tokio `Mutex` while it awaits `track_event`, and `track_event` locks the same mutex again (`:166`). Tokio's mutex is not reentrant, so the call never returns, and every later event that needs the session hangs too. It fires from `Analytics.cleanup()` on webview `beforeunload` and on provider unmount (`frontend/src/components/AnalyticsProvider.tsx:123,134`). Fix: take the session out of the mutex and release the lock before tracking. Port the upstream test.
- **Recording start errors show their cause; the Record button stops jumping** (upstream #779, merge `e4cc94b`, the UI pieces only).
  - The two `alert('Failed to start recording. Check console for details.')` calls (`frontend/src/hooks/useRecordingStart.ts:194,282`) show the actual error message instead.
  - Remove the homepage `motion.div` entrance animation (`frontend/src/app/page.tsx:212-216,290`) that shifts the Record button 20px on every mount.
- **The expanded logo is a keyboard-accessible button** (from upstream #794). The About dialog trigger for the expanded sidebar is a `<span>` (`frontend/src/components/Logo.tsx:21-24`). Change it to a `<button type="button">` with an `aria-label`, as the collapsed variant already is.
- **CI builds portable Windows Whisper and uses the locked frontend dependencies** (ideas from upstream commits `41daaa5` and `9a3193d`; no spec).
  - Set `GGML_NATIVE=OFF` for every Windows build, so whisper.cpp is not tuned to the CI runner's CPU (for example AVX-512). `whisper-rs-sys` 0.15.0 forwards every `GGML_*` env var to CMake (its `build.rs:279-287`) but declares no `rerun-if-env-changed` for them, so the Windows `rust-cache` keys must be bumped as well.
  - Fail the build before bundling unless whisper's `CMakeCache.txt` shows `GGML_NATIVE` and the `GGML_AVX512*` flags OFF.
  - Move the CI pnpm from 8 to 11.20.0 (the version that produces `frontend/pnpm-lock.yaml` locally) on Node 22, and run `pnpm install --frozen-lockfile`. The ProseMirror overrides move from `package.json` to `pnpm-workspace.yaml`, where pnpm 10+ reads them. (Revised during apply; see design D8.)

## Capabilities

### New Capabilities
- `audio-file-decoding`: decoding a stored or imported audio file into samples. The reported sample rate and duration come from the decoded stream, not from container metadata. Import, retranscription, and the batch-diarization Symphonia fallback all use this one decoder (`audio/decoder.rs`). No current spec owns it: `audio-engine` covers live capture, and `recording-start-time` covers only the import `started_at`.

### Modified Capabilities
- `summary-service`: one requirement changes and one is added.
  - "Transcript chunking for large inputs" gains a no-text-dropped guarantee and a forward-progress guarantee.
  - A new requirement, "Claude summaries use the response's text block": the summary is taken from the response's text block, thinking blocks are skipped, and the output budget leaves room for reasoning tokens.
- `analytics`: "Session management" gains a scenario: ending a session completes, and later events are not blocked.
- `audio-engine`: "Audio recording start/stop" gains scenarios for a pipeline/VAD initialization failure. The start fails with a readable error and no panic, the recording state is reset, and the UI shows the backend's message.
- `source-labeled-transcription`: adds a requirement that the live transcription worker emits every non-empty transcript, whatever the engine's confidence value.
- `source-labeled-retranscription`: adds a requirement that retranscription writes the duration decoded this run into the meeting's existing metadata.

## Impact

- **Rust code**:
  - `frontend/src-tauri/src/summary/processor.rs` and `summary/llm_client.rs`.
  - `audio/decoder.rs` and `audio/retranscription.rs`.
  - `audio/transcription/worker.rs`.
  - `audio/pipeline.rs` and `audio/recording_manager.rs`.
  - `analytics/analytics.rs`.
- **New binary test fixture**: `frontend/src-tauri/tests/fixtures/he_aac_48k_5s.m4a` (32,131 bytes, copied from upstream `v0.4.1` during apply).
- **Frontend code**: `frontend/src/hooks/useRecordingStart.ts`, `frontend/src/app/page.tsx` (drops its `framer-motion` import; the package stays, because other components use it), and `frontend/src/components/Logo.tsx`.
- **CI**: `.github/workflows/build-windows.yml`, `build.yml` (also called by `build-test.yml` and `release.yml`), `build-devtest.yml`, `build-linux.yml`, `build-macos.yml`, `pr-main-check.yml`, and a new `.github/verify-portable-ggml.cjs`.
- **No IPC contract change**: command names and signatures stay the same. Only the text of the start-failure error changes.
- **Behavior and data effects users will notice**:
  - HE-AAC meetings imported or retranscribed before this change keep their wrong timestamps until they are retranscribed. Today the ASR timeline of such a meeting is compressed 2x compared with batch diarization, which decodes through ffmpeg at the true rate (`audio/diarization/batch/pcm.rs:173`, `-ar 16000`), and compared with the audio player. After the fix, the two timelines agree.
  - Cached summaries made with the old chunker are not invalidated. Regenerating produces the corrected summary.
  - More short live hallucinations ("Thank you.", "you") may now reach the live transcript (see design).
- **Non-goals (out of scope)**:
  - Upstream VAD segmentation changes #679/#771: the fork's Silero v6 VAD and live merge already cover them, and porting them would invalidate the diarization eval baselines.
  - The dynamic ONNX runtime loader and `ensure_onnx_runtime_available` (#767).
  - #639: already fixed by fork commit `ea49018`.
  - The #744 split-view UI.
  - The PostHog key rotation, event dedupe, and opt-in changes in #784.
  - The macOS permission check and the system-audio-only mode in #779.
  - Upstream's `RUSTFLAGS=-C target-cpu=x86-64-v2` from `41daaa5`.
  - The stale Claude fallback model ids (`anthropic/anthropic.rs:42-45`, `frontend/src/contexts/ConfigContext.tsx:374`).
  - A version bump.
