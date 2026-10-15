## Why

Tags selected **before or during** a recording do not reliably reach the saved meeting — evidence on this machine shows `pending_tag_ids` is empty in every recording folder (`Music/meetily-recordings/Meeting 2026-09-1*`) and no `meeting_tag_links` rows exist for any 2026-09-15 recording, while tags applied **after** a recording link correctly. The flow can lose the pending set silently, so users must re-tag every meeting post-hoc. Separately, the tag color palette has only 10 entries, so distinct tags frequently share the same chip background and are hard to tell apart at a glance.

## What Changes

- Make the pending-tag pipeline end-to-end reliable and observable: a tag picked before start or during recording MUST end up linked to the saved meeting; any failure to persist or link the pending set MUST be surfaced to the user instead of being dropped with a console error.
- Remove the silent-loss paths: pre-start selection that cannot be flushed at recording start, save-time linking that depends on frontend session state for the recording folder, and any write that resets the pending set to empty without user intent.
- Expand the tag color palette from 10 to at least 30 distinct entries (target ~40) on both the backend key list and the frontend chip styles, keeping the existing key-based storage and deterministic name→color assignment.
- **BREAKING**: none (no database migration; stored palette keys stay valid, unknown keys still fall back).

## Capabilities

### New Capabilities

- (none)

### Modified Capabilities

- `recording-tags`: pending-set persistence must be reliable and failures observable; the saved meeting must link the pending set even when the frontend session state for the recording folder is missing.
- `meeting-tags`: the fixed color palette must contain at least 30 distinct colors, and the manual color cycle must traverse the full palette.

## Impact

- Frontend: `frontend/src/hooks/usePendingRecordingTags.ts`, `frontend/src/hooks/useRecordingStop.ts`, `frontend/src/components/MeetingTags/PendingTagsPicker.tsx`, `frontend/src/lib/meeting-tags.ts`, `frontend/src/components/MeetingTags/TagEditorPopover.tsx`.
- Backend (Rust): `frontend/src-tauri/src/database/tag_commands.rs`, `frontend/src-tauri/src/api/api.rs`, `frontend/src-tauri/src/audio/recording_saver.rs`, `frontend/src-tauri/src/database/models.rs` (palette constant).
- Tests: Rust unit tests for pending-tag read/write/link and the palette; frontend behavior covered by existing hook/type checks.
- No database migration; `metadata.json` key remains optional and forward/backward compatible.
