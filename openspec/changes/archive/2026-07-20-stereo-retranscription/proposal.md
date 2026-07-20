## Why

The "Enhance" (retranscription) feature reads the stereo recording file (mic=left, system=right) but immediately mixes both channels to mono before transcription. This loses the source separation that the live transcription pipeline preserves, causing retranscribed meetings to lose the chat-style layout (mic left/blue, system right/green) and fall back to neutral legacy rendering. Users who enhance their recordings lose the visual distinction between their voice and other participants.

## What Changes

- **Retranscription audio decoding**: Extract left (mic) and right (system) channels separately instead of mixing to mono
- **Per-channel VAD**: Run independent VAD on each channel to identify speech segments per source
- **Per-channel transcription**: Transcribe mic and system segments separately
- **Source labeling**: Tag each transcribed segment with `source_device: "Microphone"` or `"System"`
- **Database persistence**: Store `source_device` in the transcripts table (already supported by recent migration)
- **Backward compatibility**: Handle mono recordings gracefully (treat as unknown source)

## Capabilities

### New Capabilities
- `stereo-channel-extraction`: Decode stereo audio into separate left/right channel streams for independent processing
- `per-channel-vad`: Run independent VAD on each audio channel to identify speech segments per source
- `source-labeled-retranscription`: Transcribe each channel separately and label results with source_device metadata

### Modified Capabilities
- `retranscription-pipeline`: Change from mono-mixed transcription to stereo-channel-aware transcription with source labeling

## Impact

- **Code**: `audio/retranscription.rs` (main pipeline), `audio/decoder.rs` (channel extraction), `audio/vad.rs` (per-channel VAD), `audio/common.rs` (source_device labeling)
- **Performance**: 2x VAD calls, 2x transcription calls (one per channel). Total processing time roughly doubles for stereo recordings.
- **Database**: No schema changes needed (source_device column already exists from split-transcript-by-source)
- **Frontend**: No changes needed (chat-style UI already renders based on source_device field)
- **Backward compatibility**: Mono recordings (old meetings, imports) render as neutral legacy style (source_device=None)
