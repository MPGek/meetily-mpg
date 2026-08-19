## 1. Backend: session-stable mic prefix

- [x] 1.1 In `frontend/src-tauri/src/audio/online_diarization.rs`, thread a `has_system_device: bool` parameter through `OnlineDiarizationProcessor::new` and store it as a session-stable `mic_prefix` (`"MIC_SPEAKER"` if `has_system_device` else `"SPEAKER"`)
- [x] 1.2 In `process_chunk` Fast arm, replace the per-chunk `if saw_system_audio { "MIC_SPEAKER" } else { "SPEAKER" }` mic prefix selection (line ~525) with the stored session-stable `mic_prefix`
- [x] 1.3 In `finalize()`, use the stored `mic_prefix` for mic match routing (currently `format!("MIC_SPEAKER")` via `self.saw_system_audio`, lines ~694-700); keep `saw_system_audio` only to decide whether system segments exist to match
- [x] 1.4 In `frontend/src-tauri/src/audio/recording_commands.rs::start_recording_with_meeting_name`, pass `system_device` presence into the processor spawn (device resolution is already done before line ~445); verify the `DiarizationMode::Efficient` path also receives the flag consistently

## 2. Backend: channel-scoped session identity store

- [x] 2.1 In `online_diarization.rs`, change `PrototypeStore::session_embeddings` from `HashMap<usize, Vec<(Vec<f32>, f32, String)>>` to `HashMap<(String /*channel*/, usize /*pipeline_id*/), Vec<(Vec<f32>, f32)>>` (channel moves into the key; duration kept)
- [x] 2.2 Update `PrototypeStore::push_session` to key on `(channel, pipeline_id)` using the channel argument already passed at the call site (lines ~574-583)
- [x] 2.3 Update `PrototypeStore::bind` to resolve `(channel, pipeline_id)` from the cluster label prefix (`MIC_SPEAKER_` в†’ mic; `SPEAKER_` в†’ system when stereo else mic) and `parse_pipeline_id`, and seed prototypes from only that channel's embeddings
- [x] 2.4 Add a helper `PrototypeStore::channel_and_id(label) -> Option<(String, usize)>` and unit tests covering `MIC_SPEAKER_03`, `SPEAKER_02`, and mono (`SPEAKER_01` when mic_prefix is `SPEAKER`) label parsing; verify no cross-channel seeding

## 3. Backend: finalize returns raw buffers for ground-truth enrollment

- [x] 3.1 Extend `OnlineClusterEmbeddings` (or the `finalize()` return tuple) to carry the raw per-channel timestamped embedding buffers (`mic_emb`/`sys_emb` `EmbeddingBuffer` entries) in addition to grouped clusters
- [x] 3.2 Extend `OnlineSessionData` in `recording_commands.rs` with `mic_embeddings: Vec<(f32, f32, Vec<f32>)>` and `sys_embeddings` fields; populate them in `stop_recording` from the `finalize()` result (line ~860)
- [x] 3.3 Verify `finalize()` Fast arm still takes the engine (consuming the buffers) before the data is copied out; ensure no double-borrow or clone of the full buffer set

## 4. Backend: ground-truth enrollment from user-assigned blocks

- [x] 4.1 In `database/repositories/speaker.rs`, factor the per-person cap enforcement out of `enroll_cluster` (lines ~355-375) into a shared `enforce_prototype_cap(tx, speaker_id)` helper
- [x] 4.2 Add `enroll_embeddings_from_buffer(pool, speaker_id, channel, embeddings, window)` that overlaps the block's `[start, end]` against the buffer entries, inserts best-N by duration as direct prototypes (`speaker_id` set, `meeting_id`/`cluster_label` NULL), and applies the cap; return the enrolled count
- [x] 4.3 In `finalize_online_session`, for each turn override from `ONLINE_TURN_OVERRIDES`, look up the matched transcript's time window (by cluster label + time overlap) and call `enroll_embeddings_from_buffer` against the matching channel buffer from session data; log per-override enrolled counts
- [x] 4.4 Confirm cluster-wide live bindings still enroll via `enroll_cluster` and now receive channel-clean seeds from Decision 2; add a test that a mic binding and a system binding sharing numeric id 0 do not cross-enroll

