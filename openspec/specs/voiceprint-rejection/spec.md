# voiceprint-rejection Specification

## Purpose

Lets the user reject or reconfirm individual voiceprints — both confirmed prototypes and unconfirmed caches — and, when a speaker's identity was wrong, replace that speaker across the whole corpus with automatic re-mapping that preserves manual bindings.

## Requirements

### Requirement: Reject a confirmed voiceprint
The system SHALL let the user reject an enrolled prototype. Rejection SHALL remove it from its speaker's recognition set by demoting it to an unassigned cache row (retaining its provenance, channel, and duration, so it remains reviewable under the unconfirmed branch and enrollable elsewhere) or by deleting it outright when the user chooses. If rejection empties the speaker's prototype set, the speaker SHALL have no voiceprint and SHALL NOT be auto-assigned until reconfirmed.

#### Scenario: Reject one prototype keeps the rest and demotes the row
- **WHEN** Alice has 8 prototypes and the user rejects one of them
- **THEN** Alice has 7 prototypes, the rejected row no longer contributes to Alice's recognition, and the row still exists as an unconfirmed cache with its original provenance

#### Scenario: Reject the only prototype leaves the speaker unvoiced
- **WHEN** a speaker has exactly one prototype and the user rejects it
- **THEN** the speaker SHALL have no enrolled prototypes and automatic recognition SHALL NOT auto-assign any cluster to that speaker until a voiceprint is reconfirmed

#### Scenario: Reject with permanent delete
- **WHEN** the user chooses to permanently delete a rejected prototype
- **THEN** the row SHALL be removed entirely and SHALL no longer appear in either the confirmed or unconfirmed views

### Requirement: Reconfirm or reassign a voiceprint
The system SHALL let the user confirm an unconfirmed cache embedding as belonging to a chosen registry speaker, enrolling it as that speaker's prototype subject to the per-person prototype cap, and SHALL let the user reassign a rejected (demoted) voiceprint to a different speaker. Reconfirming to a different speaker SHALL move the voiceprint between speakers: the target gains it as a prototype and the source loses it.

#### Scenario: Confirm a cache as a speaker's prototype
- **WHEN** the user confirms an unconfirmed cache embedding as speaker "Bob"
- **THEN** the row SHALL become an enrolled prototype of Bob (retaining its provenance) and SHALL participate in Bob's future recognition, subject to the per-person cap

#### Scenario: Reassign a demoted voiceprint
- **WHEN** a voiceprint demoted from Alice is reassigned to Carol
- **THEN** Carol gains that prototype and Alice does not retain it, so the move is reflected in both recognition sets

#### Scenario: Cap still enforced on reconfirm
- **WHEN** reconfirming a cache would push a speaker past the per-person prototype cap
- **THEN** the cap SHALL be enforced by pruning lowest-quality rows as with normal enrollment

### Requirement: Replace speaker across the whole corpus
When the user replaces one speaker's identity with another (or with anonymous) as part of rejection, the system SHALL re-map across all meetings: every `meeting_speakers` row with `matched_by='auto'` bound to the source speaker SHALL be re-bound to the target speaker (or unbound to anonymous) and those meetings SHALL be re-matched from cached centroids. Rows with `matched_by='user'` and per-transcript speaker overrides SHALL be preserved exactly. The operation SHALL run atomically, report the number of affected meetings, clusters, and transcripts before committing, and make no partial changes on failure. The target selection UI for this operation is defined by the voiceprint-review capability and SHALL reuse the same person-picker dialog (searchable list, quick filter, create-new, anonymous) as speaker assignment and rename.

#### Scenario: Replace Alice with Bob re-maps auto matches
- **WHEN** the user rejects Alice's voiceprint and replaces her with Bob
- **THEN** all auto-matched clusters across meetings that pointed to Alice SHALL be re-bound to Bob, the source speaker's prototypes SHALL be removed, and the affected meetings SHALL be re-matched from their cached centroids

#### Scenario: Replace with anonymous unbinds auto matches
- **WHEN** the user removes Alice's identity entirely (replacement target is anonymous)
- **THEN** auto-matched clusters previously bound to Alice SHALL be unbound and display their cluster labels until recognized again, with no audio re-processing

#### Scenario: Manual bindings survive replacement
- **WHEN** a user manually bound a cluster to Alice (`matched_by='user'`) and a whole-corpus replacement of Alice runs
- **THEN** that cluster SHALL remain bound to Alice and SHALL NOT be re-bound or unbound

#### Scenario: Block overrides survive replacement
- **WHEN** a transcript has a per-block override naming Alice and a whole-corpus replacement of Alice runs
- **THEN** the override SHALL be preserved unchanged

#### Scenario: Atomic with reported impact
- **WHEN** the user confirms a whole-corpus replacement
- **THEN** the system SHALL apply all re-mappings in one transaction, report the affected counts, and on any failure leave every mapping unchanged

### Requirement: Rejection applies to unconfirmed caches
The system SHALL let the user reject an unconfirmed cache embedding directly: deleting it or otherwise removing it from the unconfirmed branch. A rejected cache SHALL no longer be available for future enrollment and SHALL no longer appear in the voiceprint browser.

#### Scenario: Delete an unconfirmed cache
- **WHEN** the user rejects an unconfirmed cache embedding
- **THEN** the row SHALL disappear from the unconfirmed branch and SHALL NOT be eligible for later confirmation

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
