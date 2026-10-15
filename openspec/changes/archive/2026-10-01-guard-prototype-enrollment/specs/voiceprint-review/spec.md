## ADDED Requirements

### Requirement: Suspect prototypes are flagged
The voiceprint browser SHALL mark an enrolled prototype as suspect when its cosine similarity to the mean of the same person's other prototypes is below the suspect threshold, or when it is more similar to another person's prototype mean than to its own person's. The flag SHALL be computed on read from stored embeddings and SHALL NOT be persisted. The browser SHALL show a suspect badge on such rows, a suspect count on the speaker group, and a filter that lists only suspect prototypes. A person with fewer than three prototypes SHALL NOT be assessed.

#### Scenario: Prototype nearer to another speaker is flagged
- **WHEN** a prototype of Alice has cosine 0.41 to Alice's other prototypes and 0.84 to Bob's mean
- **THEN** its row SHALL show the suspect badge and Alice's group SHALL count it

#### Scenario: Filter shows only suspects
- **WHEN** the user enables the suspect filter
- **THEN** only suspect prototypes SHALL be listed, with their existing Play, Verify, Reject, and Reconfirm actions available

#### Scenario: Flagging never changes data
- **WHEN** the browser loads and flags suspect prototypes
- **THEN** no prototype SHALL be demoted, deleted, or reassigned until the user acts on it

#### Scenario: Verified prototype stays flagged but marked
- **WHEN** the user verifies a suspect prototype
- **THEN** the row SHALL keep its verified state and the suspect badge SHALL be shown as acknowledged rather than removed
