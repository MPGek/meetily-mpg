## Purpose

Defines how diarization benchmark datasets are acquired, normalized into a canonical evaluation layout, and stored under version-controlled data storage so evaluation inputs are reproducible across machines and over time.

## ADDED Requirements

### Requirement: Canonical dataset layout
Every supported benchmark dataset SHALL be materialized in a canonical layout: one 16 kHz mono PCM WAV file per recording, one reference RTTM per dataset, and one UEM file declaring annotated regions. Consumers of evaluation data SHALL only depend on this layout, never on raw source formats.

#### Scenario: Normalized dataset is complete
- **WHEN** a dataset finishes processing
- **THEN** `eval/data/<dataset>/wav/` contains one `.wav` per recording, `rttm/ref.rttm` contains all reference speaker turns, and `uem/ref.uem` covers every referenced recording

#### Scenario: Downmix and resample applied
- **WHEN** a source recording is stereo, multichannel, or not 16 kHz
- **THEN** the canonical WAV is the channel selection or downmix documented for that dataset, resampled to 16 kHz mono PCM

### Requirement: Automated download of open datasets
The system SHALL download and process, via scripted commands, every dataset whose license permits automatic redistribution: VoxConverse test, MSDWild, the Russian synthetic diarization set, and the Russian YouTube diarization set. Downloads SHALL be checksum-verified and idempotent (re-running skips already-fetched artifacts).

#### Scenario: Fresh machine fetches an open dataset
- **WHEN** the download command is run for an open dataset on a machine with no cached data
- **THEN** raw archives are fetched and verified, then normalization produces the canonical layout

#### Scenario: Re-running download
- **WHEN** the download command is run again with all artifacts present and checksums matching
- **THEN** no network fetch occurs and the command succeeds

### Requirement: Manual ingest for gated datasets
For datasets requiring licensed or registration-gated acquisition (AMI, DIHARD-3), the system SHALL accept manually placed raw archives in a documented location and process them into the canonical layout, without attempting automated download.

#### Scenario: User drops a gated archive
- **WHEN** a manually downloaded AMI or DIHARD archive is placed in the documented raw directory and the ingest command is run
- **THEN** the dataset is normalized into the canonical layout and reported as available

#### Scenario: Missing gated archive
- **WHEN** the ingest command is run and the required raw archive is absent
- **THEN** the command fails with a message naming the expected file and where to obtain it

### Requirement: Versioned dataset storage
Normalized datasets and manually ingested gated archives SHALL be tracked in data version control with blobs stored in the configured local remote, so `dvc pull` on any configured machine reproduces identical evaluation inputs. Derived artifacts (harness outputs, reports) and re-fetchable raw downloads SHALL NOT be tracked.

#### Scenario: Dataset shared to another machine
- **WHEN** a dataset has been added to data version control and pushed, and a second machine runs the documented pull command
- **THEN** the canonical layout on that machine is byte-identical to the pushed version

#### Scenario: Derived output stays untracked
- **WHEN** the runner writes hypothesis RTTMs or the scorer writes reports
- **THEN** version control status shows no new tracked files from those outputs
