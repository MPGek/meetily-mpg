# Tasks

Each group from 1 to 9 is one commit (`fix(...)`/`ci: ... (openspec port-upstream-041-quick-fixes)`) and lands its own tests. Upstream clone used below: `$UP` = `C:/Users/VASILI~1.KOT/AppData/Local/Temp/claude/C--Users-vasiliy-kotov-Work-Own-meetily-mpg/3b2f5680-50c7-4d8f-9bac-8b1efb06005f/scratchpad/upstream` (blobless; needs network for blobs). Rust commands run from the repo root; frontend commands run from `frontend/`.

## 0. Baseline

- [x] 0.1 Record the pre-change baselines:
  - `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands` (pass/fail/ignored counts)
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^warning"` (the operations doc says 32)
  - `bun test tests/` and `pnpm exec tsc --noEmit -p .` in `frontend/`

  verify: all four commands ran and their counts are noted in the PR description.
  - Note (2026-10-02): baseline at dbe3ac4: Rust 515 passed / 0 failed / 9 ignored; clippy 32 warnings; `bun test tests/` 92 pass; `tsc --noEmit` clean.
- [x] 0.2 Re-check every `file:line` cited in `proposal.md` and `design.md` against HEAD (they were taken on 2026-10-02 at `f9919e4`), and fix any drifted reference in the artifacts before editing code; verify: each cited line still holds the quoted code (`grep -n` spot checks for `step = chunk_size_chars`, `max_tokens: 2048`, `let sample_rate = track`, `confidence_threshold`, `panic!("VAD processor creation failed`, `let mut session_guard`, `Check console for details`, `<motion.div`, `<span className="text-lg`).

## 1. Summary chunking keeps every character (#603)

- [x] 1.1 Apply upstream's patch: `git -C $UP diff 84370b3^1 84370b3 -- frontend/src-tauri/src/summary/processor.rs > <scratch>/603.patch` and `git apply <scratch>/603.patch`. It replaces the fixed `step` (`summary/processor.rs:221`, `:250`) with `emitted_end_char`, accepts a boundary only if it lies beyond `overlap_chars`, and computes the next start as `.saturating_sub(overlap_chars).max(start_char + 1)`. It also adds `chunk_text_preserves_content_after_early_sentence_boundary`, `chunk_text_keeps_unicode_boundaries` and `chunk_text_progresses_when_overlap_matches_window`; verify: `git apply --check` succeeds before applying, and `cargo test -p meetily --lib summary::processor` passes, including the 3 new tests.
- [x] 1.2 Add one fork test, `chunk_text_covers_every_character`: on a ~5,000-char text with sentence boundaries placed so that the snap-back exceeds the overlap, the chunks taken in order and with overlaps removed contain every character of the input; verify: `cargo test -p meetily --lib summary::processor::tests::chunk_text_covers_every_character` passes, and fails when run against the pre-1.1 `chunk_text` (check with `git stash` of 1.1 or by reasoning on the old `step` arithmetic, noted in the commit message).
  - Note: on the pre-1.1 code the first 286-char window holds only the leading `a. `, so `chunk_text` emits `a. ` and jumps to char 257 via the fixed `step`, leaving chars 3..257 in no chunk; the test asserts that gap away.

## 2. Claude summaries skip thinking blocks (#694)

- [x] 2.1 In `summary/llm_client.rs`:
  - change `ClaudeChatContent` (`:61-63`) to `{ #[serde(rename = "type")] pub kind: String, pub text: Option<String> }`;
  - add `impl ClaudeChatResponse { fn first_text(&self) -> Option<&str> }`, returning the first block with `kind == "text"` and `Some(text)`;
  - replace the `.content.first()...text` read (`:343-349`) with `.first_text().ok_or("No text content in LLM response")?.trim().to_string()`;
  - change Claude `max_tokens: 2048` (`:259`) to `8192`, with upstream's comment that the budget is shared with thinking.

  Leave `crate::llm::send_with_retry` (`:286-290`) untouched. verify: `cargo check -p meetily` succeeds, and `git diff` shows no change to the `send_with_retry` call.
