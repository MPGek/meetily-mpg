## 1. Per-channel session state (Rust)

- [x] 1.1 Add a per-session diarization stats holder with per-channel counters (chunks received, embedding attempts succeeded/failed, buffered embeddings, stable turns) plus a last-turn memo per channel, and verify with a unit test that microphone and system counters stay isolated when only one channel is fed
- [x] 1.2 Populate the holder from the online diarization chunk path on every outcome (successful embed, failed embed, buffered chunk, stable turn) and verify with a unit test that each counter advances exactly once per corresponding event
- [x] 1.3 Expose the per-channel turn-order health from the live turn registry and verify with a unit test that a turn published backwards in time flips only that channel's flag
- [x] 1.4 Create or reset the holder when a recording session starts and verify with a unit test that a second session begins from zero activity

## 2. Read-only snapshot command (Rust)

- [x] 2.1 Add a read-only command that returns the status snapshot (mode, model identifier and dimension, recognition threshold, prototype and binding counts, and the per-channel stats) and verify it returns populated data during a recording and an inactive shape when no diarization session is active
- [x] 2.2 Resolve the per-channel display state (unavailable, inactive/mono, deferred, accumulating, healthy, warning, error) and verify with a unit test that each state is produced for its documented condition, including zero stable turns in Efficient mode not resolving to a warning
- [ ] 2.3 Verify the snapshot path adds no per-chunk work and no per-chunk event emission, and that transcription, speaker assignment, and saved audio are unchanged with the lines active (compare the outputs of two recordings of the same audio, with and without the status block)

## 3. Frontend lines

- [ ] 3.1 Add the two-line diarization block immediately to the right of the animated recording indicator in the recording controls, hidden when not recording and when the mode is Off, and verify both conditions in a running app
- [x] 3.2 Render each line from its channel's snapshot only (chunk count, embedding success/failure, buffered embeddings, stable turns, last turn label or display name with attribution source and score) using compact tokens with truncation for long display names, and verify microphone/system isolation and name truncation
- [ ] 3.3 Apply the state styling so an embedding failure renders as an error and a turn-order regression renders as a warning, and verify both by inducing each condition
- [x] 3.4 Make the global context discoverable from the block (mode, model identifier and dimension, recognition threshold, prototype and binding counts) without adding a third line, and verify every displayed score is accompanied by the recognition threshold
- [x] 3.5 Sample the snapshot on the existing recording interval and pass it to the controls (no additional timer), resetting to zero on recording start, and verify no second interval is registered and a new recording starts from zero

## 4. Verification

- [x] 4.1 Run `cargo test` and `cargo check` in `frontend/src-tauri` and verify they pass with no new warnings in the touched modules
- [ ] 4.2 Run `pnpm run lint` in `frontend` and verify it passes
- [ ] 4.3 Manual end-to-end pass covering a mono session, a stereo session, Efficient mode, and Fast mode with an induced embedding failure, and verify each matches the spec's per-channel status, buffer-fill, model-activity, health, and degradation scenarios

## 5. Pipeline buffer fill (Rust)

- [x] 5.1 Publish, per channel, the cumulative audio seconds covered by buffered embeddings so the buffer is readable as work done rather than a bare count, and verify with a unit test that the figure is derived per channel
- [x] 5.2 Publish the voice-activity dispatch fill with its trigger threshold per channel, and verify with a unit test that the fraction is computed against the configured threshold and reported per channel
- [x] 5.3 Publish the pending speech accumulation (merged segment count and summed duration) with the merge window and cap that trigger recognition, and verify with a unit test that both the gap trigger and the duration cap are reported
- [x] 5.4 Publish the recording mix window fill with its window size per channel, and verify with a unit test that the fraction reflects the queued samples of that channel only

## 6. Model activity across engines (Rust)

- [x] 6.1 Publish voice-activity activity per channel (speech frames evaluated and whether speech is currently detected) and verify with a unit test that a silent channel and a speaking channel resolve to different activity
- [x] 6.2 Publish speech-recognition activity (pending queue depth and the most recent recognition) and report the engine kind, model identity and loaded state, verified by a unit test for the disabled/unloaded case
- [x] 6.3 Publish word-alignment activity (queue occupancy, dropped count, refinements produced, engine loaded state) and verify with a unit test that a disabled alignment settings resolves to disabled rather than failure
- [x] 6.4 Assemble the snapshot's model section covering every model kind in use and verify with a unit test that each kind appears with readiness and activity, and that a disabled model is not reported as an error

## 7. Frontend: fills and models

- [x] 7.1 Render each channel's buffer fills as a fraction of their trigger threshold with a short label naming the gated operation, and verify with unit tests that percentages are computed correctly and a low fill is not styled as an error or warning
- [x] 7.2 Render the model section in the block's tooltip so every model kind shows identity, readiness and activity, and verify with unit tests that disabled and unloaded models are labelled as such
- [x] 7.3 Tighten the recording interval to 150 ms on the single existing timer and verify only one interval exists and that the status is refreshed at the required rate

## 9. Visual indicators

- [x] 9.1 Replace the numeric fill tokens on each channel line with proportional bar indicators labelled with the operation they gate, clamping the rendered fill to the track while keeping the fired state visible, and verify with unit tests that the rendered proportion matches the fill, that an over-threshold buffer renders full, and that a low fill is not styled as an error or warning
- [x] 9.2 Add the model-indicator row beneath the two channel lines with one labelled indicator per model kind, and verify with unit tests that each state resolves to the documented colour, that disabled or unloaded models resolve to the idle colour rather than the error colour, and that a degraded queue resolves to the warning colour
- [ ] 9.3 Verify the block still fits the recording controls' width budget with bars and the indicator row present, and that the two channel lines plus one model row remain the only rows shown
- [x] 9.5 Publish each channel's processed-audio level (RMS and peak) with the age of its last sample, measured before the samples reach the recording ring buffer, and verify with a unit test that levels stay per channel and that silence publishes an empty level
- [x] 9.6 Render the level as a decibel-scale bar that empties when its sample is older than the staleness window, and verify with unit tests that a normal speech level fills the bar, silence renders empty, and a stale level renders empty rather than holding its last value
- [x] 9.4 Run the frontend unit tests, `npx tsc --noEmit` and lint on the touched files, and verify they pass with no new errors

## 8. Verification of the extension

- [x] 8.1 Run `cargo test` and `cargo check` in `frontend/src-tauri` and verify they pass with no new warnings in the touched modules
- [x] 8.2 Run the frontend unit tests and `npx tsc --noEmit` and verify the new formatting and fill tests pass with no new type errors
