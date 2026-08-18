# Tasks: speaker-identity-registry

## 1. Schema and repositories

- [x] 1.1 Create migration adding `speakers`, `speaker_embeddings` (with two-owner CHECK constraint), `meeting_speakers`, `meeting_expected_speakers` tables and indexes per design D1; verify migrate up/down on a copy of a real DB
- [x] 1.2 Add Rust models for the four tables in `database/models.rs` (embedding as `Vec<u8>` blob; helpers to convert `Vec<f32>` ↔ little-endian bytes)
- [x] 1.3 Create `database/repositories/speaker.rs`: CRUD for speakers (find-or-create by case-insensitive name, rename), prototype queries (by speaker set + model tag), enrollment (reparent best-K cache rows, enforce per-person cap), `meeting_speakers` upsert/read, expected-speaker set/get, storage stats query
- [x] 1.4 Unit-test repository: find-or-create idempotence, enrollment reparenting + caps, expected-list round-trip, stats math vs `SUM(LENGTH(embedding))`

## 2. Recognition core

- [x] 2.1 Add `audio/speaker_recognition.rs`: L2-normalize + cosine similarity, best-candidate matching over prototypes grouped by speaker (max over prototypes), channel-preference rule, τ=0.7 constant, `model` tag filter
- [x] 2.2 Unit-test matcher with synthetic embeddings: identical → 1.0, orthogonal → 0.0, channel preference, threshold boundary, empty candidate set
- [x] 2.3 Add display-name resolution to transcript/meeting queries: LEFT JOIN `meeting_speakers`→`speakers`, return `COALESCE(speakers.name, transcripts.speaker_label)` as display label (cluster label column unchanged)

## 3. Offline diarization integration

- [x] 3.1 In `audio/diarization.rs`, compute per-cluster centroid + exemplar set (with durations) after clustering and persist to `meeting_speakers` + `speaker_embeddings` cache rows alongside existing transcript speaker writes
- [x] 3.2 After cache persistence, run recognition against expected speakers (or all) and auto-assign `meeting_speakers.speaker_id` (`matched_by='auto'`, score) above threshold
- [x] 3.3 Add `rematch_meeting_speakers` command: re-run recognition from cached centroids only (no audio), preserving `matched_by='user'` bindings
- [x] 3.4 Verify end-to-end: diarize meeting A → name Alice → diarize meeting B (Alice expected) → Alice auto-labeled; edit expected list → re-match resolves without diarization re-run

## 4. Speaker editing commands and UI

- [x] 4.1 Add Tauri commands: `list_speakers` (registry for dropdown), `assign_speaker(meeting_id, cluster_label, speaker_id | new_name)` (find-or-create, upsert mapping `matched_by='user'`, enroll), `rename_speaker(speaker_id, new_name)` (global), `set_expected_speakers(meeting_id, ids)`, `get_expected_speakers(meeting_id)`; register in `lib.rs`
- [x] 4.2 Replace `SpeakerLabel` inline-only editor with a combobox: free text + dropdown of registry speakers (search-friendly); wire to `assign_speaker`/`rename_speaker`; keep propagation rendering via resolved names
- [x] 4.3 Add expected-speaker multi-select on the meeting page (diarization section) wired to `set_expected_speakers` + `rematch_meeting_speakers`
- [x] 4.4 Verify in UI: dropdown-assign existing person → all blocks of cluster update + embeddings enrolled; type new name → person created; rename person → name changes in a second meeting too; legacy meetings still show old labels

## 5. Online diarization: stop-time enroll and match

- [x] 5.1 Fast mode: add own `ResNet34Adapter` + per-channel timestamped `EmbeddingBuffer` fed from `process_chunk` (same extraction as Efficient); at stop, group buffered embeddings by pipeline speaker ID via time overlap with stable turns
- [x] 5.2 At recording stop (both modes): persist cluster centroids + exemplar caches, run recognition, and set `meeting_speakers` before final speaker assignments are emitted
- [x] 5.3 Plumb `expected_speaker_ids` through `start_recording*` commands into session state; persist to `meeting_expected_speakers` when the meeting row is created at stop
- [x] 5.4 Enroll session embeddings for all user-assigned clusters at stop (best-8 reparenting, per-person cap)
- [x] 5.5 Verify: record with Efficient mode → name speaker at stop → next recording (speaker expected) auto-labels; record Fast → stop-time assignments carry recognized names

