# speaker-identity-registry Specification

## Purpose
A global registry of known speakers with enrolled voiceprint prototypes, per-meeting cluster-to-person mappings, expected-speaker allowlists, and automatic recognition of diarization clusters against stored prototypes, so consistent speaker names and identities appear across meetings and recognition re-runs require no audio re-processing.
## Requirements
### Requirement: Global speaker registry
The system SHALL maintain a global registry of known speakers (`speakers` table: id, name, `is_me` flag, timestamps) independent of any meeting. Speaker names SHALL be unique case-insensitively.

#### Scenario: Person created on first naming
- **WHEN** a user assigns a name that does not match any existing registry speaker to a transcript speaker cluster
- **THEN** the system SHALL create a new registry speaker with that name and link the cluster to it

#### Scenario: Rename is global
- **WHEN** a user renames a registry speaker
- **THEN** the new name SHALL be displayed for that speaker in every meeting where they are linked, without per-meeting edits

### Requirement: Voiceprint storage
The system SHALL store speaker voiceprints in a `speaker_embeddings` table where each row holds a 256-dimensional f32 embedding blob, a `model` tag identifying the extractor model, the capture `channel` ('mic' or 'system'), and the source segment duration. Each row SHALL be owned either by a registry speaker (`speaker_id`, enrolled prototype) or by a meeting cluster (`meeting_id` + `cluster_label`, unassigned cache), enforced by a CHECK constraint. Voiceprint rows SHALL be retained indefinitely.

#### Scenario: Cache written at diarization time
- **WHEN** any diarization path (offline or online) completes clustering for a meeting
- **THEN** the system SHALL persist per-cluster exemplar embeddings as unassigned cache rows and the cluster centroid, so enrollment later requires no audio re-processing

#### Scenario: Model guard
- **WHEN** the system matches voiceprints for recognition
- **THEN** it SHALL only compare embeddings whose `model` tag equals the currently configured extractor model

### Requirement: Per-meeting cluster-to-person mapping
The system SHALL record the link between a meeting's speaker cluster label and a registry speaker in a `meeting_speakers` table (meeting_id, cluster_label, speaker_id, centroid, `matched_by` ('auto' or 'user'), match score). This table SHALL be the source of truth for speaker identity within a meeting; legacy `speaker_names` JSON and `transcripts.speaker_label` SHALL be retained only as display fallback and SHALL NOT be written by new flows.

#### Scenario: User binding wins over auto-match
- **WHEN** a cluster has `matched_by='user'` binding and recognition re-runs (e.g., after the expected-speaker list changes)
- **THEN** the system SHALL NOT overwrite the user's binding

### Requirement: Speaker enrollment on assignment
When a user links a meeting cluster to a registry speaker (existing or newly created), the system SHALL enroll that cluster's cached exemplar embeddings as prototypes of the speaker by reparenting the best rows (longest duration first, capped at K=8 per enrollment). When a user assigns a speaker to a transcript block (live or offline, single-block or cluster-wide), the embeddings whose time windows cover that block SHALL additionally be enrolled into that speaker's global prototype set as ground truth, so the person is recognized across future meetings. Enrollment seeding SHALL be keyed by the underlying pipeline speaker identity and capture channel — embeddings from microphone and system channels SHALL never be mixed into the same seed set. Per-person prototype count SHALL be capped (64), pruning lowest-quality rows.

#### Scenario: Naming enrolls voiceprint
- **WHEN** user assigns the name "Alice" to cluster `SPEAKER_00` of a diarized meeting
- **THEN** up to 8 of that cluster's cached embeddings SHALL become enrolled prototypes of speaker "Alice"

#### Scenario: Linking existing person enrolls too
- **WHEN** user links cluster `SPEAKER_01` to existing registry speaker "Bob"
- **THEN** the cluster's cached embeddings SHALL be added to Bob's prototypes, subject to the per-person cap

