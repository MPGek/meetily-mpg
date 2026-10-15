## ADDED Requirements

### Requirement: Clip-level play button playback state indicator
The VoiceprintBrowser "Play clip" buttons SHALL display a visual indicator when their associated clip is actively playing, and SHALL revert to the default appearance when playback ends or a different clip is selected.

#### Scenario: Button shows playing state during active playback
- **WHEN** the user clicks a "Play clip" button and the associated clip begins playing
- **THEN** the button SHALL change its appearance to indicate active playback (e.g., icon change to Pause, color/background change)

#### Scenario: Button reverts when playback ends naturally
- **WHEN** playback of a clip finishes and the audio element reaches the end
- **THEN** the button SHALL revert to its original "Play clip" appearance

#### Scenario: Button reverts when a different clip is selected
- **WHEN** the user clicks a different "Play clip" button while another clip is playing
- **THEN** the previously-playing button SHALL revert to its original appearance, and the newly-selected button SHALL enter the playing state

#### Scenario: Only one clip button shows playing state at a time
- **WHEN** multiple "Play clip" buttons are visible in the VoiceprintBrowser
- **THEN** at most one button SHALL display the playing-state indicator at any given time

#### Scenario: Button pauses playback on second press when playing
- **WHEN** the user clicks the same "Play clip" button again while its clip is actively playing (`playingRowId === row.id && isPlaying`)
- **THEN** the system SHALL pause playback and the button SHALL revert to its idle appearance (Play icon, default blue style)

#### Scenario: Button shows failure state when playback cannot start
- **WHEN** playback for a clip fails to start (e.g., missing audio file, `get_meeting_audio_path` returns empty, `play()`/`playRange()` rejects, or `useAudioPlayer.error` becomes non-null)
- **THEN** the button for that row SHALL display a distinct failure indicator (e.g., red background/border, failure icon such as AlertCircle/XCircle) instead of the playing or idle state
- **THEN** the failure indicator SHALL clear when the user initiates a different playback or retries the same clip successfully

### Requirement: Bulk removal of all voiceprints and cached embeddings
The VoiceprintBrowser SHALL provide a single destructive "Remove all voiceprints & caches" action that, after explicit confirmation, deletes all rows from `speaker_embeddings` (both enrolled prototypes with `speaker_id` set and unassigned caches with `meeting_id`/`cluster_label`), and SHALL reflect the deletion immediately in the UI without requiring a page reload. The action logically belongs to `speaker-identity-registry`; this delta hosts it in `meeting-audio-player` for the current change scope (a dedicated `specs/speaker-identity-registry/spec.md` delta should be created via `/opsx-continue` on next iteration).

#### Scenario: Remove-all button is visible in Storage header
- **WHEN** the VoiceprintBrowser is rendered
- **THEN** a "Remove all voiceprints & caches" button (e.g., with Trash2 icon, destructive red styling) SHALL be visible in the Storage section header, disabled when `prototype_count + cache_count == 0` and enabled otherwise

#### Scenario: Confirmation dialog appears before deletion
- **WHEN** the user clicks "Remove all voiceprints & caches"
- **THEN** the system SHALL open a modal confirmation dialog that describes the destructive scope ("This will permanently delete N prototypes and M cached embeddings for all speakers/meetings and cannot be undone"), shows the current counts from `speaker_storage_stats`, and requires explicit Confirm vs Cancel

#### Scenario: Confirm deletes all embeddings and refreshes UI
- **WHEN** the user confirms in the dialog
- **THEN** the system SHALL invoke the bulk-clear command that executes `DELETE FROM speaker_embeddings` (clearing both prototypes and caches and any in-memory `PrototypeStore`), close the dialog, refresh `speaker_storage_stats` and `list_voiceprints` (so confirmed/unconfirmed lists become empty), clear any `playingRowId`/`failedRowId` playback state, pause any ongoing playback, and show a success notification

#### Scenario: Cancel leaves data untouched
- **WHEN** the user cancels or dismisses the confirmation dialog
- **THEN** no deletion SHALL occur, the dialog SHALL close, and the existing voiceprint lists and counts SHALL remain unchanged

#### Scenario: Dialog is inaccessible during empty store
- **WHEN** the store is already empty
- **THEN** the "Remove all" button SHALL be disabled and clicking it SHALL NOT open the confirmation dialog