- [x] 2.2 Port upstream's 3 tests into a new `#[cfg(test)] mod tests` in `llm_client.rs`: `claude_response_skips_leading_thinking_block`, `claude_response_reads_plain_text_block` and `claude_response_without_text_block_returns_none` (`git -C $UP diff 4c450e1^1 4c450e1`). Add one more, `claude_response_ignores_non_text_block_carrying_text`: a block with `"type": "server_tool_result", "text": "x"` is skipped in favor of the later `"type": "text"` block; verify: `cargo test -p meetily --lib summary::llm_client` passes (4 tests).

## 3. HE-AAC decodes at the decoded rate; retranscription stores the real duration (#608, #737)

- [ ] 3.1 Copy the fixture from the immutable tag: `mkdir -p frontend/src-tauri/tests/fixtures && git -C $UP show v0.4.1:frontend/src-tauri/tests/fixtures/he_aac_48k_5s.m4a > frontend/src-tauri/tests/fixtures/he_aac_48k_5s.m4a`. Use a binary-safe redirect: Git Bash, not PowerShell `>`, which re-encodes the bytes. verify: the file size is exactly 32131 bytes (`wc -c`), and `git check-ignore` reports nothing for it.
- [ ] 3.2 In `audio/decoder.rs`, make `sample_rate` mutable (`:510`). Inside the first-packet `if sample_buf.is_none()` block (`:570-583`), next to the channel-count correction, set `sample_rate = spec.rate` and log `"Sample rate corrected: metadata={} actual={} (using actual)"` when they differ, as upstream does. Leave `expected_samples` (`:537-542`) and `probe_header_metadata` (`:686-725`) unchanged (design D3). Do NOT port the PR's `audio/vad.rs` hunk. verify: `cargo check -p meetily` succeeds; `git diff --stat` shows `decoder.rs` and no `vad.rs` change.
- [ ] 3.3 Port upstream's `test_decode_he_aac_uses_decoded_rate_not_container_rate` into `decoder.rs`'s `mod tests` (`:827`). It asserts `sample_rate == 24000`, `duration_seconds` within 0.5 s of 5.16, and a 16 kHz `to_whisper_format()` length of about 5.16 s; verify: `cargo test -p meetily --lib audio::decoder` passes, including the new test.
- [ ] 3.4 In `audio/retranscription.rs` `write_retranscription_metadata`, add `obj.insert("duration_seconds".to_string(), serde_json::json!(duration_seconds));` to the existing-file branch (`:1060-1068`). Port #737's test `retranscription_metadata_replaces_stale_duration_without_losing_existing_fields` into `mod tests` (`:1230`) (`git -C $UP diff 9f24062^1 9f24062 -- frontend/src-tauri/src/audio/retranscription.rs`); verify: `cargo test -p meetily --lib audio::retranscription` passes, including the new test.
- [ ] 3.5 Manual app check, recorded as a note under this task (the gap may stay open, as for earlier changes in `archived-with-open-manual-checks`): import an HE-AAC `.m4a` (for example the fixture, or a phone voice memo) in a release or dev build. Then confirm that the meeting duration matches the file, that a transcript row's timestamp matches the audio player position, and that retranscribing an older half-length HE-AAC meeting rewrites `metadata.json`'s `duration_seconds`; verify: the observed numbers are written in the note.

## 4. Live transcription keeps short utterances (#681)

- [ ] 4.1 In `audio/transcription/worker.rs`:
  - add `fn should_emit_transcript(text: &str) -> bool { !text.trim().is_empty() }`, with upstream's doc comment;
  - remove `confidence_threshold` and `meets_threshold` (`:259-276`) and change the emit condition (`:278`) to `if should_emit_transcript(&transcript)`;
  - drop `threshold=` from the result log (`:271-272`), keeping `confidence=`;
  - delete the low-confidence `else if` branch (`:401-406`).

  Keep the tokens, the `TranscriptUpdate.confidence` value (`:351`) and the alignment-queue push (`:386-398`) unchanged. verify: `cargo check -p meetily` succeeds, and `grep -n "confidence_threshold\|low-confidence" frontend/src-tauri/src/audio/transcription/worker.rs` returns nothing.
