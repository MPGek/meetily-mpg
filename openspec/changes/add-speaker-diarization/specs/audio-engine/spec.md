# audio-engine Specification — Delta

## ADDED Requirements

### Requirement: Speaker diarization module
The audio engine SHALL include a `diarization` submodule that performs speaker diarization on recorded audio and assigns speaker labels to transcript segments.

#### Scenario: Diarization runs on stored audio
- **WHEN** `start_diarization` is called with a valid `meeting_id`
- **THEN** the diarization module SHALL decode the meeting's audio file, run the sherpa-onnx diarization pipeline, match speaker turns to transcript segments, and persist results to the database

#### Scenario: Diarization respects per-channel audio convention
- **WHEN** diarization processes a stereo audio file (left=mic, right=system)
- **THEN** it SHALL run diarization on the full audio, then override `speaker` to "SystemAudio" for all segments with `source_device="System"`
