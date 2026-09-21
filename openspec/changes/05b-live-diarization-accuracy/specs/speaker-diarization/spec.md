# Spec Delta

## ADDED Requirements

### Requirement: Offline diarization reuses a just-recorded session's cached embeddings
When offline diarization runs on a meeting whose recording session already produced speaker embeddings and speaker turns, and those cached artifacts cover the recording's speech adequately, the system SHALL reuse them instead of decoding and re-embedding the audio, and SHALL produce speaker labels of the same kind and quality as a full run. Adequacy SHALL be decided from the cached artifacts' coverage of the recording and from the model family that produced them; a cache produced by a different embedding model family SHALL NOT be reused. When reuse is not possible or fails for any reason, the system SHALL run the full pipeline, and the user-visible result SHALL be indistinguishable from a run that never had a cache. Whether a run reused the cache SHALL be recorded in the run's logs so a result can always be attributed.

#### Scenario: Re-diarization right after recording skips decode
- **WHEN** offline diarization is triggered for a meeting whose session cached embeddings and turns covering its speech
- **THEN** the run SHALL reuse them, SHALL NOT spawn the audio decoder or re-run segmentation, and SHALL complete faster than the equivalent full run on the same machine

#### Scenario: Inadequate coverage falls back to the full pipeline
- **WHEN** the cached artifacts cover only part of the recording's speech, or are absent
- **THEN** the run SHALL decode and process the recording in full, producing the same result it would have produced without any cache

#### Scenario: Cache from another model family is not reused
- **WHEN** the cached embeddings were produced by a different embedding model family than the one currently resolved
- **THEN** the cache SHALL be ignored and the full pipeline SHALL run

#### Scenario: Reuse is attributable
- **WHEN** a diarization run completes
- **THEN** its logs SHALL state whether the cached session artifacts were reused or the full pipeline ran

#### Scenario: Reuse preserves the enrolled prototypes
- **WHEN** a cache-reusing run refreshes a cluster's exemplar cache
- **THEN** it SHALL replace only the unassigned cache rows and SHALL NOT delete enrolled prototypes, exactly as a full run does
