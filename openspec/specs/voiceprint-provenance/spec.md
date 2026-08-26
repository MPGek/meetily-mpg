# voiceprint-provenance Specification

## Purpose

Records where every speaker voiceprint embedding came from — which meeting, which channel, and the exact source time range — for both confirmed prototypes and unconfirmed caches, so embeddings can be audited and traced back to their original audio.

## Requirements

### Requirement: Provenance recorded on every embedding
The system SHALL record provenance on each `speaker_embeddings` row: the source `meeting_id`, `cluster_label`, capture `channel` ('mic'/'system'), `audio_start_time` and `audio_end_time` in seconds relative to the meeting audio timeline, plus the existing source segment duration. Unassigned cache rows SHALL always have `meeting_id` and `cluster_label` set. Enrolled prototypes SHALL retain the provenance of the segments they were extracted from. Rows with unknown provenance (for example, enrolled before provenance tracking existed) SHALL be representable with provenance fields unset, and the system SHALL NOT fabricate meeting or timecode values.

#### Scenario: Cache row carries an exact time range
- **WHEN** diarization for meeting M2 writes a cluster `SPEAKER_01` whose source segment is a 3.2 second microphone clip starting at 40.0 seconds
- **THEN** an unassigned cache row exists with `meeting_id`=M2, `cluster_label`=`SPEAKER_01`, `channel`='mic', `audio_start_time`=40.0, `audio_end_time`=43.2, and `duration_secs`=3.2

#### Scenario: Legacy row without provenance is preserved
- **WHEN** the voiceprint browser opens and a row has no `meeting_id` and no timecodes
- **THEN** the row SHALL be shown as having no retrievable source, its provenance SHALL remain unset, and it SHALL remain a valid prototype for recognition

#### Scenario: Timecodes align with the playback timeline
- **WHEN** an embedding's `audio_start_time` is used to seek the meeting audio for playback
- **THEN** it SHALL target the same timeline that transcript blocks of that meeting use, so the played clip matches what the user sees in the transcript

### Requirement: Provenance populated by both diarization paths
Every diarization path that persists cluster caches SHALL populate `audio_start_time`/`audio_end_time` from the segment timestamps it already computes: the offline path from its segmentation segments, the online path from its timestamped chunk buffer.

#### Scenario: Offline segments carry their times
- **WHEN** offline diarization clusters a meeting and writes an exemplar from a segment spanning 12.5–15.0 seconds
- **THEN** the corresponding cache row SHALL record `audio_start_time`=12.5 and `audio_end_time`=15.0

#### Scenario: Online chunks carry their times
- **WHEN** online (live) diarization stops and persists a cached embedding for a chunk spanning 320.0–325.5 seconds
- **THEN** the corresponding cache row SHALL record `audio_start_time`=320.0 and `audio_end_time`=325.5

#### Scenario: Both channels are timed
- **WHEN** a meeting records stereo with both mic and system embeddings
- **THEN** cache rows on both channels SHALL carry their own start/end times on the shared meeting timeline

### Requirement: Recognition ignores provenance
Automatic recognition SHALL load and match prototypes using only the owner `speaker_id` and the `model` tag; provenance columns SHALL NOT affect which embeddings match or their scores. Maintaining, rejecting, or reassigning provenance SHALL never change recognition behavior for a given owner and model, and deleting the source meeting SHALL NOT remove a prototype from recognition.

#### Scenario: Provenanced prototype still matches
- **WHEN** a speaker prototype retains a `meeting_id` whose meeting no longer exists
- **THEN** the prototype SHALL still be loaded for recognition and match like any other prototype of that speaker

#### Scenario: Provenance edits do not change scores
- **WHEN** a review operation updates a row's provenance fields (but not its `speaker_id` or embedding)
- **THEN** recognition scores for that speaker SHALL be unchanged
