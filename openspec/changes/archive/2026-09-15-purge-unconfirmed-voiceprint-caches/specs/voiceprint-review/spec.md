## ADDED Requirements

### Requirement: Bulk purge control in the voiceprint browser

The voiceprint browser's storage summary SHALL offer a bulk action that removes every unconfirmed cache, presented alongside the existing remove-all action and visually distinguished from it as the narrower operation. The control SHALL be disabled when the unconfirmed cache count is zero. Activating it SHALL require explicit confirmation that states how many cache embeddings will be removed, the stored voice clips affected, and that the removal cannot be undone. After the action completes, the browser's unconfirmed-cache groups, its counts, and the storage summary SHALL refresh to the post-purge state, and any clip playback of a removed row SHALL stop.

#### Scenario: Control enabled with caches present

- **WHEN** the browser loads with a nonzero unconfirmed cache count
- **THEN** the bulk purge control SHALL be enabled next to the existing remove-all action

#### Scenario: Control disabled without caches

- **WHEN** the browser loads with zero unconfirmed caches
- **THEN** the bulk purge control SHALL be disabled

#### Scenario: Confirmation states the impact

- **WHEN** the user activates the bulk purge control
- **THEN** the browser SHALL show a confirmation naming the number of caches to be removed, the affected stored clips, and the irreversibility of the action, before deleting anything

#### Scenario: Cancelling deletes nothing

- **WHEN** the user dismisses the bulk purge confirmation
- **THEN** no cache SHALL be deleted and the browser SHALL remain in its previous state

#### Scenario: Browser reflects the purge

- **WHEN** the user confirms the bulk purge
- **THEN** the unconfirmed-caches branch SHALL be empty, the storage summary SHALL report zero unconfirmed caches and the reduced size, and the confirmed-speaker groups SHALL be unchanged

#### Scenario: Playback of a purged clip stops

- **WHEN** the user confirms the bulk purge while a cache row's clip is playing
- **THEN** playback SHALL stop and the browser SHALL NOT show an error state for the removed row
