## Context

The meeting-notes audio player (in-flight change `meeting-notes-audio-player`) plays recordings through `useAudioPlayer.ts` (Web Audio): `read_audio_file` transfers the whole file as a JSON number array over IPC, `decodeAudioData` decodes it into an in-memory PCM `AudioBuffer`, and a `requestAnimationFrame` loop tracks position. Field feedback shows three defects:

1. **Slow load**: a 31-minute stereo recording (~46MB mp4) becomes a ~46M-element JSON array (~300MB) then a ~700MB float32 PCM buffer — seconds of blocking on every meeting open.
2. **Broken pause/seek**: `AudioBufferSourceNode.stop()` fires its own `onended` handler, which resets `currentTime` to 0 on pause and — worse — kills a freshly started source when seeking from a transcript block (the stale `onended` of the old source stops the new one). This requires "pause first" to play from a block.
3. **No live highlight**: only the clicked block is marked; the marker does not follow playback.

Verified platform facts (tauri 2.11.1 in local cargo registry, `@tauri-apps/api` in node_modules):
- `Manager::asset_protocol_scope(&self) -> scope::fs::Scope` exists (`tauri-2.11.1/src/lib.rs:761`) and `Scope::allow_file(path)` exists (`tauri-2.11.1/src/scope/fs.rs:297`) — runtime scope extension is supported.
- `convertFileSrc` is exported from `@tauri-apps/api/core`.
- `tauri.conf.json` currently has `assetProtocol.enable: true` (scope `$APPDATA/**`) and a CSP without `media-src`; recordings live in the user-configurable `Music/meetily-recordings`, outside `$APPDATA`.
- Recordings are AAC-LC 48kHz stereo MP4 (`ffprobe` confirmed) — Chromium's media element plays this natively.

## Goals / Non-Goals

**Goals:**

- Instant-feeling load and bounded memory: stream the recording through an `<audio>` element via the asset protocol.
- Correct pause/resume and play-from-block semantics in every playback state.
- Live highlight of the block being played (mic and system variants), auto-scrolling the list to it as playback moves, cleared when playback ends.
- Keep audio on the system default output device — explicitly no device pinning (e.g. not tied to the Buds3 Pro).
- Keep the FFmpeg transcode fallback for formats the media element cannot decode.

**Non-Goals:**

- Output-device selection UI or per-device routing (Rust cpal playback engine).
- Waveform, speed control, volume UI.
- Highlight/auto-scroll on the home-page live view (historical meetings only, same as play buttons).
- Pausing auto-scroll on user wheel input (follow-up if it feels aggressive).

## Decisions

### D1: Playback engine — HTML `<audio>` element streaming via asset protocol

Replace the Web Audio engine with `<audio src={convertFileSrc(path)}>`. Chromium's media pipeline streams from disk, decodes natively, supports `currentTime` seeking, and has sane `pause()`/`ended` semantics — all three reported defects disappear by construction (no IPC payload, pause keeps position, no `onended` aliasing of `stop()`).

Alternatives considered:
- **Patch the Web Audio hook** (base64 IPC + a stop-intent flag): keeps ~300 lines of state-machine code that already failed once, keeps the ~700MB RAM decode, keeps `decodeAudioData` codec uncertainty. Rejected.
- **Rust cpal streaming player** (symphonia decode + cpal output): the only option that could pin a specific output device, but the requirement is explicitly *not* to pin devices; ~500 lines of decode/ring-buffer code for zero requirement coverage. Rejected.

### D2: Asset protocol access — runtime scope allow + CSP

`get_meeting_audio_path` gains an `AppHandle` parameter and, after resolving the file, calls `app.asset_protocol_scope().allow_file(&path)` so the webview can stream it even though it lives outside the configured `$APPDATA/**` scope (the recordings folder is user-configurable, so a static conf scope cannot cover it). The frontend converts the returned path with `convertFileSrc`. CSP gains `media-src asset: http://asset.localhost https://asset.localhost` (Windows/Linux use `http(s)://asset.localhost`; macOS uses the `asset:` scheme — the existing `img-src` already allows the same set). The same `tauri.conf.json` CSP block applies in dev and prod (no separate `devCsp` key).

