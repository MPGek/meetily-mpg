# Proposal: speaker-identity-registry

## Why

Diarization already computes speaker embeddings (ResNet34 INT8, 256-d) in both the offline and online paths — and discards them. Speaker names are per-meeting only (`meetings.speaker_names` JSON + denormalized `transcripts.speaker_label`), so every meeting starts anonymous: users re-type "Alice" in meeting after meeting, and there is no way to recognize a voice across meetings. We should persist voiceprints, recognize known speakers automatically, and let users manage a global speaker registry directly from the transcript UI.

## What Changes

- **Global speaker registry**: new `speakers` table (id, name, `is_me` flag) with cross-meeting identity. Renaming a speaker renames them **globally** (all meetings at once).
- **Voiceprint storage**: new `speaker_embeddings` table storing 256-d embeddings with a `model` tag (version guard), capture `channel` ('mic'|'system'), and duration (quality signal). One table serves two owners: enrolled prototypes (`speaker_id` set) and unassigned per-meeting cluster caches (`meeting_id` + `cluster_label` set). Enrollment = reparenting cache rows, not copying. Caches are kept forever.
- **Per-meeting cluster→person mapping**: new `meeting_speakers` table replacing `speaker_names` JSON as the source of truth; stores cluster centroid (recognition target), `matched_by` ('auto'|'user'), and match score. The JSON/label columns become derived/legacy fallback only.
- **Expected-speaker allowlist per meeting**: new `meeting_expected_speakers` table. Auto-recognition matches only against expected speakers' prototypes; an empty list means match against ALL known speakers. The allowlist never restricts manual assignment. In live mode the list travels in-memory with the recording session (the meeting row does not exist until stop) and is persisted at stop.
- **Auto-recognition**: after clustering (offline diarization, online Efficient/Fast at stop), cluster centroids are matched against expected speakers' prototypes by cosine similarity; matches above threshold are auto-assigned (`matched_by='auto'`). Because centroids are cached, editing the expected-speaker list after diarization allows instant re-matching with no audio re-processing.
- **Speaker editing UX**: speaker name editor gains a dropdown of known registry speakers in addition to free text, available on offline transcripts and on live turns (Fast mode). Editing **defaults to single-block scope**: selecting an existing person or typing a new name relabels only the transcript block (or live turn) being edited, via a new per-transcript speaker override. An explicit "apply to all blocks of this speaker" control performs the cluster-wide link (mapping with `matched_by='user'` + enrollment of the cluster's cached embeddings). Renaming a linked person still propagates globally via the read-time join. All relabels update the transcript view **in place**: only the affected block(s) re-label from local state, the scroll position is preserved, and the panel never re-renders fully or flashes an empty/loading state.
- **Live rename with immediate effect (Fast mode only)**: renaming a live speaker during recording updates an in-memory prototype store shared with the online diarization processor, so subsequent chunks are recognized and labeled live; at stop, the session's cluster embeddings enroll to the DB. Efficient mode supports rename at/after stop only (it has no live labels).
- **Storage visibility**: new command + Settings display showing voiceprint storage usage (embedding counts and bytes) so the user can track growth; no cleanup tooling in this change.

## Capabilities

### New Capabilities
- `speaker-identity-registry`: global speaker registry, voiceprint storage (prototypes + per-meeting cluster caches), cluster→person mapping, per-transcript speaker overrides (single-block relabel), expected-speaker allowlists, cosine-similarity auto-recognition, speaker editing UI (dropdown + free text, single-block default with apply-to-all option, global rename), storage stats.

### Modified Capabilities
- `speaker-diarization`: offline diarization must persist per-cluster centroids and exemplar embeddings, and auto-match clusters against expected-speaker prototypes after clustering.
- `online-speaker-diarization`: both modes must retain per-chunk embeddings for enrollment at recording stop (Fast mode embeds chunks itself since polyvoice `SpeakerTurn` carries no embedding), and must enroll user-assigned clusters to the registry at stop.
- `live-speaker-labels`: live speaker labels must resolve to recognized registry names, and renaming a live speaker in Fast mode must take effect immediately for the remainder of the session.

## Impact

- **Database**: new migration (`speakers`, `speaker_embeddings`, `meeting_speakers`, `meeting_expected_speakers` + indexes, plus nullable `transcripts.speaker_override_id` for per-block relabels); read path for display names moves to joins over `meeting_speakers`/`speakers` (legacy `speaker_names`/`speaker_label` retained as fallback; per-transcript override takes precedence).
- **Backend (Rust)**: new `database/repositories/speaker.rs`; changes in `audio/diarization.rs` (cache centroids/exemplars, post-cluster matching), `audio/online_diarization.rs` (self-embedding in Fast mode, shared prototype store, enrollment at stop), `audio/recording_commands.rs` (expected-speaker list plumbed through recording start; live-assign command; per-turn override recording applied at stop), new Tauri commands (`list_speakers`, `assign_speaker`, `assign_block_speaker`, `rename_speaker`, `set_expected_speakers`, `speaker_storage_stats`, live assign).
- **Frontend**: `SpeakerLabel` editor becomes text+dropdown combobox with a single-block default and "apply to all blocks of this speaker" option (offline transcript view and live recording view); relabels update the transcript list **in place** (no full re-render, no scroll reset, no empty/loading flash); expected-speaker multi-select on recording start UI and meeting page; voiceprint storage stats in Settings.
- **Dependencies**: none new (matching is brute-force cosine in Rust; embeddings are 1 KB each, hundreds of rows → microseconds).
- **Privacy/storage**: voiceprints are ~1 KB each; retained indefinitely by design (user tracks size via the new stats display).
