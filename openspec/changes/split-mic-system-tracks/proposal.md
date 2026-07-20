## Why

Currently, microphone and system audio are mixed into a single mono stream before VAD and transcription. This loses the ability to distinguish who is speaking (local participant via mic vs. remote participants via system audio), prevents per-source audio analysis, and makes it impossible to export independent tracks. Splitting tracks enables per-source transcription labeling, independent VAD sensitivity tuning, and stereo recordings where each channel preserves its original source.

## What Changes

- **AudioChunk** gains a `channels` field (1 = mono VAD segment, 2 = stereo recording chunk)
- **AudioCapture** stops converting to mono — instead interleaves each source into a dedicated stereo channel (mic → left, system → right)
- **AudioPipeline** runs two independent `ContinuousVadProcessor` instances (one per source) instead of one on mixed audio
- **AudioMixerRingBuffer** no longer mixes — it interleaves mic and system into stereo for recording
- **RecordingSaver / IncrementalAudioSaver** encode in stereo (2 channels) instead of mono
- **TranscriptUpdate** gains `source_device` field to label each transcript segment as "Microphone" or "System"
- **BREAKING**: Final saved audio changes from mono to stereo; downstream consumers must handle 2-channel audio
- **BREAKING**: `ProfessionalAudioMixer` is removed — mixing is replaced by interleaving

## Capabilities

### New Capabilities
- `independent-vad`: Per-source voice activity detection with separate VAD processor instances for microphone and system audio
- `stereo-recording`: Stereo audio recording with mic on left channel and system audio on right channel
- `source-labeled-transcription`: Transcription segments carry source device metadata ("Microphone" / "System")

### Modified Capabilities
- `audio-engine`: Dual-channel capture now produces independent stereo tracks instead of mixed mono; audio mixing requirement replaced by stereo interleaving; recording output changes from mono to stereo format

## Impact

- **Code**: `audio/recording_state.rs` (AudioChunk), `audio/pipeline.rs` (AudioCapture, AudioPipeline, ring buffer, mixer removal), `audio/vad.rs` (no changes — existing API reused), `audio/incremental_saver.rs` (channels parameter), `audio/recording_saver.rs` (stereo handling), `audio/transcription/worker.rs` (TranscriptUpdate), `audio/encode.rs` (already supports channels parameter — no change needed)
- **File format**: Saved audio changes from mono MP4 to stereo MP4; existing replay/export logic must handle 2-channel audio
- **Memory**: Two VAD processors double VAD state memory (~1KB each); stereo ring buffer doubles sample storage (~115KB per 600ms window at 48kHz)
- **Performance**: VAD processing is independent per stream (parallelizable in future); transcription load unchanged (both VAD outputs feed one serial Whisper worker)