## 5. Backend: surface provenance and confidence

- [x] 5.1 In `database/repositories/meeting.rs`, extend `TRANSCRIPT_DISPLAY_SELECT` to also select `ms.matched_by AS speaker_matched_by` and `ms.match_score AS speaker_match_score`, and resolve a provenance value (override в†’ user в†’ auto в†’ fallback) following the existing `COALESCE` precedence (lines ~13, ~85-106)
- [x] 5.2 In `online_diarization.rs`, thread `speaker_recognition::MatchResult` (score included) through `PrototypeStore::recognize` so the Fast-mode turn emission can carry `matched_by`/`match_score`; extend the `SpeakerTurn` struct with `matched_by` and `match_score` fields (optional, skip serialization when absent)
- [x] 5.3 Verify the live `online-speaker-turn` payload includes `matched_by`/`match_score` for recognized turns and omits them for unrecognized turns

## 6. Frontend: pin user-assigned live labels

- [x] 6.1 In `contexts/TranscriptContext.tsx`, add a `pinnedLabelsRef: Map<string, { cluster: string; name: string }>` keyed by transcript id
- [x] 6.2 Update `applyLiveSpeakerLabel` (line ~626) to record pins: block-scope pins that transcript's id; cluster-scope pins every transcript currently carrying the cluster label
- [x] 6.3 In the `onSpeakerTurn` handler (lines ~126-142), skip re-matching any transcript whose pin's cluster matches the turn's speaker вЂ” the pinned name and cluster stay frozen until the user edits again; unpinned transcripts keep current behavior
- [x] 6.4 Clear pins in `clearTranscripts` and on the `recording-started` reset (line ~171); verify a fresh recording starts with an empty pin set

## 7. Frontend: render provenance and confidence

- [x] 7.1 Extend the frontend `Transcript`/segment types with `speaker_matched_by` and `speaker_match_score` fields, mapped in `app/_components/TranscriptPanel.tsx` segment conversion and the meeting-details page segment mapping
- [x] 7.2 Add a small formatter (e.g., in `VirtualizedTranscriptView.tsx` or a shared lib) that renders `name` + ` (auto)` + similarity percentage for `matched_by === 'auto'`, plain name for `user`, and the formatted cluster label for fallback
- [x] 7.3 Update `SpeakerLabel` (line ~131) to use the formatter so the meeting-details view and live recording view both show the `(auto)`/confidence marker only for auto-matched names
- [x] 7.4 Verify the live `SpeakerTurn` handler maps `display_name` + `matched_by`/`match_score` into the segment rendering so live renames of pinned clusters keep showing the plain user name

## 8. Verification

- [x] 8.1 `cargo test` in `frontend/src-tauri` passes; run the existing `repro_online_diarization` and `repro_full_stop` binaries with `MEETILY_MODELS_DIR` set and confirm no engine error-state regressions
- [x] 8.2 Manual stereo recording in Fast mode: confirm mic turns emit `MIC_SPEAKER_NN` from the first chunk (before any system audio arrives), renames survive stop, and system renames persist with `(auto)`/confidence shown for auto-matched names
- [x] 8.3 Manual mono recording (no system device): confirm mic turns use `SPEAKER_NN` throughout and renames persist
- [x] 8.4 Manual single-block live rename: confirm the block shows the new name, does not revert on subsequent turns, and the saved meeting displays the name while other blocks of the same cluster stay labeled
- [x] 8.5 Re-match check: run "Re-analyze Speakers" on a meeting with a live user binding and confirm the binding is preserved (`matched_by='user'`) and not overwritten by auto-recognition