#### Scenario: Ground-truth block assignment enrolls its covering embeddings
- **WHEN** a user assigns "Alice" to a single transcript block whose time window is covered by session embeddings
- **THEN** the embeddings overlapping that block's time window SHALL be enrolled as prototypes of Alice at the same time as the block's identity is saved

#### Scenario: Enrollment seeding keeps channels separate
- **WHEN** a user assigns a microphone-channel block to "Alice" and a system-channel block to "Bob" in the same session
- **THEN** Alice's enrollment SHALL contain only microphone-channel embeddings and Bob's only system-channel embeddings, even when both channels share the same numeric pipeline speaker index

### Requirement: Expected-speaker allowlist per meeting
The system SHALL allow users to select, per meeting, a set of expected speakers from the registry (`meeting_expected_speakers` table). Automatic recognition for that meeting SHALL match only against prototypes of expected speakers. If no expected speakers are selected, recognition SHALL match against ALL registry speakers. The allowlist SHALL NOT restrict manual speaker assignment.

#### Scenario: Restricted matching
- **WHEN** a meeting has expected speakers {Alice, Bob} and diarization produces a cluster whose centroid best matches registry speaker Carol
- **THEN** the cluster SHALL remain unidentified even if the Carol score exceeds the threshold

#### Scenario: Empty allowlist matches all
- **WHEN** a meeting has no expected speakers selected
- **THEN** recognition SHALL consider every registry speaker's prototypes

#### Scenario: Unexpected guest assigned manually
- **WHEN** a meeting has expected speakers {Alice, Bob} but Carol also spoke
- **THEN** the user SHALL still be able to assign Carol (or a new name) to her cluster via the speaker editor

### Requirement: Automatic speaker recognition
After clustering completes on any diarization path, the system SHALL match each cluster against candidate prototypes (expected speakers, or all if no allowlist) using cosine similarity over embeddings of the current model tag: score = maximum similarity over the candidate's prototypes; the best-scoring candidate with score above threshold τ=0.7 SHALL be auto-assigned with `matched_by='auto'` and the score recorded. When both channels' prototypes exist, same-channel prototypes SHALL be preferred.

#### Scenario: Known voice auto-labeled
- **WHEN** offline diarization completes on a meeting whose expected speakers include Alice, and a cluster centroid matches an Alice prototype with score 0.78
- **THEN** the cluster SHALL be linked to Alice with `matched_by='auto'`, and her name SHALL display on all blocks of that cluster without user action

#### Scenario: Unknown voice stays anonymous
- **WHEN** no candidate scores above threshold for a cluster
- **THEN** the cluster SHALL remain unidentified and display its formatted cluster label

### Requirement: Instant re-match on allowlist change
Because cluster centroids are cached in `meeting_speakers`, the system SHALL provide a re-match operation that re-runs recognition for a meeting from cached centroids only, without reading or processing audio. User bindings (`matched_by='user'`) SHALL be preserved.

#### Scenario: Allowlist edit resolves pending clusters
- **WHEN** user adds Dave to a meeting's expected speakers after diarization and triggers re-match
- **THEN** previously unidentified clusters SHALL be matched against Dave's prototypes and auto-assigned where confident, with no diarization re-run

### Requirement: Speaker display name resolution
Transcript queries SHALL resolve each transcript's display name by first checking the transcript's per-block speaker override (if any), then joining its meeting's `meeting_speakers` mapping to `speakers`, and finally falling back to legacy `transcripts.speaker_label` and then to the formatted cluster label. The cluster label SHALL remain stored on the transcript row unchanged. Transcript queries SHALL also return the provenance of the resolved name (user-assigned, auto-matched, or fallback) and, for auto-matched names, the match score.

#### Scenario: Joined name displayed
- **WHEN** a transcript's cluster is linked to registry speaker Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Alice" as the display label for that transcript

#### Scenario: Block override takes precedence
- **WHEN** a transcript has a per-block speaker override to registry speaker Bob even though its cluster is linked to Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Bob" as the display label for that transcript

