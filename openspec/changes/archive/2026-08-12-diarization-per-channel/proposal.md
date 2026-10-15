## Why

The "Speakers" button (offline diarization) reads the stereo recording but feeds the raw interleaved sample stream into the diarization pipeline as if it were mono. The sinc resampler then blends the microphone and system channels together, and because the mic side usually dominates the mix, the diarizer effectively only picks up local (mic) speakers — remote participants are never separated. On top of that, `compute_speaker_matches` hard-codes every system-source transcript to the `"SystemAudio"` label, so even a correct diarization input could never split remote speakers into individuals.

## What Changes

- **Per-channel diarization**: Decode once, then de-interleave the stereo recording (`extract_channels()`) into microphone (left) and system (right) mono streams, and run the sherpa-onnx diarization pipeline independently on each — reusing a single diarizer instance so models load once
- **Namespaced speaker IDs**: Microphone-channel clusters map to `MIC_SPEAKER_00..NN`, system-channel clusters map to `SPEAKER_00..NN` — no index collisions between the two independent runs, and existing `SPEAKER_NN` labels in old meetings keep working
- **Mono fallback**: Mono recordings (imported files, pre-stereo recordings) are diarized once and all matched transcripts are labeled as remote speakers (`SPEAKER_NN`)
- **Per-source matching**: Transcripts with `source_device="Microphone"` (or NULL) match against microphone-channel segments; `source_device="System"` transcripts match against system-channel segments — replacing the `"SystemAudio"` override
- **Frontend label rendering**: `formatSpeakerId`/`getSpeakerColor` handle both ID namespaces, and the legacy `"SystemAudio"` value renders as "System Audio" instead of "Speaker NaN"

## Capabilities

### New Capabilities
- None

### Modified Capabilities
- `speaker-diarization`: Offline diarization processes mic and system channels independently with namespaced speaker IDs; system-source transcripts receive real diarization labels instead of the `"SystemAudio"` override; mono recordings fall back to remote-only labeling; the transcript view renders both ID namespaces

## Impact

- **Affected code**: `audio/diarization.rs` (de-interleave, dual run, per-source matching, ID namespacing); frontend `VirtualizedTranscriptView.tsx` (`formatSpeakerId`, `getSpeakerColor`); `audio/decoder.rs` already provides `extract_channels()` — no changes expected
- **No new dependencies** — sherpa-onnx v1.13.4 and existing models are reused
- **No breaking changes** — DB schema untouched (`speaker` is TEXT, `speaker_names` map is keyed by ID); existing `SPEAKER_NN` labels remain valid; legacy `"SystemAudio"` transcripts are left as-is unless the meeting is re-analyzed
- **Performance**: Offline diarization inference cost doubles (two runs instead of one); model load time is unchanged (single diarizer instance); accepted for an explicit button action with existing progress reporting
- **Consistency**: The online diarization change (`online-speaker-diarization`) adopts the same naming scheme so labels match across offline and online paths
