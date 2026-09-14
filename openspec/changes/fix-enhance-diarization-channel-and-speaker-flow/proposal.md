## Why

Offline diarization (the "Speakers" button) decides stereo vs mono from container metadata via `probe_audio_metadata`, which has no channel information for common AAC-in-MP4 recordings and falls back to `.unwrap_or(1)`. A stereo recording is then silently downmixed (`ffmpeg -ac 1`), so the microphone and system channels are clustered as one stream: a single cluster label is applied to both Microphone and System transcripts, system-audio speakers are recognized and enrolled as the local microphone speaker, and the auto-match scores that drive the `(auto) N%` labels and confirmation affordance never reach the transcripts that need them.

The common user sequence "Enhance, then Speakers" makes this guaranteed breakage: Enhance deletes and re-inserts all transcripts without speaker attribution, so Speakers is the only way to recover labels — and it runs on the downmixed stream. Observed on the 2026-09-14_11-31 recording: offline diarization wrote 20 `SPEAKER_NN` clusters all tagged `channel='mic'` and no `MIC_SPEAKER_*`, assigned the same `SPEAKER_00` to both Microphone and System rows, and enrolled system-channel exemplars into "Vasiliy Kotov", who was present only on the microphone channel.

## What Changes

- Channel layout for offline per-channel processing SHALL be determined from the actually decoded audio, not from container/header metadata, and mono SHALL apply only to genuinely single-channel recordings. A stereo recording SHALL never be downmixed into one diarization stream.
- Offline diarization SHALL use that reliable detection when deciding to split microphone/system channels, so `MIC_SPEAKER_*` / `SPEAKER_*` namespacing and `source_device`-routed matching stay consistent.
- Word-level alignment's per-channel repair span source SHALL use the same reliable detection, so refined token times are mapped to the correct channel.
- Enhance SHALL NOT leave a meeting with silently discarded speaker attribution and stale cluster bindings. After a successful retranscription the transcript SHALL re-establish speaker attribution and provenance (carrying user-confirmed identities forward and/or re-running channel-correct diarization), so the `(auto) N%` labels and confirmation affordance behave as before and no system-channel voiceprint is attributed to a microphone-only speaker.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `stereo-channel-extraction`: channel-layout detection for extraction must come from decoded audio, not header metadata, and must not downgrade a stereo source to mono when metadata lacks channel info.
- `speaker-diarization`: the offline microphone/system split and the mono-recording behavior must be driven by that reliable detection; diarization must never downmix a stereo recording into a single stream.
- `ctc-word-alignment`: the offline per-channel refinement span source must use the reliable channel detection, so refinement and the token N-way split operate per channel.
- `source-labeled-retranscription`: retranscription must preserve or re-establish speaker attribution and provenance instead of deleting it and leaving stale cluster bindings.

## Impact

- Backend: `audio/decoder.rs` (`probe_audio_metadata` and its callers), `audio/diarization.rs` (channel-split decision, mono fallback), `audio/recording_commands.rs` (`meeting_span_source`), `audio/retranscription.rs` (speaker attribution preservation / re-run), speaker/diarization repositories for stale-binding cleanup.
- Data: `meeting_speakers` (mislabeled `channel` on mono-confused runs), `speaker_embeddings` (enrollment provenance), `transcripts.speaker`/`speaker_override_id`.
- Affected recordings: at least 6 recent meetings show the mono-offline signature (`SPEAKER_%` + `channel='mic'`), including 2026-09-11_16-21, 2026-09-09_16-45, 2026-09-09_14-47, 2026-09-09_11-50, 2026-09-08_16-16.
- No new external dependencies; no API surface changes beyond internal behavior.