#### Scenario: Legacy fallback
- **WHEN** a transcript has a legacy `speaker_label` but no `meeting_speakers` mapping and no per-block override
- **THEN** transcript queries SHALL return the legacy label

#### Scenario: Auto-matched name carries score
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='auto'` and `match_score=0.78`
- **THEN** transcript queries SHALL return the name together with 'auto' provenance and a 0.78 score

#### Scenario: User-assigned name carries user provenance
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='user'`
- **THEN** transcript queries SHALL return the name together with 'user' provenance and no score

### Requirement: Per-block speaker override
The system SHALL allow relabeling a single transcript block to a registry speaker (existing or newly created) without affecting the rest of its cluster. Block-level assignment SHALL store a per-transcript override that takes precedence over the cluster mapping at display time, SHALL NOT modify `meeting_speakers`, and SHALL NOT enroll any embeddings (a single block owns no cached embeddings). Auto-recognition and re-match SHALL NOT overwrite block overrides.

#### Scenario: Single misattributed block relabeled
- **WHEN** a user assigns "Bob" to a single transcript block whose cluster maps to Alice
- **THEN** only that block SHALL display "Bob"; the cluster mapping and the other blocks' labels SHALL remain unchanged

#### Scenario: Block override survives re-match
- **WHEN** user relabels one block of a cluster and then triggers re-match or diarization runs again for the meeting
- **THEN** the relabeled block SHALL keep "Bob" while other unlinked blocks are re-matched normally

### Requirement: Speaker editing UI with dropdown and free text
The speaker name editor (offline transcript view and live recording view) SHALL offer both a free-text input and a dropdown of registry speakers. The editor SHALL default to single-block scope: selecting an existing speaker or entering a new name links only the block being edited via a per-block override. An explicit "apply to all blocks of this speaker" option SHALL instead link the whole cluster; editing SHALL propagate to all transcript blocks sharing the same cluster label in the meeting only when that option is used.

#### Scenario: Assign from dropdown
- **WHEN** user opens the speaker editor on a transcript block and selects existing speaker "Alice" from the dropdown
- **THEN** the block SHALL be linked to Alice via a per-block override (default scope), and the cluster's cached embeddings SHALL NOT be enrolled

#### Scenario: Apply to all blocks of this speaker
- **WHEN** user selects the "apply to all blocks of this speaker" option and picks "Alice"
- **THEN** the cluster SHALL be linked to Alice with `matched_by='user'`, the cluster's cached embeddings SHALL enroll to Alice, and every transcript block with that cluster label SHALL display "Alice"

#### Scenario: Propagation within meeting (explicit only)
- **WHEN** user assigns a name to one block of cluster `SPEAKER_00` using the default single-block scope
- **THEN** only that block SHALL display the resolved name; other blocks of `SPEAKER_00` SHALL be unchanged

### Requirement: Non-disruptive speaker relabel rendering
After any speaker relabel (single-block override or apply-to-all), the transcript view SHALL update the affected transcript block(s) in place: the view SHALL NOT re-render the full list, SHALL NOT reset the scroll position, and SHALL NOT show an empty or loading state during the update. Unaffected blocks SHALL NOT re-mount, reorder, or change appearance.

#### Scenario: Scroll position preserved on relabel
- **WHEN** user relabels a transcript block while the view is scrolled mid-list
- **THEN** the view SHALL remain scrolled at the same position, with only the relabeled block(s) showing the new name

#### Scenario: No empty/loading flash on assignment
- **WHEN** user confirms a speaker assignment in the editor
- **THEN** the panel SHALL NOT show a loading spinner, blank area, or empty state; the affected block(s) SHALL update to the new label in place

### Requirement: Voiceprint storage visibility
The system SHALL provide voiceprint storage statistics (registry speaker count, enrolled prototype count, unassigned cache count, total embedding bytes) via a command, and SHALL display them in Settings so the user can track growth.

