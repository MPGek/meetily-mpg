# Spec Delta

## ADDED Requirements

### Requirement: A person holds no duplicate voiceprints
The system SHALL NOT store two voiceprints for the same registry speaker that come from the same meeting, the same capture channel and the same audio window and carry the same embedding. Every enrollment path SHALL skip a candidate that duplicates a voiceprint the speaker already holds, and skipped duplicates SHALL NOT count toward the per-person prototype cap or evict any other prototype. Voiceprints that duplicate each other and are already stored SHALL be reduced to a single row per speaker on upgrade. Where copies differ, the one kept SHALL be verified if any copy was verified, and SHALL carry an audio clip if any copy had one.

#### Scenario: Re-enrolling the same chunk is a no-op
- **WHEN** a speaker already holds a voiceprint for meeting M, channel `system`, window [1473.4 s, 1493.2 s], and a later enrollment offers that same embedding and window for the same speaker
- **THEN** no new row SHALL be inserted and the speaker's prototype count SHALL stay unchanged

#### Scenario: The same chunk for a different person is not a duplicate
- **WHEN** a chunk embedding already enrolled for "Alice" is enrolled for "Bob"
- **THEN** Bob SHALL gain the prototype; the rule applies per person

#### Scenario: Existing duplicates are collapsed on upgrade
- **WHEN** the app starts on a database where one speaker holds 13 identical voiceprints for one meeting, channel and window
- **THEN** after the upgrade that speaker SHALL hold exactly one of them, and voiceprints that are not duplicates SHALL be left untouched

#### Scenario: The kept copy preserves verification and clip
- **WHEN** duplicates are collapsed and only one copy is verified while a different copy has an audio clip
- **THEN** the surviving row SHALL be verified and SHALL carry an audio clip
