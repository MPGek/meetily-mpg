## 1. State tracking

- [x] 1.1 Add `playingRowId: string | null` state to VoiceprintBrowser, initialized to `null`
- [x] 1.2 In `handlePlay`, set `playingRowId` to the row's `id` before setting `pendingRange` and `audioPath`
- [x] 1.3 Add a `useEffect` that watches `player.endedCount` and clears `playingRowId` to `null` when it increments

## 2. Button visual feedback

- [x] 2.1 Import `Play` and `Pause` icons from `lucide-react` in VoiceprintBrowser
- [x] 2.2 Update the confirmed voiceprint "Play clip" button (line ~526) to show `Pause` icon when `playingRowId === row.id && player.isPlaying`, and `Play` icon otherwise; apply blue highlight style for the playing state
- [x] 2.3 Update the unconfirmed cache "Play clip" button (line ~624) with the same playing-state indicator logic and styles

## 3. Cleanup on clip switch

- [x] 3.1 Verify that clicking a different "Play clip" button correctly updates `playingRowId` to the new row, causing the previous button to revert automatically

## 4. Pause toggle on second press

- [x] 4.1 Update `handlePlay` to toggle pause: if `playingRowId === row.id && player.isPlaying`, call `player.pause()`, clear `playingRowId` (and `failedRowId` if set), and return early without setting `pendingRange`/`audioPath`
- [x] 4.2 Verify second press on the same playing button pauses playback and reverts the button to idle (Play icon, `bg-blue-500`)

## 5. Failure state (red) when playback cannot start

- [x] 5.1 Add `failedRowId: string | null` state, import `AlertCircle` (or `XCircle`) from `lucide-react`, and add effects/handling to set `failedRowId` when `get_meeting_audio_path` returns empty, when `handlePlay` catches, or when `player.error` becomes non-null for the active row; clear `failedRowId` on next play attempt and on `player.error` clear
- [x] 5.2 Update both Play clip buttons (confirmed ~526 and unconfirmed ~624) to show red failure style (`bg-red-500`/`bg-red-600` text-white) with failure icon when `failedRowId === row.id` (precedence: Disabled > Failure > Playing > Idle)
- [x] 5.3 Verify failure indicator appears when playback fails to start and clears on retry or on playing a different clip

## 6. Bulk removal — Remove all voiceprints & caches with confirmation

- [x] 6.1 Add repository method and Tauri command `clear_all_voiceprints` (or `clear_all_speaker_embeddings`) that executes `DELETE FROM speaker_embeddings` (both prototypes and caches), clears in-memory `PrototypeStore` if present, returns `{ deleted_prototypes, deleted_caches }`, and register the command in `lib.rs` (and wrapper in `recordingService.ts` if used)
- [x] 6.2 Add "Remove all voiceprints & caches" button to VoiceprintBrowser Storage header (with `Trash2` icon, destructive red styling, `aria-label`, disabled when `prototype_count + cache_count == 0`) that opens a confirmation dialog (reusing `ConfirmReplaceDialog` pattern) showing current counts and permanent-delete warning with Cancel / "Confirm delete all" (red) actions
- [x] 6.3 Wire confirmation: on Confirm invoke `clear_all_voiceprints`, close dialog, call `player.pause()` and clear `playingRowId`/`failedRowId`/`pendingRange`, reload `list_voiceprints` and `speaker_storage_stats` via `load()`, and show success feedback; on Cancel/dismiss do nothing and keep data intact
- [x] 6.4 Verify bulk delete removes both prototypes and caches (lists empty, counts zero), dialog is disabled when store already empty, playback state is cleared if a clip was playing, and no data is removed on cancel