Fallback if runtime `allow_file` is rejected for out-of-scope paths (unlikely but possible): read bytes via a base64 variant of `read_audio_file`, build a `Blob` URL, and add `media-src blob:` — implemented only if the spike fails.

### D3: Hook rewrite — same public API, element-owned lifecycle

`useAudioPlayer` is rewritten around a ref to a hidden `<audio>` element rendered by `AudioPlayer.tsx`. The public API stays `{ isPlaying, currentTime, duration, error, load, play, pause, seek }` plus the element ref, so `TranscriptPanel` wiring and `playFrom` change minimally. Element events map to state: `durationchange` → duration, `timeupdate` (~4Hz) → currentTime, `ended` → isPlaying=false + a distinct "ended" signal (used to clear the highlight), `error` → transcode fallback (D6), `play`/`pause` events → isPlaying. Cleanup on unmount: pause and drop the element. The Web Audio code (`AudioContext`, `BufferSource`, rAF loop) is deleted entirely.

### D4: Live highlight — position-driven active segment

`AudioPlayer` reports `currentTime` to `TranscriptPanel` via a callback on `timeupdate` (~4Hz, no extra throttle needed). `TranscriptPanel` derives `activeSegmentId` with a binary search over segments sorted by `audio_start_time`: the segment whose `[timestamp, endTime)` (fallback: next segment's start) contains the position. `VirtualizedTranscriptView` receives `activeSegmentId` and computes a per-row `isActive` **boolean**, so memoized rows re-render only when their own boolean flips (boundary crossing touches exactly two rows, not the list). Paused → highlight stays on the block at the paused position, glyph reverts to play. Natural end → `activeSegmentId` cleared.

### D5: Auto-scroll to the active block

When `activeSegmentId` changes while playing, scroll the transcript container so the segment element (`#segment-<id>`, already present) is visible: `scrollIntoView({ block: 'nearest', behavior: 'smooth' })`. No scrolling while paused and none after end. `block: 'nearest'` keeps motion minimal when the block is already visible.

### D6: Transcode fallback on element error

On the `<audio>` element's `error` event, invoke `prepare_audio_for_playback(filePath)` (existing command, cached by path+mtime in the temp dir), swap `src` to `convertFileSrc(wavPath)`, and reload. A second failure surfaces the existing player error state.

### D7: Default output device only

No output-device APIs are touched. Playback routes through the OS default output endpoint, identical to any web media playback — the player is device-independent.

## Risks / Trade-offs

- [R1: runtime `allow_file` rejects out-of-config paths] → API is verified present in tauri 2.11.1; spike first in the implementation, fallback to base64+Blob (D2) if rejected.
- [R2: `timeupdate` granularity (~4Hz) may visually skip very short blocks] → Highlight target is still exact (binary search); blocks shorter than ~250ms are rare; acceptable.
- [R3: auto-scroll may feel aggressive] → Only on playback-driven changes with `block:'nearest'`; user wheel handling deferred (non-goal).
- [R4: `read_audio_file` becomes unused] → Command stays registered (harmless); removal offered as optional cleanup, not required.
- [R5: archive ordering] → This change amends a capability introduced by the in-flight `meeting-notes-audio-player`; that change must be archived first (or both together) for spec deltas to apply cleanly.
- [R6: CSP change is app-wide] → Scoped to `media-src` with exactly the asset-protocol origins already allowed for `img-src`; no relaxation of other directives.

## Migration Plan

No data or persisted-state changes. Rollback: revert the change; behavior returns to the Web Audio engine (bugs included). The transcode temp cache is unaffected. Deploy: normal app rebuild.

## Open Questions

- Whether auto-scroll should suspend on user wheel input — deferred until the feature is felt.
- Whether `read_audio_file` should be removed entirely (it becomes unused) — leave registered unless asked.
