## Context

Offline diarization (the "Speakers" button) decodes the stereo recording and passes `decoded.samples` — raw interleaved stereo `[mic₀, sys₀, mic₁, sys₁, ...]` at 48kHz — directly into `run_sherpa_diarization`, which expects a mono 16kHz stream. The sinc resampler blends both channels into one signal; since the mic channel usually dominates, only local speakers emerge and remote participants collapse. Additionally, `compute_speaker_matches` hard-codes all `source_device="System"` transcripts to the `"SystemAudio"` label, so remote speakers could never be separated even with correct input.

Existing building blocks:
- `DecodedAudio::extract_channels()` (decoder.rs:118) de-interleaves stereo into `(left, right)` mono streams
- Transcripts carry `source_device` (`"Microphone"` / `"System"`) set during live transcription
- The ring buffer keeps mic/sys windows time-synchronized, so transcript timestamps map 1:1 to both channels

## Goals / Non-Goals

**Goals:**
- Separate local (mic) and remote (system) speaker attribution in offline diarization
- Namespaced speaker IDs that don't collide between the two runs and persist per-channel renames
- Graceful handling of mono recordings (all remote)
- Correct frontend rendering of both namespaces (fixing the "Speaker NaN" wart)

**Non-Goals:**
- Online/streaming diarization — covered by the `online-speaker-diarization` change, which adopts the same naming scheme
- Speaker count auto-detection improvements (FastClustering threshold stays 0.5)
- Overlapping speech diarization (pyannote limitation)

## Decisions

### Decision 1: De-interleave once, run diarization twice, reuse one diarizer instance

**Chosen**: `run_diarization_blocking` calls `extract_channels()` after decoding. For stereo: run `run_sherpa_diarization` on the mic stream and on the sys stream, sharing a single `OfflineSpeakerDiarization` instance (it is stateless per `process()` call, so models load once). For mono (`right == None`): run once on the left stream.

**Rationale**: Two independent runs give each channel its own cluster space; the channel assignment is known a priori, so no cross-channel clustering is needed. Loading the ONNX models once avoids doubling the (slow) model-load cost.

**Alternative considered**: Proper stereo→mono downmix and a single run. Rejected — remote speakers are indistinguishable in a mix; the whole point is separating them.

### Decision 2: Namespaced IDs `MIC_SPEAKER_NN` / `SPEAKER_NN`

**Chosen**: Mic-channel clusters map to `MIC_SPEAKER_00..NN`; system-channel clusters map to `SPEAKER_00..NN`. Cluster indices restart at 0 in each run; the prefix makes IDs globally unique.

**Rationale**: The `speaker_names` rename map is keyed by ID string, so user renames stay per-channel automatically. Existing meetings with `SPEAKER_NN` IDs remain valid (no migration). System transcripts get the plain `SPEAKER_NN` namespace per product decision ("remote = Speaker, mic = Mic Speaker").

**Alternative considered**: Offset numbering (`SPEAKER_10+` for system). Rejected — implicit and confusing in the UI ("Speaker 11" for the second remote speaker). Alternative: display-ready strings as IDs. Rejected — breaks the rename-persistence keying.

### Decision 3: Mono recordings are all remote

**Chosen**: When the decoded file has one channel (or the system channel is absent), run diarization once and label every matched transcript `SPEAKER_NN` regardless of `source_device`.

**Rationale**: Mono files predate channel separation (imported audio, older recordings); there is no separate local/remote signal to recover, and "all remote" is the agreed product behavior.

### Decision 4: Frontend renders both namespaces

**Chosen**: `formatSpeakerId` and `getSpeakerColor` in `VirtualizedTranscriptView.tsx` parse both `MIC_SPEAKER_` and `SPEAKER_` prefixes (falling back to plain parseInt). The legacy `SystemAudio` value renders as "System Audio" with a fixed color.

**Rationale**: The current `parseInt(speaker.replace("SPEAKER_", ""))` yields `NaN` for any new prefix — silently rendering "Speaker NaN" with an undefined color. This change fixes both the new namespaces and the pre-existing legacy wart.

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| **2× inference time** for offline diarization (two runs) | Accepted: explicit button action with existing progress events (diarizing 20% → matching 70%); model load happens once; per-channel runs are sequential |
| **System channel mixes all remote participants** — pyannote may under-split or over-split the remote side | No structural mitigation; clustering threshold stays at 0.5; behavior is strictly better than today's single "SystemAudio" label |
| **Legacy `SystemAudio` transcripts** remain in old meetings | Cosmetic only; UI now renders them as "System Audio"; re-analysis re-labels them |
| **Time-sync drift between channels** could misalign transcripts to segments | Ring buffer already synchronizes mic/sys windows before interleaving; de-interleaving preserves frame order |
