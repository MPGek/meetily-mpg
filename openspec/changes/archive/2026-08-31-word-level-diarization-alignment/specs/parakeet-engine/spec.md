# parakeet-engine Delta Spec

## MODIFIED Requirements

### Requirement: Audio transcription via Parakeet
The system SHALL transcribe 16kHz audio samples using the loaded Parakeet model and SHALL return, alongside the transcript text, per-word timestamps derived from the model's native token-frame alignment (not linear interpolation): each emitted word SHALL carry a start and end time in seconds, relative to the start of the transcribed chunk, quantized to the model's frame granularity. Word timestamps SHALL be non-decreasing across the sequence, and the last word's end time SHALL NOT exceed the chunk duration by more than one frame.

#### Scenario: Transcribe a recording chunk
- **WHEN** recording produces an audio chunk and Parakeet model is loaded
- **THEN** system returns cleaned transcript text from the ONNX inference pipeline

#### Scenario: Chunk transcription carries word timestamps
- **WHEN** the transcription worker receives a Parakeet result for an audio chunk
- **THEN** the published transcript update SHALL include one token per emitted word with frame-aligned start/end times offset to absolute recording time, in the same form Whisper chunks publish

#### Scenario: Empty transcription
- **WHEN** Parakeet returns no text for a chunk
- **THEN** the transcript update SHALL carry no tokens (an empty or absent token list), consistent with the empty text
