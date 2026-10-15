## Why

Speaker corrections the user makes in the transcript UI are not fully persisted or reused: correct names and premise labels revert to "predicted" when a live recording stops, the `(auto) xx%` suffix never clears even after an edit or a confirmation, and inline block corrections never enroll the person's voiceprint — so automatic recognition stays empty unless the user manually picks caches in Settings. The user cannot tell whether their correction was saved.

## What Changes

- **Enroll voiceprints on inline corrections** (offline single-block, offline apply-to-all, and live single-turn): when a user assigns a speaker to a transcript block, the embeddings covering that block's time window are enrolled as that speaker's ground-truth prototypes — fulfilling the already-specified but unimplemented `speaker-identity-registry` enrollment requirement. This makes voiceprints appear automatically instead of requiring Settings-side reconfirm.
- **Persist live corrections reliably on stop** (`finalize_online_session`): user-assigned live labels are written so the stored/displayed transcript uses the user's identity, not the raw predicted cluster label, and are not lost when the post-stop restore join fails. Silent no-op live assignments (no active prototype store) surface an error instead of reverting silently.
- **Clear the `(auto) xx%` suffix on edit and on confirm**: the offline local updater mirrors the live updater (sets `speaker_matched_by='user'`, clears `match_score`); a new "confirm prediction" capability lets the user mark an auto-assigned block/cluster as correct, dropping the suffix so a saved confirmation is visibly distinct from an unchecked prediction.
- **Give explicit "saved" affordance**: confirming or correcting always yields visible feedback (suffix removed, provenance fixed), so the user reliably knows the change persisted.
- **Human-readable voiceprint storage**: the voiceprint browser storage summary shows MB (matching the Settings general-tab storage section) instead of a raw byte count.
- **Voiceprint browser starts collapsed**: speaker and unconfirmed-meeting groups open collapsed by default, with the existing expand/collapse-all controls still available.

## Capabilities

### New Capabilities
- `speaker-correction` (under `speaker-identity-registry`? No — new top-level capability): captures the shared behavior of inline speaker corrections — enrolling ground-truth voiceprints from block assignments, confirming an auto-prediction as correct, and clearing the auto-confidence suffix. (Path: `speaker-correction`)

### Modified Capabilities
- `speaker-identity-registry`: the existing "Speaker enrollment on assignment" requirement is currently unmet for single-block corrections; this change implements the block-window enrollment it already mandates and adds the confirmation semantics.
- `live-speaker-labels`: finalize must persist live user bindings into stored transcripts/SQL deterministically rather than relying only on the render-time join, and inline live corrections must enroll.
- `speaker-label-confidence`: the `(auto) xx%` display state must be cleared (or never shown) after any user edit or confirmation in both modes.

## Impact

- **Backend (Rust)**: `frontend/src-tauri/src/database/speaker_commands.rs` (`assign_block_speaker`, `assign_speaker`, `apply_block_speaker_to_cluster`, new confirm command), `frontend/src-tauri/src/database/repositories/speaker.rs` (block-window enrollment for offline, `enroll_embeddings_from_buffer` wiring), `frontend/src-tauri/src/audio/recording_commands.rs` (`assign_live_speaker` error on no store, `finalize_online_session` deterministic persistence of user bindings).
- **Frontend (TS/React)**: `frontend/src/hooks/usePaginatedTranscripts.ts` (`updateSpeakerLabel`), `frontend/src/contexts/TranscriptContext.tsx`, `frontend/src/components/VirtualizedTranscriptView.tsx` (`SpeakerLabel` confirm affordance, remove `speaker_label===name` short-circuit), `frontend/src/services/recordingService.ts`.
- **Database**: `transcripts` (block confirm/override), `meeting_speakers` (matched_by='user' flip for confirmation); voiceprint enrollment writes to `speaker_embeddings`.
- **Specs synced**: `speaker-identity-registry`, `live-speaker-labels`, `speaker-label-confidence` in `openspec/specs/`.