## 6. Live Fast-mode recognition and rename

- [x] 6.1 Add shared `PrototypeStore` (`Arc<RwLock<…>>`) beside `ONLINE_DIARIZATION_TASK`; load expected speakers' prototypes at session start
- [x] 6.2 Match each Fast-mode chunk embedding against the store; relabel outgoing live turns with recognized names (fallback: cluster label)
- [x] 6.3 Add `assign_live_speaker(cluster_label, speaker_id | new_name)` command: find-or-create person, update session binding, merge prototypes into store; include bindings in stop-time finalize
- [x] 6.4 Add the combobox speaker editor to the live recording view wired to `assign_live_speaker`
- [x] 6.5 Verify: Fast-mode recording → mid-recording rename `SPEAKER_01`→Alice → subsequent turns show "Alice" live → after stop, embeddings enrolled and next session recognizes her

## 7. Storage stats and polish

- [x] 7.1 Add `speaker_storage_stats` command (registry count, prototype count, cache count, total bytes) and display in Settings near diarization models with human-readable size
- [x] 7.2 Update `AGENTS.md`/docs speaker-diarization notes to reference the registry, new tables, and new commands
- [x] 7.3 Run `openspec validate speaker-identity-registry` and full `cargo test`/`cargo clippy` for touched crates; run frontend typecheck

## 8. Per-block speaker override (single-transcript relabel)

- [x] 8.1 Add migration column `transcripts.speaker_override_id` (nullable FK → `speakers.id`); update transcript display-name resolution to `COALESCE(override_speaker.name, meeting_speakers→speakers.name, transcripts.speaker_label)` in `database/repositories/meeting.rs`; verify migrate up/down
- [x] 8.2 Add repository helpers in `database/repositories/speaker.rs`: set/clear per-transcript override (no `meeting_speakers` change, no enrollment) and override-aware display-name lookup; unit-test override precedence + survival of override across re-match
- [x] 8.3 Add `assign_block_speaker(transcript_id, speaker_id | new_name)` Tauri command (find-or-create person, write override only) + `apply_block_speaker_to_cluster` route for the "apply to all blocks of this speaker" option (reuses `assign_speaker` path); register in `lib.rs`
- [x] 8.4 Frontend: `SpeakerLabel` combobox gains a scope toggle defaulting to "This block" — single-block writes the override; "All blocks of this speaker" falls back to cluster-wide `assign_speaker`; propagate single-block updates in `VirtualizedTranscriptView`/`TranscriptContext` state
- [x] 8.5 Live Fast-mode: record per-turn overrides (cluster label + turn time range → speaker_id) in session state via `assign_live_speaker` scope; at stop-finalize apply overrides to matched transcripts before emitting assignments; live combobox gains the same scope toggle
- [x] 8.6 Verify end-to-end: relabel one block offline → only it changes; apply-to-all → whole cluster + enrollment; relabel one live turn → shows immediately and persists at stop; rename person → overrides resolve to new name everywhere

## 9. Non-disruptive in-place label updates

- [x] 9.1 Meeting transcript view: speaker relabels update only the affected segment(s) in local state (via `onUpdateSpeakerLabel` → paginated list state), no full refetch after assignment; verify `VirtualizedTranscriptView` rows reconcile in place
- [x] 9.2 Live recording view: live relabels (single-turn and apply-to-all) update only the affected turn(s) via `applyLiveSpeakerLabel`, no panel re-render or scroll reset
- [x] 9.3 Error handling: on failed assignment, revert the locally applied label and show a toast; the list must otherwise remain undisturbed
- [x] 9.4 Verify in UI: relabel a block while scrolled mid-list → scroll position kept, no empty/loading flash, only the relabeled block(s) change; same for a live rename during Fast-mode recording