#### Scenario: Stats displayed
- **WHEN** user opens the speaker/diarization section of Settings
- **THEN** the UI SHALL show the current voiceprint storage usage (counts and human-readable size)

### Requirement: Confirm an auto-assigned binding as correct
The system SHALL let the user confirm that an automatically recognized speaker binding is correct without changing the name. Confirming SHALL flip the binding to user provenance: for a cluster-binding share, `meeting_speakers.matched_by` SHALL become `'user'` and `match_score` SHALL be cleared; for a single-block override, the block SHALL be marked overridden so it reads as user provenance. Confirming SHALL NOT re-enroll or duplicate the already-present voiceprint.

#### Scenario: Confirm a recognized cluster binding
- **WHEN** a cluster is auto-bound to "Alice" with a match score and the user confirms it is correct
- **THEN** the cluster's `matched_by` SHALL become `'user'` and its match score SHALL be cleared, so the `(auto)` decoration no longer renders

#### Scenario: Confirm a recognized single block
- **WHEN** a single transcript block is auto-bound to "Alice" with user confirmation
- **THEN** the block SHALL be marked as user-confirmed and SHALL render with plain "Alice", losing the `(auto)` decoration

#### Scenario: Confirmation does not duplicate prototypes
- **WHEN** the user confirms an auto-assigned binding whose speaker already has enrolled prototypes
- **THEN** no new `speaker_embeddings` rows SHALL be created for that confirmation

### Requirement: Enrollment is wired into the single-block correction path
The system SHALL enroll voiceprints on single-block speaker corrections using the embeddings covering that block, not only on cluster-wide assignment. Existing cached cluster exemplars SHALL be enrollable from a block's cluster label so a per-block correction doubles as a teaching signal.

#### Scenario: Cached exemplars enrollable from a block
- **WHEN** an offline meeting has persisted cluster cache rows for a block's cluster and the user corrects that single block
- **THEN** the block's correction SHALL enroll the cluster's exemplar embeddings for the assigned speaker, satisfying the existing "Speaker enrollment on assignment" requirement for the single-block case

### Requirement: Voiceprint browser storage summary is human-readable
The voiceprint browser's storage summary SHALL display total voiceprint storage as a human-readable size (e.g. megabytes) using the same formatting as the Settings general-tab storage section, rather than a raw byte count. The speaker, prototype, and unconfirmed-cache counts SHALL remain numeric.

#### Scenario: Size shown in megabytes
- **WHEN** the voiceprint browser loads with 2,258,944 total embedding bytes
- **THEN** the summary SHALL show a human-readable size (e.g. "2.2 MB") rather than "Bytes: 2258944"

#### Scenario: Empty storage shows zero
- **WHEN** the voiceprint browser loads with zero total embedding bytes
- **THEN** the summary SHALL show "0 MB" rather than "Bytes: 0"

#### Scenario: Consistent with the general-tab storage section
- **WHEN** the voiceprint browser storage summary and the Settings general-tab storage section both render
- **THEN** both SHALL format embedding size the same way, so the two surfaces agree

### Requirement: Voiceprint browser groups start collapsed
The voiceprint browser SHALL load with every speaker group and every unconfirmed-meeting group collapsed by default, showing group headers and the expand controls first rather than every voiceprint row expanded. The existing per-group toggles and expand-all / collapse-all controls SHALL continue to work.

#### Scenario: Default state is collapsed
- **WHEN** the voiceprint browser first loads with voiceprints present
- **THEN** every speaker group and meeting group SHALL start collapsed with their rows hidden

#### Scenario: Expand-all reveals every group
- **WHEN** the user activates expand-all
- **THEN** every speaker and meeting group SHALL become expanded

#### Scenario: Single group expands independently
- **WHEN** the user expands one collapsed group
- **THEN** only that group's rows SHALL be shown while the others remain collapsed
