# Spec Delta

## ADDED Requirements

### Requirement: End-of-meeting speaker refinement
At recording stop, before final speaker labels are written, the system SHALL re-derive the session's speaker timeline by clustering the session's buffered per-channel speech embeddings with the same clustering behavior and the same resolved parameters the offline path uses, rather than accepting the incremental speaker identities produced while recording. The refinement SHALL run per channel, SHALL keep microphone and system embeddings separate, and SHALL produce the same channel-scoped label scheme as before. When a channel has too few buffered embeddings to cluster, or the refinement fails, the system SHALL fall back to the incremental identities and record that it did so, rather than failing the stop.

#### Scenario: Two voices merged live are separated at stop
- **WHEN** a session's incremental identities assigned one label to speech that the refinement clusters into two
- **THEN** the final labels SHALL reflect the refined clustering, and the affected transcripts SHALL be saved under the refined speakers

#### Scenario: Refinement obeys the resolved parameters
- **WHEN** the refinement runs with a stored clusterer kind, merge threshold, and speaker-count ceiling
- **THEN** it SHALL use those resolved values and SHALL NOT exceed the effective ceiling for a channel

#### Scenario: Channels stay isolated
- **WHEN** the refinement runs on a session that captured both microphone and system audio
- **THEN** each channel SHALL be refined over its own embeddings only, and no cluster SHALL span both channels

#### Scenario: Refinement failure falls back rather than failing the stop
- **WHEN** a channel has fewer buffered embeddings than the clustering requires, or the clustering errors
- **THEN** the stop SHALL complete using the incremental identities for that channel, and the fallback SHALL be logged

### Requirement: Live speaker labels are provisional until promoted
A speaker label displayed during recording SHALL be treated as provisional, and the labels written at recording stop SHALL be the final ones. When the end-of-meeting refinement changes the speaker of already-displayed content, the system SHALL deliver the change through the existing revision mechanism of the live transcript stream rather than through a new event or a silent divergence between the display and the saved transcript. By default the final pass SHALL revise only content whose speaker was still unresolved or provisional and content whose cluster the refinement changed; revising all content unconditionally SHALL be an explicit opt-in setting. A label the user assigned SHALL NOT be demoted or overwritten by promotion.

#### Scenario: Provisional label promoted unchanged
- **WHEN** the refinement agrees with the label already displayed for a block
- **THEN** the block SHALL be marked final with the same speaker, and no revision that changes its displayed name SHALL be emitted

#### Scenario: Changed cluster produces a final revision
- **WHEN** the refinement assigns a different speaker to a block than the one displayed live
- **THEN** a final revision SHALL be emitted for that block, and the saved transcript SHALL carry the refined speaker

#### Scenario: Untouched content is not relabeled by default
- **WHEN** the refinement leaves a block's cluster unchanged and the block was already resolved
- **THEN** no revision SHALL be emitted for it unless the wholesale-relabel setting is enabled

#### Scenario: User assignment survives promotion
- **WHEN** the user assigned a speaker to a block or bound its cluster during the session and the refinement would assign a different speaker
- **THEN** the user's identity SHALL be kept for that content with user provenance, and the refinement SHALL NOT overwrite it
