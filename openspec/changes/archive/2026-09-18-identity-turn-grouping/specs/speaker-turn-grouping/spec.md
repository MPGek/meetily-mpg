## Purpose

Refines speaker-turn grouping so the merge key is the resolved speaker *identity* (registry person), not the raw diarization cluster label. Merged turns stay collapsed: no expand/collapse control is offered, so the member records behind a turn are not individually reachable and only the turn header edits its first member.

## MODIFIED Requirements

### Requirement: Consecutive same-speaker segments form one displayed turn
The transcript views (live recording view and meeting-details view) SHALL merge runs of chronologically adjacent transcript segments that share the same resolved speaker **identity** on the same `source_device` into a single displayed turn block. The resolved speaker identity SHALL be the registry person the segment's speaker is bound to (a user binding/pin) or recognized as (an auto-match to a registry speaker); when the cluster label carries no registry identity the identity SHALL fall back to the raw cluster label combined with the `source_device`. Clusters that resolve to the same registry person SHALL merge together, so one human whose speech was split by diarization into several clusters (each auto-recognized under the same name) is shown as one person's turns.

#### Scenario: Two consecutive records of one speaker are shown as one block
- **WHEN** two transcript segments with the same resolved speaker, the same `source_device`, and no unresolved gap between them are adjacent in chronological order
- **THEN** the transcript view SHALL render them as one turn block instead of two separate records

#### Scenario: Different raw clusters resolving to the same person merge
- **WHEN** two adjacent segments on the same `source_device` carry different raw cluster labels that both resolve to the same registry person (by name or registry speaker id within the same resolution kind and channel)
- **THEN** the transcript view SHALL render them as one turn block

#### Scenario: Speaker change starts a new turn
- **WHEN** a segment whose resolved-speaker identity differs from the previous segment follows it in chronological order
- **THEN** the transcript view SHALL start a new turn block at the differing segment

#### Scenario: Channel change starts a new turn
- **WHEN** a segment with the same resolved speaker identity but a different `source_device` follows another segment
- **THEN** the transcript view SHALL start a new turn block at that segment (e.g., Microphone and System audio with identical speaker values are never merged)
