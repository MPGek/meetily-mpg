## ADDED Requirements

### Requirement: Enrollment drops incoherent candidates
When cache rows are promoted to prototypes of a speaker (cluster-wide binding, single-block correction, or ground-truth buffer enrollment), the system SHALL compare each selected candidate with the mean of the other selected candidates and SHALL NOT enroll a candidate whose cosine similarity to that mean is below the coherence threshold. A candidate that is not enrolled SHALL remain an unconfirmed cache row. The guard SHALL NOT apply when fewer than three candidates are selected, and SHALL NOT remove prototypes that are already enrolled.

#### Scenario: Outlier among the best-K is left in the cache
- **WHEN** a cluster is bound to Alice and one of the eight selected exemplars has cosine 0.35 to the mean of the other seven
- **THEN** seven exemplars SHALL become Alice's prototypes and the outlier SHALL remain an unconfirmed cache row of its meeting

#### Scenario: Next-best exemplar replaces a dropped one
- **WHEN** an exemplar is dropped by the guard and more cache rows exist for the cluster
- **THEN** the system SHALL fill up to K=8 from the next-longest remaining rows that pass the guard

#### Scenario: Small candidate set is not judged
- **WHEN** a single-block correction selects two candidate rows
- **THEN** both SHALL be enrolled without the coherence check

#### Scenario: Existing prototypes are untouched
- **WHEN** a new enrollment runs for a speaker who already holds prototypes
- **THEN** the guard SHALL evaluate only the new candidates and SHALL NOT demote or delete any existing prototype

### Requirement: Enrollment reports dropped candidates
The enrollment commands SHALL report how many candidates were dropped by the guard so callers and logs can surface it, and SHALL log the count with the meeting and cluster.

#### Scenario: Count returned
- **WHEN** enrollment drops two candidates
- **THEN** the result SHALL state that two were skipped as incoherent in addition to the number enrolled
