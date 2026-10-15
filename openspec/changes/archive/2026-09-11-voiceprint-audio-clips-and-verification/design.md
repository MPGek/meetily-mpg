## Context

See proposal.md Why. Current state: `speaker_embeddings` holds only a 192-f32 embedding blob plus provenance (`meeting_id`, `cluster_label`, `audio_start/end`); `VoiceprintBrowser.tsx` plays via `get_meeting_audio_path` + `useAudioPlayer.playRange()`. `SpeakerRepository::write_cluster_cache` receives only `Exemplar { embedding, duration, start, end }` — no PCM. Shared encoder `audio/encode.rs::encode_single_audio` pipes f32 PCM to bundled ffmpeg (AAC VBR today). Migration precedent: table rebuild in `20260819000000_add_voiceprint_provenance.sql`.

## Goals / Non-Goals

**Goals:**
- Self-contained clip per voiceprint row, playable with no meeting file.
- Verify workflow that hides already-checked rows.
- Backward compatible: legacy rows work with degraded states.

**Non-Goals:**
- Fixing the underlying checkpoint-concat / TimelineMapper time drift (separate bug; clips sidestep it but do not fix transcripts).
- Changing recognition scoring, thresholds, enrollment caps, or matcher weighting.
- Backfilling clips for all legacy rows automatically.

## Decisions

### D1. Opus mono 16 kHz ~24 kbps voip, Ogg container, ~15 s cap
Rationale: 24k is transparent for voice with headroom for noise/overlap; 12–16k saves ~15 KB/clip but risks false rejects on noisy clips. 16 kHz matches the diarization sample rate (no wasted bits above 8 kHz voice band). Ogg-Opus plays natively in Chromium/WebView2 `<audio>`.
Alternatives: keep AAC (larger at equal quality for speech, and couples clips to the meeting-file codec); raw PCM blob (10x larger); 12 kbps (rejected: too lossy for validation confidence).

### D2. Clip bytes as columns on `speaker_embeddings`
Add `audio_blob BLOB`, `audio_codec TEXT DEFAULT 'opus'`, `audio_sample_rate INTEGER DEFAULT 16000`, `is_verified INTEGER DEFAULT 0`, `verified_at TEXT`. Same-table keeps voiceprint+clip lifecycle atomic (reject/delete/enroll/clear_all keep working); no new FK graph. Size (~30–45 KB/row) is fine for SQLite at the 64-cap scale.
Alternatives: sidecar files (break on DB move, orphan cleanup), separate 1:1 table (extra join + cascade risk for no benefit now).

### D3. PCM sourcing via second-pass decode, not in-memory retention
`write_cluster_cache` has no PCM; retaining a full-meeting f32 buffer (~230 MB/hour mono) is unacceptable. At persist time, re-decode only the needed `[start,end]` windows per channel (ffmpeg `-ss/-to`, 16 kHz mono demux: mic=left, system=right) into a temp PCM slice, then encode to Opus. Deterministic and independent of `audio.mp4` checkpoint drift.
Alternatives: thread the live decode buffer through clustering (chicken-egg: exemplars known only post-cluster; large lifetime); slice from saved `audio.mp4` (re-inherits the shifted-marks bug this change escapes).

### D4. New `audio/clip_encode.rs` reusing `run_ffmpeg_with_timeout` pattern
Slice → sanitize non-finite → pipe to `libopus` (`-c:a libopus -b:a 24k -vbr on -application voip -ar 16000 -ac 1 -f ogg`). Guard with ENCODE_TIMEOUT-style bound. Spike must confirm `libopus` in bundled ffmpeg; fallback documented as AAC-HE 24k only if missing.
Alternatives: in-process opus crate (new native dep, no ffmpeg reuse); reuse AAC path (worse speech efficiency).

### D5. Blob playback via `get_voiceprint_audio` temp-file cache
Command takes voiceprint `id`, writes blob to temp dir keyed by `(id)` (content-addressed: rewrite only when row changes), registers asset-protocol scope, returns path; frontend `convertFileSrc` + direct `play()` (no `playRange`). Legacy rows keep `get_meeting_audio_path` + range path.
Alternatives: base64-over-IPC (JSON bloat for 45 KB × N rows); data-URL per list call (same bloat; temp file keeps `list_voiceprints` light — add only `has_audio` boolean to `VoiceprintRow`).

### D6. Verify is display-only flag
`verify_voiceprint(id)` / `verify_speaker(speaker_id)` do bare `UPDATE is_verified/verified_at`; no re-enrollment, no matcher change, no binding change. `reconfirm_voiceprint` resets target row to unverified. `list_voiceprints` returns `is_verified` + per-group `unverified_count`; filter applied client-side with server passing flags through.
Alternatives: reuse `matched_by='user'` (wrong layer: that is cluster binding, not clip review); weighted matcher for verified rows (deferred to follow-up).

## Risks / Trade-offs

- [Bundled ffmpeg lacks libopus] → Spike first (`ffmpeg -encoders | grep opus`); fallback to AAC-HE documented, spec codec field absorbs it.
- [Second-pass decode cost on long meetings] → Only exemplar windows (≤8/meeting-cluster, ≤15 s each) decoded; bounded and off the hot path.
- [DB growth] → ~2 MB per 5-cluster meeting; mitigated by clip cap + audio-bytes visibility; caches rewritten on rediarize so no unbounded duplication.
- [WebView2 Opus playback gap] → Spike playback of Ogg-Opus via `<audio>`; temp-WAV transcode fallback already exists in `useAudioPlayer.onError`.
- [Enrollment reparenting vs clip copy] → `enroll_cluster` reparents rows (clip+flag travel together); `enroll_embeddings_from_buffer` inserts new rows with freshly cut clips; cap pruning deletes whole rows (no orphan blobs).

## Migration Plan

1. Spike: libopus presence + Ogg-Opus `<audio>` playback.
2. Migration: table rebuild adding 5 columns (NULL blob / 0 flag = legacy state); recreate existing indexes; no CHECK change.
3. Land repository + encode + command + UI behind existing browser (legacy fallback keeps old rows usable).
4. Rollback: revert migration (blobs dropped with columns); app reads legacy columns only.

## Open Questions

- None blocking specs or task breakdown. Bitrate tuning (24k vs 32k) can follow listening tests without spec changes via the `audio_codec`/bitrate constant.

## Spike results (tasks 1.1–1.2, verified 2026-09-08)

- **1.1 libopus**: both the system ffmpeg and the bundled
  `frontend/src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` report
  `libopus` (`-encoders | grep opus`). No AAC fallback needed; D4 stands.
  Ogg-Opus plays natively in Chromium/WebView2 `<audio>`; the existing
  `useAudioPlayer.onError` WAV-transcode fallback covers edge cases.
- **1.2 PCM slicing points**: offline (`diarization.rs`) and online
  (`finalize_online_session` in `recording_commands.rs`) both funnel through
  `persist_and_recognize_session` → `persist_channel_clusters` →
  `SpeakerRepository::write_cluster_cache`, which receives per-exemplar
  `(start_secs, end_secs)` windows. Clip cutting hooks into
  `persist_channel_clusters` (meeting audio file resolvable via
  `meetings.folder_path` + `find_audio_file` at that point); `enroll_cluster`
  reparents rows so clips travel with enrollment; `reconfirm_voiceprint`
  resets `is_verified`.
