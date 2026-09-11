## Purpose

Lets users confirm that a stored voiceprint truly belongs to its speaker so periodic review only needs to cover new, unverified voiceprints instead of re-checking everything.

## ADDED Requirements

### Requirement: Per-voiceprint verified flag
Each `speaker_embeddings` row SHALL carry a verified state (unverified by default, plus verification timestamp when verified). Verifying a row SHALL set only this flag and SHALL NOT modify the embedding, the audio clip, provenance, speaker binding, or recognition behavior. Reject, reconfirm, replace, and clear-all operations SHALL preserve or reset the flag consistently: a reconfirmed row enrolled to a speaker SHALL become unverified for its new owner until explicitly verified.

#### Scenario: New voiceprints start unverified
- **WHEN** enrollment creates new prototype rows
- **THEN** each new row SHALL be unverified

#### Scenario: Verify sets flag only
- **WHEN** the user verifies a voiceprint row
- **THEN** the row SHALL become verified with a timestamp, and its embedding, clip, and speaker binding SHALL be unchanged

#### Scenario: Reconfirm resets verification
- **WHEN** an unverified or verified cache row is reconfirmed to a speaker
- **THEN** the resulting prototype row SHALL be unverified until explicitly verified

### Requirement: Verify actions and hide-verified filter
The voiceprint browser SHALL offer a Verify action on every voiceprint row, a Verify-all action per speaker group and per unconfirmed meeting group, a hide-verified toggle, and an unverified-count badge per group. Verifying SHALL refresh counts and badges without a page reload.

#### Scenario: Verify single row
- **WHEN** the user activates Verify on an unverified row
- **THEN** that row SHALL become verified and the group's unverified count SHALL decrease by one

#### Scenario: Verify all in group
- **WHEN** the user activates Verify-all on a speaker group with 3 unverified prototypes
- **THEN** all 3 SHALL become verified and the group badge SHALL clear

#### Scenario: Hide verified shows only new work
- **WHEN** the user enables hide-verified with 10 verified and 2 unverified rows
- **THEN** only the 2 unverified rows SHALL be shown, grouped as before
