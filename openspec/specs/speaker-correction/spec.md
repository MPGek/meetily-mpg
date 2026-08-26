# speaker-correction Specification

## Purpose
Defines the shared behavior of inline speaker corrections in transcript editing — that assigning or confirming a speaker to a transcript block enrolls that person's voiceprint from the block's audio, marks the cluster/block as user-confirmed, and clears the auto-confidence suffix so the user always has visible feedback that their correction was saved.

## Requirements

### Requirement: Inline block correction enrolls the speaker
When a user assigns a registry speaker (existing or newly created) to a transcript block — single-block or apply-to-all, in offline or live mode — the system SHALL enroll the embeddings whose time windows cover that block as ground-truth prototypes of that speaker, in addition to wiring the transcript/cluster mapping. Every enrolled prototype row SHALL carry full provenance (`meeting_id`, `cluster_label`, `audio_start_time`, `audio_end_time`) so the prototype is playable and navigable in the Voiceprint Browser. Enrollment SHALL obey the per-person prototype cap and SHALL keep microphone and system channel embeddings in separate seed sets. A correction that produces no retrievable audio embeddings (e.g. a legacy meeting with no cached clips) SHALL still apply the label mapping without error.

#### Scenario: Offline single-block correction enrolls
- **WHEN** the user assigns "Bob" to one speaker block of an offline diarized meeting and that block's time window overlaps cached embeddings
- **THEN** the block SHALL be overridden to "Bob", and the overlapping offline embeddings SHALL be enrolled as Bob's prototypes with full provenance

#### Scenario: Offline apply-to-all correction enrolls the cluster
- **WHEN** the user assigns "Bob" to a cluster via "apply to all blocks of this speaker"
- **THEN** the cluster SHALL be user-bound to "Bob" and the cluster's cached exemplars SHALL be enrolled as Bob's prototypes

#### Scenario: Live single-turn correction enrolls ground truth
- **WHEN** the user assigns "Bob" to one live turn during Fast-mode recording and the turn's time window overlaps buffered session embeddings
- **THEN** the matching transcript SHALL be overridden to "Bob" and the overlapping embeddings SHALL be enrolled as Bob's prototypes at finalize, with `meeting_id` and `cluster_label` set so the Voiceprint Browser can play and navigate the source

#### Scenario: Correction with no audio still labels
- **WHEN** the user assigns a speaker to a block whose meeting has no cached embeddings or playback audio
- **THEN** the label mapping SHALL be applied and no error SHALL be raised, even though zero embeddings can be enrolled

#### Scenario: Channel separation preserved
- **WHEN** a block corrected on the microphone channel enrolls a speaker
- **THEN** the enrolled embeddings SHALL be microphone-channel embeddings only, and system-channel embeddings SHALL NOT be mixed in