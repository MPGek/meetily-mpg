# Spec Delta

## ADDED Requirements

### Requirement: Live and stop-time speaker attribution agree
Attributing a time window (or a word token) to a speaker SHALL be one shared behavior used by every diarization path: the labels shown during recording, the labels written at recording stop, and the labels written by offline diarization. Given the same set of speaker turns for a channel and the same time window, all paths SHALL select the same speaker, including the gap-fill rules (unconditional fill on a single-speaker channel, nearest turn within 30 seconds on a multi-speaker channel, otherwise unassigned). A path MAY decline to attribute a window it considers not yet decidable, but SHALL NOT apply a different overlap, tie-break, or gap-fill rule than the others.

#### Scenario: Same input yields the same speaker in every path
- **WHEN** the same channel turn list and the same transcript time window are attributed by the live path, by the stop-time path, and by the offline path
- **THEN** all three SHALL return the same speaker, and the same unassigned outcome when no rule applies

#### Scenario: Gap-fill rules are identical
- **WHEN** a transcript window has no overlapping turn and the channel has multiple speakers with the nearest turn more than 30 seconds away
- **THEN** every path SHALL leave the window unassigned rather than one path filling it and another not

#### Scenario: Live may defer without diverging
- **WHEN** the live path holds a block because its speaker is not yet decidable
- **THEN** the block SHALL remain unlabeled rather than being labeled by a rule the stop-time path would not apply

## MODIFIED Requirements

### Requirement: Efficient mode clusters with the calibrated threshold

The system SHALL cluster the buffered speaker embeddings at recording stop in Efficient mode using the clustering parameters resolved from application settings — clusterer kind, merge threshold, and speaker-count ceiling — exactly as the offline path resolves them, so distinct speakers are assigned distinct labels and online/offline results stay consistent. With no stored overrides the resolved merge threshold SHALL be the calibrated enhanced TitaNet-Large family value, so default behavior is unchanged.

#### Scenario: Multi-speaker recording produces distinct labels

- **WHEN** recording stops in Efficient mode with buffered embeddings containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and matched transcripts SHALL be assigned distinct speaker IDs

#### Scenario: Clustering matches offline label scheme

- **WHEN** an online-Efficient-diarized meeting is re-analyzed with offline diarization
- **THEN** both paths SHALL apply the same resolved clustering parameters, producing the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and comparable speaker counts

#### Scenario: Default resolution reproduces the calibrated threshold

- **WHEN** a recording is made in Efficient mode with no clustering overrides stored
- **THEN** the clustering SHALL use the calibrated enhanced-family merge threshold and produce the same result as before this change for the same buffered embeddings
