## Context

VoiceprintBrowser has two sets of "Play clip" buttons (confirmed voiceprint prototypes at line 526, unconfirmed caches at line 624) and a Storage header (line ~443) showing `speaker_storage_stats` counts. These buttons currently use plain text with no visual feedback during playback. The component already uses `useAudioPlayer` (line 252) which exposes `isPlaying`, `endedCount`, `error`, `pause()`, and `playRange()`. The `handlePlay` function (line 305) sets `pendingRange` and `audioPath`, triggering playback via a `useEffect` (line 277). Implemented state `playingRowId: string | null` tracks the active row, and buttons show Play/Pause icons with `bg-blue-500` (idle) vs `bg-blue-700` (playing). Storage stats come from `SpeakerRepository::speaker_storage_stats` (`COUNT(*) prototypes/caches + SUM(LENGTH(embedding))`). Today clearing the store requires per-row `reject_voiceprint` calls; there is no bulk "remove all" command. A new command will execute `DELETE FROM speaker_embeddings` and clear the in-memory `PrototypeStore`.

The transcript view (`VirtualizedTranscriptView`) already implements a Play/Pause icon pattern with color changes for active segments — the same approach should be applied here for consistency. `useAudioPlayer` also exposes `error: string | null` on load/play failure and `pause()` for toggling. The existing `ConfirmReplaceDialog` pattern in VoiceprintBrowser provides a reusable modal confirmation for destructive whole-corpus actions.

## Goals / Non-Goals

**Goals:**
- Show a visible playing-state indicator on the "Play clip" button that is currently playing
- Revert the button when playback ends naturally or when a different clip is selected
- Support pause-on-second-press: clicking the same button while playing pauses playback and reverts the button
- Show a distinct red failure state with failure icon when playback cannot start
- Provide a safe, confirmed bulk removal of all voiceprints and cached embeddings from the Storage header
- Maintain consistency with the existing Play/Pause pattern in VirtualizedTranscriptView and with existing confirmation dialogs

**Non-Goals:**
- Changing the playback engine or audio loading behavior
- Adding a loading/spinner state while audio is being resolved (the existing pending-range mechanism handles this)
- Modifying the AudioPlayer bar or transcript segment play buttons (they already work)
- Auto-retry or toast for playback failure beyond the button's red state (global error banner already exists)
- Selective bulk delete (e.g., by speaker or by meeting) — only "remove all" is in scope
- Undo/restore for bulk deletion — once confirmed, deletion is permanent

## Decisions

### Track the currently-playing row by ID

**Decision:** Add a `playingRowId: string | null` state variable to VoiceprintBrowser. Set it in `handlePlay` and clear it when playback ends or is paused.

**Rationale:** The component already has `useAudioPlayer` which exposes `isPlaying` and `endedCount`. By storing which row initiated playback, we can compare each button's `row.id` to `playingRowId` to determine visual state. This avoids prop-drilling or context changes.

**Alternatives considered:**
- Using `player.isPlaying` alone: insufficient because multiple buttons exist and we need to know *which* row is playing, not just whether anything is playing.
- Lifting state to a shared context: overkill for a single component with two button sections.

### Use Play/Pause icons from lucide-react

**Decision:** Import `Play` and `Pause` from `lucide-react` (already a project dependency, used in VirtualizedTranscriptView and AudioPlayer). Show `Pause` icon when the button's row is playing, `Play` icon otherwise.

**Rationale:** Consistent with existing UI patterns in the codebase. Users already understand Play/Pause semantics from the transcript view.

### Visual style matching VirtualizedTranscriptView

**Decision:** Apply these styles per button state:
- **Playing** (`playingRowId === row.id && player.isPlaying`): blue background (`bg-blue-700`), white text, Pause icon
- **Idle** (not playing, no failure): original blue button (`bg-blue-500`) with Play icon
- **Failure** (`failedRowId === row.id`): red background (`bg-red-500` / `bg-red-600`), white text, AlertCircle (or XCircle) icon
- **Disabled** (no audio times): original gray styling (`bg-gray-100`)

Priority order when evaluating a row: Disabled > Failure > Playing > Idle. Failure takes precedence over playing so a failed row does not briefly flash the playing style.

**Rationale:** Matches the existing blue color scheme already used for these buttons. Red for failure is a universal error affordance and distinct from blue playing and gray disabled.

### Clear playingRowId on playback end

**Decision:** Watch `player.endedCount` via a `useEffect`. When it increments, clear `playingRowId`.

**Rationale:** `endedCount` increments each time the HTML audio element fires the `ended` event naturally. This cleanly handles both natural completion and the case where `playRange` auto-stops at the range end.

### Toggle pause on second press

**Decision:** At the top of `handlePlay`, if `playingRowId === row.id && player.isPlaying`, call `player.pause()` and set `playingRowId` to `null` (and clear any `failedRowId` for that row), then return early without setting `pendingRange`/`audioPath`. This makes the button act as Play/Pause toggle for the active clip.

**Rationale:** Users expect a second click on a playing item to pause. Checking the same `playingRowId && isPlaying` condition used for the visual state keeps logic consistent. Early return prevents re-triggering `playRange` for the same range.

**Alternatives considered:**
- Ignoring second press: would require user to wait for range end or click another clip; poor UX.
- Always restarting on second press: would never allow pause; contradicts transcript view behavior where active segment can be toggled.

### Failure state tracking via failedRowId

**Decision:** Add `failedRowId: string | null` state. Set it in three places: (1) when `get_meeting_audio_path` returns empty, (2) in `handlePlay` catch block, (3) via a `useEffect` watching `player.error` — when `player.error` becomes non-null and `playingRowId` is set, copy `playingRowId` to `failedRowId` and clear `playingRowId`. Clear `failedRowId` at the start of any new `handlePlay` attempt (before setting `playingRowId`) and when `player.error` clears or `audioPath` changes. Import `AlertCircle` (or `XCircle`) from `lucide-react` for the failure icon.

