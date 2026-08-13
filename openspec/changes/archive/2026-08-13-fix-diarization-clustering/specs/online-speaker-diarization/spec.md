## ADDED Requirements

### Requirement: Efficient mode clusters with the calibrated threshold

The system SHALL cluster the buffered speaker embeddings at recording stop in Efficient mode using the same fixed cosine-similarity threshold (`0.45`) as offline diarization, so distinct speakers are assigned distinct labels and online/offline results stay consistent.

#### Scenario: Multi-speaker recording produces distinct labels

- **WHEN** recording stops in Efficient mode with buffered embeddings containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and matched transcripts SHALL be assigned distinct speaker IDs

#### Scenario: Clustering matches offline label scheme

- **WHEN** an online-Efficient-diarized meeting is re-analyzed with offline diarization
- **THEN** both paths SHALL apply the same fixed threshold, producing the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and comparable speaker counts

### Requirement: Efficient mode prunes singleton clusters

The system SHALL dissolve single-embedding clusters in Efficient mode by reassigning them to the nearest larger cluster, matching offline diarization behavior.

#### Scenario: Singleton fragment reassigned

- **WHEN** Efficient-mode clustering produces a cluster containing fewer than two embeddings
- **THEN** the system SHALL reassign those embeddings to the nearest cluster with at least two embeddings

### Requirement: Gap-fill matches offline behavior

The system SHALL fill unmatched transcripts with the nearest speaker at recording stop, using the same rules as offline diarization.

#### Scenario: Short mic utterance labeled

- **WHEN** a microphone transcript has no overlapping diarization segment and the microphone channel has a single speaker
- **THEN** the transcript SHALL be assigned that speaker's `MIC_SPEAKER_NN` label

#### Scenario: Gap-fill bounded on multi-speaker channel

- **WHEN** a transcript has no overlapping diarization segment and the channel has multiple speakers
- **THEN** the transcript SHALL be assigned the temporally nearest segment's speaker when within 30 seconds, and SHALL remain NULL otherwise
