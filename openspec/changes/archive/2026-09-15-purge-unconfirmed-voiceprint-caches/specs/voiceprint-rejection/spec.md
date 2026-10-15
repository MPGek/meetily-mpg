## ADDED Requirements

### Requirement: Bulk purge of unconfirmed caches

The system SHALL let the user remove every unconfirmed cache embedding in one action, deleting only embeddings owned by a meeting cluster (no owning registry speaker) and leaving enrolled prototypes and the speaker registry intact. The operation SHALL NOT modify or delete any prototype, any registry speaker, any per-meeting cluster-to-person mapping or its cached centroid, any expected-speaker allowlist entry, or any per-transcript speaker override. The operation SHALL report how many cache rows were deleted and the storage reclaimed, including embedding bytes and stored audio clips. The purge SHALL be irreversible: deleted cache exemplars SHALL be reproducible only by running diarization again for the affected meeting, while existing cluster bindings SHALL continue to support re-match from their cached centroids without the purged caches.

#### Scenario: All caches removed, prototypes kept

- **WHEN** the user confirms a bulk purge and the database holds N unconfirmed caches and P enrolled prototypes
- **THEN** exactly the N caches SHALL be deleted, the P prototypes SHALL remain enrolled, and the result SHALL report N deleted caches

#### Scenario: Prototypes stay recognizable after a purge

- **WHEN** a bulk purge runs while a speaker has enrolled prototypes whose provenance points at meetings that still hold caches
- **THEN** those prototypes SHALL remain enrolled and SHALL still be loaded for recognition

#### Scenario: Bindings and allowlists survive a purge

- **WHEN** meetings have cluster-to-person bindings with cached centroids, expected-speaker allowlists, and per-transcript overrides and a bulk purge runs
- **THEN** every binding, centroid, allowlist entry, and override SHALL be unchanged, and re-match SHALL still run from the cached centroids

#### Scenario: Stored clips are removed with their cache rows

- **WHEN** cache rows carry stored audio clips and a bulk purge runs
- **THEN** those clips SHALL be deleted with their rows and the reclaimed clip bytes and clip count SHALL be reported, while clips stored on prototype rows SHALL remain

#### Scenario: Empty cache layer is a no-op

- **WHEN** a bulk purge runs on a database with zero unconfirmed caches
- **THEN** nothing SHALL be deleted, the result SHALL report zero, and no error SHALL occur

#### Scenario: Live session sees the post-purge state

- **WHEN** a bulk purge completes while a live recording session has in-memory voiceprint state loaded
- **THEN** subsequent enrollment and recognition in that session SHALL use the post-purge database state and SHALL NOT use purged cache rows, without requiring an application restart
