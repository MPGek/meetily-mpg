## 1. Backend: asset protocol scope registration

- [x] 1.1 In `frontend/src-tauri/src/api/api.rs`, change `get_meeting_audio_path` to accept `app: AppHandle<R>` and, after resolving the audio file, register it in the asset protocol scope via `app.asset_protocol_scope().allow_file(&path)` (spike R1: verify out-of-scope paths are accepted at runtime on tauri 2.11.1)
- [x] 1.2 Verify: `cargo check` and `cargo test --lib audio::` in `frontend/src-tauri` pass

## 2. Config: CSP media-src

- [x] 2.1 In `frontend/src-tauri/tauri.conf.json`, extend the CSP with `"media-src": "'self' asset: http://asset.localhost https://asset.localhost"` (same asset origins already allowed for `img-src`)
- [ ] 2.2 Verify: `convertFileSrc` on Windows/Linux produces `http(s)://asset.localhost/...` and the URL loads in the webview (checked during E2E in section 5)

## 3. Frontend: streaming engine rewrite

- [x] 3.1 Rewrite `frontend/src/hooks/useAudioPlayer.ts` around a hidden `<audio>` element: keep the public API `{ isPlaying, currentTime, duration, error, load, play, pause, seek }` and expose the element ref for `AudioPlayer.tsx` to render; map `durationchange`/`timeupdate`/`play`/`pause`/`ended`/`error` events to state; delete all Web Audio code (`AudioContext`, `BufferSource`, rAF loop)
- [x] 3.2 Add an explicit ended signal (e.g. `onEnded` callback or `endedAt` state) so `TranscriptPanel` can clear the highlight on natural end
- [x] 3.3 In `frontend/src/components/AudioPlayer.tsx`: render the hidden `<audio ref>` element, set `src={convertFileSrc(audioPath)}`, keep play/pause button, seek slider with drag-commit, and time readouts; on the element `error` event invoke `prepare_audio_for_playback` and swap `src` to the returned WAV (retry once; second failure keeps the error state)
- [x] 3.4 In `frontend/src/components/MeetingDetails/TranscriptPanel.tsx`: replace `PlaybackState.startTime`-based logic with position-based state — report `currentTime` up via callback (timeupdate) and call `playFrom(startTime)` through the ref handle as today (semantics become: seek + play, correct in all states)
- [x] 3.5 Verify: `pnpm exec tsc --noEmit` in `frontend/` passes (no new errors)

## 4. Frontend: live highlight and auto-scroll

- [x] 4.1 In `TranscriptPanel.tsx`: derive `activeSegmentId` from `currentTime` with a binary search over the sorted segments (time range `[timestamp, endTime)`, fallback to next segment's start when `endTime` is missing); memoize
- [x] 4.2 Pass `activeSegmentId` + `isAudioPlaying` to `VirtualizedTranscriptView`; compute a per-row `isActive` boolean so memoized rows re-render only on boundary crossings
- [x] 4.3 In `VirtualizedTranscriptView.tsx` `TranscriptSegment` (legacy, mic, system variants): apply the active highlight style to the bubble/row when `isActive`, and show the pause glyph on the play button when `isActive && isAudioPlaying`
- [x] 4.4 Auto-scroll: when `activeSegmentId` changes while playing, scroll the segment element (`#segment-<id>`) into view with `scrollIntoView({ block: 'nearest', behavior: 'smooth' })`; no auto-scroll while paused
- [x] 4.5 Clear `activeSegmentId` when playback ends (natural `ended`), keeping the player time reset
- [x] 4.6 Verify: `pnpm exec tsc --noEmit` in `frontend/` passes

## 5. End-to-end verification

- [x] 5.1 Run the app (`pnpm dev` + `cargo tauri dev`), open a meeting with a 30+ minute recording: player ready quickly (no multi-second hang), duration correct
- [x] 5.2 Pause at a mid position: display keeps the position; resume continues from it (no 0:00 jump)
- [x] 5.3 Click a transcript block's play button while paused: playback starts at that block; click a different block while playing: playback seeks and continues (no stop)
- [x] 5.4 During playback: the current block is highlighted and the list auto-scrolls to it; highlight moves across block boundaries for mic and system blocks; pause keeps highlight with play glyph; natural end clears highlight
- [x] 5.5 Audio plays through the system default output device (e.g. speakers and Bluetooth headphones) with no device pinning
- [x] 5.6 Import a format the media element rejects (e.g. WMA): the player falls back to the transcoded WAV and plays; a forced transcode failure shows the error state
- [x] 5.7 Navigating away from the meeting details page stops playback and releases the media element
