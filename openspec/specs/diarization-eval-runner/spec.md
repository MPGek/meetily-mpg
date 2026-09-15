# diarization-eval-runner Specification

## Purpose

Defines a headless way to execute Meetily's production diarization pipeline on arbitrary WAV files and obtain speaker-turn output in RTTM format, so evaluation can exercise the same code path the app ships without running the full application.

## Requirements

### Requirement: Headless diarization harness binary
The project SHALL provide a developer-only command-line binary that takes an input WAV path and an output RTTM path, loads the same segmentation and embedding models the app uses (with the same model-discovery fallback), runs the offline diarization pipeline, and writes the result as RTTM. It SHALL NOT require the Tauri app, the database, a recording session, or network access at runtime.

#### Scenario: Diarize a file
- **WHEN** the harness is invoked with a valid 16 kHz mono WAV and an output path
- **THEN** it exits successfully and the output file contains one `SPEAKER` line per detected speech segment with start, duration, and a speaker label

#### Scenario: Models unavailable
- **WHEN** the harness is invoked without the diarization models installed in any supported location
- **THEN** it exits with a non-zero status and an error naming the missing model files

#### Scenario: Unsupported input
- **WHEN** the harness is invoked with a file that cannot be decoded as audio
- **THEN** it exits with a non-zero status and a diagnostic identifying the file

### Requirement: Pipeline parity with the app
The harness SHALL reuse the production offline diarization code path (same channel handling, chunking, segmentation, embedding, and clustering) so measured results reflect the shipped pipeline; behavioral divergence between harness and app SHALL be treated as a defect.

#### Scenario: Same input, same turns
- **WHEN** the same meeting audio is diarized by the app's offline flow and by the harness on the same machine with the same models and settings
- **THEN** the resulting speaker turns match within the pipeline's nondeterminism tolerance (identical for deterministic configuration)

### Requirement: Dataset-wide orchestration
The evaluation tooling SHALL provide a command that runs the harness over every recording of a materialized dataset, in parallel up to a configurable worker count, writing per-file hypothesis RTTMs into a per-run output directory and skipping files whose output already exists unless a force flag is given.

#### Scenario: Full dataset run
- **WHEN** the run command is invoked for a dataset name
- **THEN** every WAV in the dataset's canonical layout has a corresponding hypothesis RTTM in the run directory

#### Scenario: Resumed run
- **WHEN** a previously interrupted run is re-invoked without the force flag
- **THEN** files that already have hypothesis RTTMs are not reprocessed and missing ones are
