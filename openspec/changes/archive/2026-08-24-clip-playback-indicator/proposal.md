## Why

The "Play clip" buttons in VoiceprintBrowser have no visual feedback when audio is playing. Users cannot tell whether a clip is currently playing, which is confusing when the button stays in its default state during active playback. This contrasts with the transcript view and the main audio player bar, which already show Play/Pause icon transitions and color changes. Additionally, clicking the same button again should pause (toggle) rather than restart, and a failure to start playback currently shows no distinct error state. Users also lack a safe way to reset the voiceprint store: clearing all enrolled prototypes and unassigned caches currently requires tedious per-row deletion, with no single confirmed bulk action for privacy, device hand-off, or test-data cleanup.

## What Changes

- Add a playing-state indicator to the VoiceprintBrowser "Play clip" buttons: when a clip is actively playing, the button changes its icon (e.g., Pause icon) and visual style (color/background)
- When playback ends naturally, the button reverts to its original "Play clip" appearance
- When the user selects a different clip to play, the previously-playing button reverts and the newly-selected one enters the playing state
- When the user clicks the same "Play clip" button again while its clip is actively playing, playback pauses and the button reverts to the idle state (toggle pause)
- If playback fails to start (missing audio file, decode error, or `play()` rejection), the button for that row shows a distinct failure state (red background/style with failure icon) instead of remaining in idle or playing state
- Add a "Remove all voiceprints & caches" destructive button to VoiceprintBrowser (in the Storage header) that opens a confirmation dialog; on confirm it deletes all rows from `speaker_embeddings` (both registry prototypes and per-meeting unassigned caches) and refreshes the browser counts

## Capabilities

### New Capabilities

_None._

### Modified Capabilities

- `meeting-audio-player`: The clip-level play buttons in VoiceprintBrowser gain a playing-state visual indicator (icon + style change) that tracks the active playback session, supports pause-on-second-press toggle, shows a red failure indicator when playback cannot start, and the browser now provides a confirmed bulk-removal action for all voiceprints/cached embeddings (clearing `speaker_embeddings` prototypes + caches).
- `speaker-identity-registry`: Voiceprint storage gains a bulk-clear operation that removes all `speaker_embeddings` rows in one confirmed step (prototypes + caches), reflected immediately in `speaker_storage_stats` and the VoiceprintBrowser lists. Delta for this capability is hosted in `specs/meeting-audio-player/spec.md` for this change scope; a dedicated `specs/speaker-identity-registry/spec.md` delta can be split out via `/opsx-continue` if a separate capability file is desired.

## Impact

- `frontend/src/components/VoiceprintBrowser.tsx` — Play clip buttons (confirmed voiceprint section and unconfirmed cache section) need state tracking, icon/style changes, second-press pause toggle, failure-state handling; Storage section needs a "Remove all" button with confirmation dialog (reusing `ConfirmReplaceDialog` pattern with destructive styling)
- `frontend/src/hooks/useAudioPlayer.ts` — already exposes `isPlaying`, `endedCount`, `error`, `pause()`, and `playRange()`; no changes required
- `frontend/src-tauri/src/database/repositories/speaker.rs` + `frontend/src-tauri/src/database/speaker_commands.rs` + `frontend/src-tauri/src/lib.rs` — new repository method and Tauri command `clear_all_voiceprints` / `clear_all_speaker_embeddings` that executes `DELETE FROM speaker_embeddings` (and clears related in-memory `PrototypeStore` caches if present) and returns affected counts; registered as Tauri command
- `frontend/src/services/recordingService.ts` (if used) — optional wrapper for the new command
- Uses `lucide-react` icons already imported elsewhere in the component tree (Play, Pause, plus AlertCircle/XCircle for failure, Trash2 for bulk delete)
