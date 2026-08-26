## 1. Fix provenance in `enroll_embeddings_from_buffer`

- [x] 1.1 Add `meeting_id: &str` and `cluster_label: &str` parameters to `SpeakerRepository::enroll_embeddings_from_buffer` in `speaker.rs`
- [x] 1.2 Update the INSERT query to include `meeting_id` and `cluster_label` columns in the VALUES clause, binding the new parameters
- [x] 1.3 Update the doc comment to reflect that provenance is now preserved

## 2. Update caller in `finalize_online_session`

- [x] 2.1 Pass `&meeting_id` and `cluster_label` from the turn override tuple into the `enroll_embeddings_from_buffer` call in `recording_commands.rs`

## 3. Fix unit tests

- [x] 3.1 Update `enroll_embeddings_from_buffer_takes_overlapping_best_n` test to pass `meeting_id` and `cluster_label` arguments, and assert they are set on the enrolled rows
- [x] 3.2 Update `enroll_embeddings_from_buffer_keeps_channels_clean` test to pass the new arguments

## 4. Verify

- [x] 4.1 Run `cargo check` in `frontend/src-tauri` to confirm compilation
- [x] 4.2 Run `cargo test` in `frontend/src-tauri` to confirm all speaker repository tests pass
