## Why

Saved meeting recordings are encoded as AAC-LC at 192 kbps CBR stereo (~86 MB/hour), which is 2-3x more than voice content needs. Speech is transparent for AAC-LC at roughly 48-64 kbps mono (Hydrogenaudio cites ~150 kbps transparency for *music*; Opus reference data shows wideband speech at 12 kbps and fullband music at 32 kbps). The saved file never feeds transcription (the transcription pipeline always decodes and resamples to 16 kHz mono), so the bitrate only affects human listening. 192k wastes disk space without improving anything audible.

## What Changes

- Lower the saved-recording AAC-LC target from 192 kbps CBR to ~96 kbps for stereo, and switch from CBR (`-b:a`) to VBR (`-q:a`) so silence and pauses in meetings compress much harder, cutting storage roughly in half or more.
- Fix an encoding inconsistency: the legacy save path (`write_audio_to_file_with_meeting_name`) encodes **mono** (channels=1) while the incremental checkpoint saver encodes **stereo** (channels=2). Unify both on stereo so the left=mic / right=system separation is preserved in every saved file.
- Keep everything else stable: AAC-LC codec (ffmpeg native), MP4/M4A container, 48 kHz sample rate, `+faststart`, and stream-copy (`-c copy`) checkpoint merging. No file extension or playback-format change.
- Considered and rejected: switching to Opus in a different container (would beat AAC at equal bitrate but changes the file format surface: extension, `audioFormats.ts` whitelist, decoder paths) and HE-AAC (needs non-default fdk encoder). Both out of scope.

## Capabilities

### New Capabilities
- `audio-encoding`: contract for how recorded meeting audio is encoded to disk - codec, bitrate mode and target, channel layout, and container.

### Modified Capabilities
<!-- No existing spec (audio-engine) governs output encoding; behavior is currently unspecified. -->

## Impact

- `frontend/src-tauri/src/audio/encode.rs` - ffmpeg args: `-b:a 192k` CBR → VBR quality target.
- `frontend/src-tauri/src/audio/audio_processing.rs` - `write_audio_to_file_with_meeting_name` passes `channels=1`; change to stereo to match the incremental saver.
- `frontend/src-tauri/src/audio/incremental_saver.rs` - checkpoint encoding inherits new parameters from `encode_single_audio`; no code change expected beyond what `encode.rs` provides.
- Storage: ~86 MB/hour → ~43 MB/hour or less with VBR (silence-heavy meetings compress significantly further).
- No impact on transcription, diarization, audio player, imports, or DB schema (all decode paths are format-independent and resample to 16 kHz).
- No new dependencies.
