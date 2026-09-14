# ctc-word-alignment Specification

## Purpose

Provides post-ASR word-level forced alignment: given transcribed text and the corresponding recorded audio, the system refines per-word start/end times so speaker diarization can attribute utterances at true word precision, with graceful fallback when alignment is unavailable.

## Requirements

### Requirement: Alignment model catalog and download
The system SHALL provide a catalog of word-alignment models with metadata (name, language coverage, size) and SHALL support downloading a selected model with progress reporting, following the same model-management UX pattern as the Parakeet engine. The system SHALL report alignment-model readiness (available / missing) independently of transcription models.

#### Scenario: List alignment models in settings
- **WHEN** the user opens the word-alignment section of settings
- **THEN** the system displays the available alignment models with size and language metadata and a download control per model

#### Scenario: Download alignment model
- **WHEN** the user clicks "Download" on an alignment model
- **THEN** the system downloads the model files with progress reporting and marks the model available when complete

#### Scenario: Alignment requested without a downloaded model
- **WHEN** alignment is requested but no alignment model has been downloaded
- **THEN** the system SHALL report the model as unavailable and SHALL NOT fail the enclosing transcription or diarization operation

### Requirement: Word-level alignment of a text segment against audio
The system SHALL align a sequence of word tokens to a supplied audio span, producing for each word a refined start and end time (in seconds, relative to the recording) such that times are monotonically non-decreasing across the word sequence and lie within the supplied span. Alignment SHALL preserve the transcribed word order and text unchanged.

#### Scenario: Align a segment's words to its audio
- **WHEN** a transcript segment with word tokens and its corresponding audio span are submitted for alignment
- **THEN** the system returns the same words with refined start/end timestamps bounded by the segment's audio span

#### Scenario: Alignment of a single-word segment
- **WHEN** a single-word segment is aligned against its audio span
- **THEN** the returned word timestamp SHALL remain within the span and the operation SHALL succeed

### Requirement: Alignment is opt-in with silent fallback
The system SHALL provide a user-facing setting to enable or disable word-level alignment. When alignment is disabled, the model is missing, the audio span for a segment is unavailable, the live alignment queue overflows and the block is dropped, or alignment fails at runtime, the system SHALL fall back to the timestamps already provided by the transcription engine (interpolated for Whisper, frame-aligned for Parakeet) without error.

#### Scenario: Alignment disabled by user
- **WHEN** the alignment setting is off and diarization runs
- **THEN** word tokens SHALL be used exactly as produced by the transcription engine and no alignment inference SHALL run

#### Scenario: Alignment runtime failure on one segment
- **WHEN** alignment fails for a particular segment during a diarization run
- **THEN** that segment SHALL keep its pre-alignment word timestamps and the run SHALL continue for all other segments

#### Scenario: Alignment queue overflow during recording
- **WHEN** the live alignment queue exceeds its bound and the oldest pending block is dropped
- **THEN** that block's tokens SHALL remain as produced by the transcription engine and recording and transcription SHALL continue unaffected

### Requirement: Alignment runs at segment finalization with stop-time and offline repair
The system SHALL align each **final** (non-partial) transcript segment's word tokens against that segment's own in-memory audio during recording, and SHALL persist the refined tokens through the same transcript-update path so buffered segments, incremental transcript files, and saved transcript rows carry word-true timestamps as the meeting progresses. Partial transcription results SHALL NOT be aligned. The stop-time finalize and offline re-diarization flows SHALL refine only segments that do not already carry refined tokens, using the meeting's recorded audio (per-channel where channels are diarized separately); refined timestamps SHALL be the ones the N-way token split operates on.

#### Scenario: Final block aligned live
- **WHEN** a transcription provider returns a final result with word tokens while recording is active and alignment is enabled and available
- **THEN** the block's tokens SHALL be refined against the block's own audio during recording and the buffered segment SHALL be updated in place with the refined timestamps

#### Scenario: Partial results never aligned
- **WHEN** a provider emits a partial transcription update for a segment
- **THEN** no alignment inference SHALL run for that segment before its final result arrives

#### Scenario: Stop-time finalize reuses refined tokens
- **WHEN** recording stops and buffered segments already carry refined tokens
- **THEN** finalize SHALL split on those timestamps without re-running alignment on them

#### Scenario: Stop-time repair of unrefined segments
- **WHEN** recording stops with buffered segments that lack refined tokens (alignment was off, the model was missing, or a block was dropped) and alignment is enabled and available
- **THEN** those segments SHALL be refined against the saved meeting audio before the stop-time speaker assignment splits cross-speaker segments

#### Scenario: Offline re-diarization repairs legacy meetings
- **WHEN** offline diarization runs on a saved meeting whose transcript rows carry unrefined word tokens and alignment is enabled and available
- **THEN** each segment's word tokens SHALL be refined against the meeting audio before tokens are attributed to speakers and cross-speaker segments are split

### Requirement: Windowless audio decoding for alignment repair
When the system extracts an audio span from a meeting's recorded audio for word-alignment refinement, it SHALL launch the external decoder without creating a visible console window on Windows.

#### Scenario: Offline re-diarization repair
- **WHEN** the user runs speaker re-analysis with word alignment enabled and audio spans are extracted from the meeting recording
- **THEN** no console windows appear during the refinement

#### Scenario: Stop-time finalize repair
- **WHEN** recording stops and segments lacking refined tokens are refined from the saved meeting audio
- **THEN** no console windows appear during the refinement

### Requirement: Per-channel refinement uses the decoded channel layout
When word-level refinement reads a meeting's saved audio per channel, the system SHALL determine whether the audio is stereo from the actually decoded audio, not from container/header metadata, so refined token times are mapped to the same microphone or system channel the segment was transcribed from.

#### Scenario: Refining a segment from a stereo recording with unknown metadata
- **WHEN** offline repair refines word tokens for a meeting whose saved audio has two decoded channels but no channel count in its metadata
- **THEN** refinement SHALL read the segment's audio span from its `source_device` channel and SHALL NOT collapse both channels into one

#### Scenario: Mono meeting refinement
- **WHEN** offline repair refines word tokens for a meeting whose decoded audio has a single channel
- **THEN** refinement SHALL read the span from that single stream