- [ ] 4.2 Add a `#[cfg(test)] mod tests` to `worker.rs` with upstream's `keeps_short_acknowledgements` (`"Yes"`, `"ok"`) and `drops_empty_and_whitespace_only`, plus `keeps_short_cyrillic_reply` (`"Да"`, `" Да."`); verify: `cargo test -p meetily --lib audio::transcription::worker` passes (3 tests).

## 5. VAD init failure fails the start instead of panicking (#767 idea)

- [ ] 5.1 In `audio/pipeline.rs`:
  - change `AudioPipeline::new` (`:715`) to return `anyhow::Result<Self>`;
  - replace both `panic!("VAD processor creation failed: {}", e)` arms (`:763`, `:775`) with an `error!` log plus `Err(anyhow!("Failed to initialize voice activity detection for microphone: {e}"))`, and the same for `system audio`;
  - wrap the final struct literal in `Ok(...)`.

  In `AudioPipelineManager::start`, call `AudioPipeline::new(...)?` and move `state.set_audio_sender(audio_sender.clone())` (`:1336`) to after it (design D5). verify: `cargo check -p meetily` succeeds, and `grep -n "panic!" frontend/src-tauri/src/audio/pipeline.rs` shows no VAD panic.
- [ ] 5.2 In `audio/recording_manager.rs` `start_recording` (`:65-155`), change the `self.pipeline_manager.start(...)?` call (`:121-132`) to an `if let Err(e) = ... { self.state.stop_recording(); return Err(e); }`, so a failed start leaves the state not recording; verify: `cargo check -p meetily` succeeds.
- [ ] 5.3 Add a unit test in `pipeline.rs`'s test module (or `recording_manager.rs`'s, wherever an `Arc<RecordingState>` can be built without devices): an `AudioPipelineManager::start` whose VAD creation fails returns `Err` with the "voice activity detection" text, does not panic, and leaves `state`'s audio sender unset.

  The failure can be forced by:
  - a sample rate `ContinuousVadProcessor::new` rejects, if there is one (check `audio/vad.rs`);
  - otherwise a `#[cfg(test)]` failure hook on the VAD constructor.

  If neither works without production-code contortions, record the reason under this task and rely on 5.4. verify: `cargo test -p meetily --lib audio::pipeline` passes, or a note explains the skip.
- [ ] 5.4 Manual check: temporarily make `ContinuousVadProcessor::new` return `Err` (local edit, not committed) and start a recording from the home page in `pnpm tauri:dev`. The app keeps running, the alert shows `Failed to start recording: Failed to initialize voice activity detection for microphone: ...`, and a second start attempt (after reverting the edit and restarting) succeeds; verify: the observed alert text is noted under this task. This depends on group 7.1 for the alert text; if group 7 lands later, check the error in the devtools console instead.

## 6. Ending an analytics session does not deadlock (#784)

- [ ] 6.1 In `analytics/analytics.rs` `end_session` (`:208-224`), replace the held `session_guard` with `let session = { let mut g = self.current_session.lock().await; g.take() };`, then `if let Some(session) = session { ... self.track_event(...).await?; }`. This is upstream's change; leave the PostHog key (`analytics/commands.rs`), event dedupe and consent untouched. verify: `git diff --stat` touches only `analytics/analytics.rs`.
- [ ] 6.2 Port upstream's `ending_session_completes_and_emits_an_event` tokio test and its `tokio::{io, net::TcpListener, time}` imports into `analytics.rs`'s `mod tests` (`:561`) (`git -C $UP diff c7bdb3b^1 c7bdb3b -- frontend/src-tauri/src/analytics/analytics.rs`); verify: `cargo test -p meetily --lib analytics::analytics` passes, and the new test fails with the "must not wait on its own session mutex" timeout when 6.1 is reverted locally.

