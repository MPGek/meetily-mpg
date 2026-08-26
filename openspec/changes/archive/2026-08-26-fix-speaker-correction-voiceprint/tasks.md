## 1. Database Layer

- [x] 1.1 Add `SpeakerRepository::resolve_cluster_by_time_overlap` method in `frontend/src-tauri/src/database/repositories/speaker.rs` that queries `speaker_embeddings` for unassigned cache rows (`speaker_id IS NULL`) overlapping a given time range, groups by `cluster_label`, and returns the cluster with the longest total overlap duration
- [x] 1.2 Add unit tests for `resolve_cluster_by_time_overlap` covering: single cluster match, multiple clusters with different overlap durations, no match (empty result), channel filtering (mic vs system)

## 2. Command Layer

- [x] 2.1 Modify `assign_block_speaker` in `frontend/src-tauri/src/database/speaker_commands.rs` to call `resolve_cluster_by_time_overlap` when `get_transcript_cluster` returns `(meeting_id, None)`, passing the transcript's time range and channel
- [x] 2.2 Determine the transcript's channel from its `source_device` column ("System" → system, otherwise → mic) and pass it to the resolution method
- [x] 2.3 Use the resolved cluster label to call `enroll_cluster` when the transcript has no cluster label
- [x] 2.4 Add a warning log when `enroll_cluster` returns 0 (no exemplars reparented) including meeting ID and cluster label for observability

## 3. Integration Testing

- [x] 3.1 Add integration test: correct a speaker on a transcript with `speaker = NULL` but with overlapping cluster exemplars → verify voiceprint is enrolled
- [x] 3.2 Add integration test: correct a speaker on a transcript with `speaker = NULL` and no overlapping exemplars → verify label is applied without error and zero voiceprints enrolled
- [x] 3.3 Add integration test: correct a speaker on a transcript with valid `speaker` column → verify existing behavior unchanged (enrollment uses transcript's cluster label directly)

## 4. Verification

- [x] 4.1 Manually test the flow: run enhance on an old meeting, run diarization, correct a speaker label on a block, verify the voiceprint appears in VoiceprintBrowser
- [x] 4.2 Verify channel separation: correct a mic-channel block and a system-channel block, verify their enrolled voiceprints have the correct channel tags
- [x] 4.3 Run `cargo test` to ensure all existing and new tests pass
- [x] 4.4 Run `cargo clippy` and `cargo fmt` to ensure code quality
