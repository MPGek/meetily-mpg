## Context

The live transcription pipeline runs two independent VAD processors (one per source) and labels each transcript segment with `source_device: "Microphone"` or `"System"`. The frontend renders these in a chat-style layout (mic=left/blue, system=right/green).

However, the retranscription pipeline (`audio/retranscription.rs`) reads the stereo `audio.mp4` file and immediately calls `decoded.to_whisper_format()` which mixes both channels to mono via `audio_to_mono()`. After that point, there's no way to distinguish which audio came from which source.

Current retranscription flow:
```
audio.mp4 (stereo: L=mic, R=system)
  → decode_audio_file() → DecodedAudio { samples: interleaved, channels: 2 }
  → to_whisper_format() → audio_to_mono() → single mono stream
  → get_speech_chunks_with_progress() → single VAD pass
  → transcribe segments → source_device: None
```

## Goals / Non-Goals

**Goals:**
- Extract left (mic) and right (system) channels from stereo recordings
- Run independent VAD on each channel
- Transcribe each channel's segments separately
- Label each result with `source_device: "Microphone"` or `"System"`
- Handle mono recordings gracefully (treat as unknown source, `source_device: None`)
- Maintain progress reporting and cancellation support

**Non-Goals:**
- Speaker diarization within a single channel
- Changing the live transcription pipeline (already works correctly)
- Modifying the audio file format or recording process
- Supporting >2 channel audio (e.g., 5.1 surround)

## Decisions

### 1. Channel extraction in `DecodedAudio`

Add a new method `extract_channels()` to `DecodedAudio` that returns separate `Vec<f32>` for left and right channels. For stereo input, de-interleave samples. For mono input, return the same samples for both channels (or return `None` for right to signal mono).

**Alternative considered**: Modify `to_whisper_format()` to return separate channels. Rejected — this would break other callers (import, batch processing) that expect mono output.

### 2. Per-channel VAD and transcription

After extracting channels, run the existing `get_speech_chunks_with_progress()` on each channel independently. Then transcribe each channel's segments using the existing transcription logic.

```
DecodedAudio (stereo)
  → extract_channels() → (left_samples, right_samples)
  → VAD on left → mic_segments
  → VAD on right → system_segments
  → transcribe mic_segments → label source_device="Microphone"
  → transcribe system_segments → label source_device="System"
  → merge results, sort by timestamp
```

**Alternative considered**: Run VAD once on mixed audio, then somehow attribute segments to channels. Rejected — this is ambiguous when both sources speak simultaneously.

### 3. Progress reporting

Current progress: 0-15% decode, 15-20% format conversion, 20-25% VAD, 25-100% transcription.

New progress for stereo:
- 0-15% decode
- 15-20% channel extraction + resampling
- 20-30% VAD (split: 15-25% mic VAD, 25-30% system VAD)
- 30-100% transcription (split proportionally by segment count)

For mono recordings, use the original progress mapping.

### 4. Mono recording handling

If the decoded audio has `channels == 1`, treat it as a mono recording. Run single-channel VAD and transcription, set `source_device: None`. This preserves backward compatibility for old meetings and imported audio.

### 5. Result merging and sorting

After transcribing both channels, merge the results into a single `Vec<TranscriptSegment>` sorted by `audio_start_time`. This matches the existing behavior and ensures the frontend displays segments in chronological order.

## Risks / Trade-offs

- **[Risk] 2x processing time** → Stereo recordings will take roughly twice as long to retranscribe (2x VAD, 2x transcription). Mitigation: This is acceptable for an offline batch operation. Users can cancel if needed.
- **[Risk] Channel crosstalk** → If the stereo recording has bleed between channels (e.g., mic picks up system audio), VAD might detect speech on both channels. Mitigation: This is the same issue as live transcription; the existing VAD thresholds handle it reasonably well.
- **[Risk] Progress reporting complexity** → Splitting progress between two channels adds complexity. Mitigation: Keep it simple — allocate progress ranges proportionally. If either channel has no speech, skip its progress range.