## 7. Recording start errors show their cause; Record button stops jumping; logo is a button (#779 UI, #794)

- [ ] 7.1 Add `frontend/src/lib/recording-start-error.ts` exporting `formatRecordingStartError(error: unknown): string`. It returns `` `Failed to start recording.\n\n${message}` `` using `normalizeMessage` from `@/lib/ipc/core`, or just `Failed to start recording.` when the message is empty. Replace both `alert('Failed to start recording. Check console for details.')` calls (`frontend/src/hooks/useRecordingStart.ts:194`, `:282`) with `alert(formatRecordingStartError(error))`. Add `frontend/tests/lib/recording-start-error.test.ts`, covering an `IpcError` (message carried through), a plain string, an `Error`, and an empty message. If the import pulls in `@tauri-apps/api/core`, mock both `@tauri-apps/api/core` and `@tauri-apps/api/event`, per the operations-doc Bun gotcha. verify: `bun test tests/lib/recording-start-error.test.ts` passes, and `grep -rn "Check console for details" frontend/src` returns nothing.
- [ ] 7.2 In `frontend/src/app/page.tsx`, replace `<motion.div initial=... animate=... transition=... className="flex flex-col h-screen bg-gray-50">` (`:212-216`) and its closing tag (`:290`) with a plain `<div className="flex flex-col h-screen bg-gray-50">`, and remove `import { motion } from 'framer-motion';` (`:4`). Keep `framer-motion` in `package.json`, since other components use it. verify: `pnpm exec tsc --noEmit -p .` passes, and `grep -n "motion" frontend/src/app/page.tsx` returns nothing.
- [ ] 7.3 In `frontend/src/components/Logo.tsx`, change the expanded `DialogTrigger` child (`:22-24`) from `<span ...>` to `<button type="button" aria-label="About Meetily" className="<existing classes> w-full">`, keeping the inner `<span>Meetily</span>`. Add `type="button"` and `aria-label="About Meetily"` to the collapsed `<button>` (`:16`); verify: `pnpm exec tsc --noEmit -p .` and `pnpm exec next lint` report no new errors, and `grep -n "<span className=\"text-lg" frontend/src/components/Logo.tsx` returns nothing.
- [ ] 7.4 Manual UI check in `pnpm tauri:dev`:
  - navigating to the home page no longer slides the Record button in from 20px below;
  - the expanded sidebar logo can be focused with Tab and opens the About dialog with Enter or Space;
  - its pill width matches the old full width.

  verify: the observations are noted under this task.

## 8. CI: portable Windows Whisper build

- [ ] 8.1 Copy upstream's `.github/verify-portable-ggml.cjs` (`git -C $UP show 41daaa5:.github/verify-portable-ggml.cjs`). It requires `GGML_NATIVE:BOOL=OFF` and `GGML_AVX512{,_VBMI,_VNNI,_BF16}:BOOL=OFF` in every `target/<triple>/<profile>/build/whisper-rs-sys-*/out/build/CMakeCache.txt`. Do not copy `force-portable-ggml.cmake` (design D8). Copy upstream's `verify-portable-ggml.test.cjs` only if it runs under plain `node` with no extra dependencies; verify: `node .github/verify-portable-ggml.cjs` with no args exits 1 with the usage message; `node .github/verify-portable-ggml.test.cjs` passes, if copied.
- [ ] 8.2 Edit `.github/workflows/build-windows.yml`:
  - add `GGML_NATIVE: "OFF"` to the workflow or job `env:` (`:35`);
  - change the rust-cache `key: windows-x64-vulkan-v2` (`:507`) to `windows-x64-vulkan-v2-portable-v1`;
  - add a step before "Build Tauri app" (`:669`) that writes a temp Tauri config with `build.beforeBundleCommand = { script: "node .github/verify-portable-ggml.cjs target/x86_64-pc-windows-msvc/<profile>", cwd: $GITHUB_WORKSPACE }`, and append `--config <path>` to the tauri-action `args` (`:683`), as upstream's `windows-portability` step does.

  verify: `actionlint .github/workflows/build-windows.yml` reports no new findings (if actionlint is unavailable, `python -c "import yaml,sys; yaml.safe_load(open(sys.argv[1]))" .github/workflows/build-windows.yml`), and the diff matches design D8.
