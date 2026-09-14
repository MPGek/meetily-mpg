## ADDED Requirements

### Requirement: Speaker attribution survives retranscription
Retranscription SHALL NOT leave a meeting with silently discarded speaker attribution or with cluster bindings that no longer correspond to any transcript. After a successful retranscription the meeting SHALL re-establish speaker attribution and provenance for the new transcript rows: user-confirmed identities SHALL be carried forward where their time ranges overlap and/or channel-correct diarization SHALL be re-run, so that the transcript presents the same speaker names, `matched_by` provenance, and match scores as a meeting that was diarized correctly. A user-confirmed (`matched_by='user'`) identity SHALL NOT be lost by retranscription.

#### Scenario: Retranscription of a meeting with confirmed speakers
- **WHEN** retranscription completes for a meeting whose transcripts had user-confirmed speaker identities
- **THEN** the new transcript rows SHALL display those identities for the overlapping time ranges with user provenance, without the user re-assigning them

#### Scenario: Retranscription leaves no stale bindings
- **WHEN** retranscription completes and re-analysis has not yet run
- **THEN** the meeting SHALL NOT present cluster bindings whose labels no longer map to any transcript as if they applied to the new rows, and the user-visible labels, confidence values, and confirmation affordances SHALL be consistent with the attribution actually recorded
