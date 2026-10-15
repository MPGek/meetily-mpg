## Context

See `proposal.md` — Why. Two independent facts shape the approach:

1. `decode_audio_file` already corrected a metadata-vs-decoded channel mismatch (commit `585a96a`: "File metadata can report an incorrect channel count ... detects the actual channel count from the first decoded buffer and overrides metadata"). `probe_audio_metadata` (`audio/decoder.rs`) is a header-only fast path that was left untouched and still returns `channels.unwrap_or(1)`. It gates the mono/stereo split in offline diarization (`audio/diarization.rs`) and the per-channel span source used by word alignment (`audio/recording_commands.rs`).
2. Retranscription (`audio/retranscription.rs`) deletes and re-inserts all transcript rows without speaker attribution, while leaving `meeting_speakers` rows in place. Display provenance (`speaker_matched_by`, `speaker_match_score`) is resolved at read time by joining `transcripts.speaker` to `meeting_speakers.cluster_label`, so after retranscription nothing renders attribution or confidence until diarization runs again.

## Goals / Non-Goals

**Goals:**
- A stereo recording is never diarized or aligned as a single downmixed stream because of missing/wrong container metadata.
- The `Enhance → Speakers` sequence produces channel-correct, consistent speaker attribution and provenance, and does not lose user-confirmed identities.

**Non-Goals:**
- Changing clustering/recognition thresholds, model selection, or the diarization UX.
- Automatically repairing historical meetings in the database (they can be fixed by re-running speaker analysis).
- Changing how live/online diarization decides channels (it does not use the metadata probe).

## Decisions

### D1: Detect channel layout from decoded audio, never default to mono
Extend the header-only probe into a shared channel-layout detection that falls back to decoding the first audio packet (Symphonia `decoder.spec().channels.count()`) when `codec_params.channels` is `None`, and never coerces unknown to 1. Callers receive the decoded layout and treat `>= 2` as stereo. When even the first-packet decode cannot determine the count, the layout is "unknown" and callers MUST NOT downmix — offline diarization passes the native stream to the channel splitter instead of `ffmpeg -ac 1`.

*Alternative considered:* shell out to `ffprobe`. Rejected: ffmpeg is optional (`find_ffmpeg_path` may return `None`), the probe is deliberately header-only/fast, and the existing decode fallback already has the correct answer without a new dependency.

*Alternative considered:* keep `unwrap_or(1)` and instead detect "did the downmix lose a channel" later. Rejected: the downmix is destructive and upstream of clustering; detection must happen before the split.

### D2: One detection point, three consumers
The shared detection is used by (a) offline diarization's mono/stereo split, (b) the word-alignment per-channel span source, and (c) retranscription's own per-channel path (already correct via `decode_audio_file`, kept as the decoded-layout reference). This keeps `source_device` routing, `MIC_SPEAKER_*`/`SPEAKER_*` namespacing, and per-channel refinement mutually consistent.

### D3: Retranscription re-establishes attribution instead of dropping it
After inserting new rows, retranscription re-establishes speaker attribution:
- Per-block user overrides (`speaker_override_id`) are carried forward by time overlap onto the new rows, since the recording time base is unchanged.
- Cluster-level attribution is re-derived by running the existing offline diarization path when diarization is enabled with auto-run (mirroring recording-stop behavior); otherwise the meeting is marked as needing analysis and the UI surfaces the existing "Speakers" affordance.
- Stale `meeting_speakers` rows whose cluster labels are not produced by the new run MUST NOT silently drive display; the read-time join is only meaningful once labels correspond.

*Alternative considered:* pure carry-forward of cluster labels by overlap. Rejected: retranscription re-segments with different VAD boundaries, so label-to-label overlap is approximate and can attach an old identity to the wrong new cluster.

*Alternative considered:* leave Enhance as-is and rely on the user pressing Speakers. Rejected: it is exactly the observed failure — Enhance destroys labels and the recovery run then downmixes.

### D4: Serialize retranscription and diarization
Both jobs mutate the same transcripts / `meeting_speakers`. Add a shared guard so a diarization run cannot start while retranscription is writing (and vice versa), and so the auto re-run after retranscription is sequenced after the insert transaction commits. The frontend "Speakers" control reflects that state.

## Risks / Trade-offs

- [Re-running diarization after Enhance adds cost] → Only auto-run when diarization is enabled with auto-run; otherwise mark stale and prompt, and reuse the existing progress UI.
- [First-packet decode in the probe adds latency] → Only on the metadata-missing path; header fast path is unchanged for files that report channels.
- [Carrying overrides by overlap can misplace a block when VAD boundaries shift] → Overlap-only carry-forward for user overrides (never auto identity), keep `matched_by='user'` provenance, and let the subsequent run refine cluster attribution.
- [Unknown layout path when both metadata and first-packet decode fail] → Do not downmix; pass the native stream so a stereo file is at worst split correctly rather than collapsed, and surface an actionable error if the stream cannot be interpreted.
- [Historical mono-confused meetings remain wrong until re-analyzed] → Non-goal; document that re-running speaker analysis on them now takes the channel-correct path.

## Migration Plan

- No schema migration. `meeting_speakers.channel` values written by a mono-confused run stay wrong until re-analysis; after the fix, re-running speaker analysis rewrites them correctly.
- Rollback: revert the detection and retranscription changes; behavior returns to the metadata-only probe.