- [ ] 8.3 Edit `.github/workflows/build.yml`, which is reused by `build-test.yml` and `release.yml`:
  - in a Windows-only step (`if: contains(inputs.platform, 'windows')`), append `GGML_NATIVE=OFF` to `$GITHUB_ENV`;
  - change the rust-cache key (`:114`) to `${{ inputs.platform }}-${{ inputs.target }}-vulkan-v2${{ contains(inputs.platform, 'windows') && '-portable-v1' || '' }}`;
  - add the same `beforeBundleCommand` override for Windows only, appended to the tauri-action `args` (`:594`).

  Do the same in `.github/workflows/build-devtest.yml`, but only the env var and the key bump (`:100`), with no bundle check (design D8). verify: both files parse (actionlint or the yaml one-liner), and the non-Windows matrix legs' cache keys are unchanged.
- [ ] 8.4 After pushing, trigger `build-windows.yml` (workflow_dispatch) once; verify: the "Verified Windows Whisper CPU portability before bundling." line appears in the tauri-action log, and the run produces the MSI/NSIS artifacts. If CI cannot be run before archive, note this task as open.

## 9. CI: pnpm 9 with a frozen lockfile

- [ ] 9.1 Locally, in `frontend/`, run `npx -y pnpm@9.15.9 install --frozen-lockfile`. If it fails because the lockfile is out of date, run `npx -y pnpm@9.15.9 install` and include the regenerated `pnpm-lock.yaml` in this group's commit; verify: `npx -y pnpm@9.15.9 install --frozen-lockfile` exits 0.
- [ ] 9.2 Change `version: 8` to `version: 9.15.9` in the `pnpm/action-setup@v4` block of each of the six workflows:
  - `build-devtest.yml:70`
  - `build-linux.yml:111`
  - `build-macos.yml:72`
  - `build-windows.yml:477`
  - `build.yml:83`
  - `pr-main-check.yml:88`

  Change each `pnpm install` to `pnpm install --frozen-lockfile` (`build-devtest.yml:150`, `build-linux.yml:187`, `build-macos.yml:107`, `build-windows.yml:638`, `build.yml:434`, `pr-main-check.yml:99`); verify: `grep -n "version: 8" .github/workflows/*.yml` returns nothing; `grep -n "pnpm install" .github/workflows/*.yml` shows only `--frozen-lockfile` lines; and the `frontend-checks` job of `pr-main-check.yml` passes on the PR.

## 10. Integration checks

- [ ] 10.1 Run the full Rust suite: `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`; verify: no failures, and the pass count equals the 0.1 baseline plus the new tests from groups 1-6 (expected +14 or +15: 3+1, 4, 1+1, 3, 0-1 for 5.3, 1).
- [ ] 10.2 Run `cargo clippy -p meetily --all-targets --message-format=short`; verify: the warning count is at or below the 0.1 baseline, with no new warning in a file this change touched.
- [ ] 10.3 Run the frontend checks in `frontend/`: `bun test tests/` and `pnpm exec tsc --noEmit -p .`; verify: both pass, and the test count equals the 0.1 baseline plus the 7.1 tests.
- [ ] 10.4 Run `graphify update .` from the repo root; verify: it completes without errors (dirty `graphify-out/` files are expected).
- [ ] 10.5 Run `openspec validate port-upstream-041-quick-fixes --strict`; verify: it reports the change as valid.