**Rationale:** `useAudioPlayer` already surfaces failures through `error` state (load error, `play()` rejection, transcode failure). Mirroring that into a row-specific `failedRowId` lets the button render a per-row red state without coupling the whole component to a global error banner. Clearing on next attempt ensures the red state is transient and retryable.

**Alternatives considered:**
- Using `player.error` alone without row tracking: would not know which of many buttons failed; could incorrectly highlight all buttons.
- Using toast only: already exists for some errors but does not give per-button feedback at the point of interaction.

### Bulk removal of all voiceprints and cached embeddings

**Decision:** Add a destructive "Remove all voiceprints & caches" button to the Storage header (next to the existing counts, using `Trash2` icon, `bg-red-100`/`bg-red-600` styling with `title`/`aria-label`). The button is disabled when `stats.prototype_count + stats.cache_count == 0`. On click, open a confirmation dialog reusing the `ConfirmReplaceDialog` pattern (fixed overlay, `role="dialog"`, `aria-modal="true"`): it displays the current counts from `stats`/`speaker_storage_stats`, warns "This will permanently delete N prototypes and M cached embeddings for all speakers/meetings and cannot be undone," and offers Cancel (gray) and "Confirm delete all" (red `bg-red-600`). On confirm, invoke a new Tauri command `clear_all_voiceprints` (or `clear_all_speaker_embeddings`) implemented in `SpeakerRepository` as `DELETE FROM speaker_embeddings` (no WHERE), also clearing the in-memory live `PrototypeStore`/`meeting_speakers` caches if present, returning `{ deleted_prototypes, deleted_caches }`. After success: close dialog, call `player.pause()` and clear `playingRowId`/`failedRowId`/`pendingRange`, reload `list_voiceprints` and `speaker_storage_stats` (via existing `load()`), and show success feedback (e.g., `toast.success` or `window.alert` consistent with existing replace flow). On cancel/dismiss, perform no deletion.

**Rationale:** Users need a single confirmed step to reset the voiceprint store for privacy hand-off or testing; per-row deletion is tedious and error-prone. Reusing the existing confirmation dialog keeps accessibility (focus trap, Escape, overlay click) and visual language consistent with "Replace speaker across meetings." A single unconditional `DELETE` is atomic, fast (even for 1000+ embeddings), and avoids WHERE-clause mistakes; counts from `speaker_storage_stats` give the user accurate pre-confirmation visibility. Disabling the button when empty prevents a no-op destructive prompt.

**Alternatives considered:**
- Per-speaker/per-meeting bulk deletes: more flexible but adds UI complexity and still leaves the "clear everything" use case unaddressed; deferred.
- Hard delete of `speakers` registry rows as well: intentionally NOT done — the registry (`speakers` table) is separate from embeddings; deleting embeddings already removes recognition capability while preserving named identities for future re-enrollment. A future option could add "delete orphaned speakers" but is out of scope.
- Soft-delete flag: adds schema churn and retention ambiguity; permanent delete matches existing `reject_voiceprint(permanent=true)` semantics and user expectation for a "remove all" reset.

## Risks / Trade-offs

- **Race condition on rapid clicks:** If a user clicks two different "Play clip" buttons in quick succession, the first clip's audio may still be loading when the second is clicked. The existing `pendingRange` mechanism handles this (the new `setPendingRange` call overwrites the previous one), and `playingRowId` will be set to the second row immediately, so the first button will never show the playing state. This is acceptable. The pause-toggle check uses `isPlaying`, so a rapid double-click before `isPlaying` becomes true will still be treated as a second attempt to play, not a pause — acceptable because playback has not yet started.

- **Audio path change while playing:** If `handlePlay` resolves a different `audioPath` (different meeting), `useAudioPlayer` reloads the audio element, which resets `isPlaying` to false and `error` to null momentarily. The `playingRowId` will already be set to the new row, so the visual transition is correct. `failedRowId` is cleared on path change to avoid stale red state.

- **Range-end pause vs ended event:** `useAudioPlayer` pauses (not ends) when `currentTime >= rangeEnd`. This does not increment `endedCount`, so `playingRowId` would stay set but `isPlaying` becomes false, causing the button to visually revert to idle via the `isPlaying` condition. `failedRowId` is not affected. This is acceptable; the `endedCount` watcher still handles natural file-end.

- **No loading indicator:** The button does not show a "loading" state while the audio file is being resolved. This matches the current behavior and is acceptable because the audio resolution is typically fast (local file path lookup). Adding a spinner would be a separate enhancement. Failure state (red) covers the case where loading never succeeds.

- **Destructive bulk delete has no undo:** `DELETE FROM speaker_embeddings` is irreversible. Mitigation: confirmation dialog with explicit counts and permanent-delete warning, disabled button when empty, and no hotkey/shortcut. The `speakers` registry itself is NOT deleted, so names can be reused. Future enhancement could add an export-before-delete step, but is not required for the current reset use case.

- **Concurrent clear while playing:** If bulk clear is confirmed while a clip is playing, the underlying embeddings are removed but the audio element still holds the decoded range. Mitigation: `handleClearAll` pauses playback and clears `playingRowId`/`failedRowId` before/after the delete, so the UI does not show a playing state for a now-deleted voiceprint.

- **PrototypeStore in-memory stale cache:** Live Fast-mode `PrototypeStore` may retain loaded embeddings after DB delete. Mitigation: repository method also clears the in-memory store (or the command invokes `clear_prototype_store`) so subsequent recognition does not use deleted prototypes.
