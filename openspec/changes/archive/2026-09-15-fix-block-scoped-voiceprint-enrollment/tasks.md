## 1. Enrollment primitive (window + channel scoped)

- [x] 1.1 Add `SpeakerRepository::enroll_block_window(pool, meeting_id, cluster_label, channel, window, speaker_id)` that selects up to `ENROLLMENT_BEST_K` unassigned cache rows (`speaker_id IS NULL`) for `(meeting_id, cluster_label, channel)` with `audio_start_time < window.end` and `audio_end_time > window.start`, ordered by `duration_secs` DESC, and reparents them to `speaker_id`, then enforces the per-person cap. Verify: a unit test in `speaker.rs` asserts only overlapping rows of the matching channel are reparented, capped at K.
- [x] 1.2 Add a cleanup helper that demotes to unassigned cache the prototype rows for `(meeting_id, cluster_label, channel)` that overlap a window, belong to a speaker other than the target, and are not covered by a transcript block overridden to their current speaker. Verify: a unit test asserts overlapping foreign rows are demoted while a row pinned by a covering per-block override is left untouched.
- [x] 1.3 Run the cleanup and enrollment steps inside a single transaction so a failure cannot leave rows demoted without being re-enrolled. Verify: a unit test that corrects the same block twice (speaker A, then speaker B) ends with B owning the overlapping rows and A owning none.

## 2. Offline single-block correction path

- [x] 2.1 Update `speaker_commands::assign_block_speaker` to derive the channel from the transcript's `source_device` and call the cleanup + `enroll_block_window` sequence instead of `SpeakerRepository::enroll_cluster`. Verify: an integration test that corrects one block of a mixed cluster enrolls only that block's overlapping exemplars and leaves sibling speakers' exemplars unassigned.
- [x] 2.2 Keep the existing NULL-cluster time-overlap fallback (`resolve_cluster_by_time_overlap`) but make its enrollment window-scoped rather than whole-cluster. Verify: a unit test over a transcript with no cluster label enrolls only rows overlapping the resolved block window.
- [x] 2.3 Confirm the no-cache / legacy path still applies the override with zero enrollment and no error. Verify: the existing `block_correction_without_cache_is_noop` test still passes unchanged.
- [x] 2.4 Confirm a single-block correction does not create or modify a `meeting_speakers` row. Verify: an assertion that `meeting_speakers` is unchanged after `assign_block_speaker`.

## 3. Cluster-wide binding cleanup

- [x] 3.1 Update `assign_speaker` and `apply_block_speaker_to_cluster` so a cluster-wide (re-)binding demotes prototypes that originate from the same cluster and channel under a different speaker, except rows pinned by a covering per-block override, in addition to the existing best-K `enroll_cluster`. Verify: a unit test that re-binds a cluster from Bob to Carol ends with Bob owning none of that cluster's rows and Carol owning the fresh best-K.
- [x] 3.2 Apply the same cleanup to live cluster renames in `audio::recording_commands::finalize_online_session`'s `live_bindings` loop so online and offline behave identically. Verify: a finalize test (or targeted manual run) leaves no stale prototypes from a re-bound live cluster with the previous speaker.

## 4. Regression verification

- [x] 4.1 Add a regression test reproducing the reported shape — a cluster with blocks attributed to three different people, one block corrected — and assert the corrected speaker's prototype set contains no exemplars from the sibling blocks. Verify: the test fails against the pre-change `enroll_cluster` behavior and passes after the fix.
- [x] 4.2 Run the speaker repository test suite and the Tauri test build. Verify: `cargo test --manifest-path frontend/src-tauri/Cargo.toml` passes with no regressions in `speaker.rs` enrollment/override tests.
- [ ] 4.3 Manually re-correct the `SPEAKER_02` blocks of `Meeting 2026-09-14_15-01` and inspect the Voiceprint Browser. Verify: each speaker's prototypes from that meeting carry timecodes inside their own corrected blocks, and Alex Shingel retains no `SPEAKER_02` exemplars from the 47–105s or 238–269s ranges.
