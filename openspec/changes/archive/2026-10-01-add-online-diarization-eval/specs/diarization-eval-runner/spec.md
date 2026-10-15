## MODIFIED Requirements

### Requirement: Headless diarization harness binary

The project SHALL provide a developer-only command-line binary that takes an input WAV path, a selectable diarization mode (offline or online), and an output RTTM path; loads the same segmentation and embedding models the app uses (with the same model-discovery fallback); runs the selected pipeline; and writes the result as RTTM. In online mode it SHALL drive the application's production online diarization processor (per channel, streaming) and additionally emit a streaming event sidecar for the same recording. The binary SHALL NOT require the Tauri app, the database, a recording session, a voiceprint registry, or network access at runtime.

#### Scenario: Diarize a file
- **WHEN** the harness is invoked with a valid 16 kHz mono WAV and an output path
- **THEN** it exits successfully and the output file contains one `SPEAKER` line per detected speech segment with start, duration, and a speaker label

#### Scenario: Models unavailable
- **WHEN** the harness is invoked without the diarization models installed in any supported location
- **THEN** it exits with a non-zero status and an error naming the missing model files

#### Scenario: Unsupported input
- **WHEN** the harness is invoked with a file that cannot be decoded as audio
- **THEN** it exits with a non-zero status and a diagnostic identifying the file

#### Scenario: Online mode emits two artifacts
- **WHEN** the harness is invoked in online mode for a recording
- **THEN** it writes both the finalized RTTM and a streaming event sidecar for that recording, and the RTTM has the same canonical shape as an offline run so the existing scorer consumes it unchanged

#### Scenario: Online mode without a registry
- **WHEN** the harness runs in online mode and no speaker registry or prototype store is available
- **THEN** it completes successfully, attributing speakers by clustering alone, and never fails or degrades because identity recognition was unavailable

#### Scenario: Online mode reports the mode it ran
- **WHEN** any harness run completes
- **THEN** the run's output identifies the mode (and the chunking policy in online mode) that produced it, so a hypothesis can never be attributed to the wrong pipeline

### Requirement: Pipeline parity with the app

The harness SHALL reuse the production diarization code paths for both modes — the same channel handling, chunking, segmentation, embedding, clustering, and streaming processor the app ships — so measured results reflect the shipped pipeline; behavioral divergence between harness and app SHALL be treated as a defect. Parity obligations apply to offline mode and online mode alike.

#### Scenario: Same input, same turns
- **WHEN** the same meeting audio is diarized by the app's offline flow and by the harness on the same machine with the same models and settings
- **THEN** the resulting speaker turns match within the pipeline's nondeterminism tolerance (identical for deterministic configuration)

#### Scenario: Online replay matches the app's online flow
- **WHEN** the same recorded audio is diarized by the app's online flow and by the harness in online mode on the same machine with the same models and chunking policy
- **THEN** the emitted live turns and the finalized assignments match within the streaming pipeline's ordering tolerance

### Requirement: Dataset-wide orchestration

The evaluation tooling SHALL provide a command that runs the harness over every recording of a materialized dataset in a selected mode, in parallel up to a configurable worker count, writing per-file hypothesis RTTMs into a per-run output directory and skipping files whose output already exists unless a force flag is given. Runs of different modes SHALL be isolated from one another so an online run never resumes from, overwrites, or is scored against offline outputs.

#### Scenario: Full dataset run
- **WHEN** the run command is invoked for a dataset name
- **THEN** every WAV in the dataset's canonical layout has a corresponding hypothesis RTTM in the run directory

#### Scenario: Resumed run
- **WHEN** a previously interrupted run is re-invoked without the force flag
- **THEN** files that already have hypothesis RTTMs are not reprocessed and missing ones are

#### Scenario: Modes stay isolated
- **WHEN** an online run and an offline run have been executed for the same dataset and recording
- **THEN** each mode's hypothesis remains independently addressable and scoring a run reports only that mode's outputs

## ADDED Requirements

### Requirement: Production-faithful online replay

Online mode SHALL feed the streaming processor the same silence-stripped, VAD-merged speech chunks with absolute timestamps that the application's recording pipeline produces, and SHALL map streaming time back to absolute recording time the same way the application does. The chunking policy SHALL be selectable for ablation, with production replay as the default; a run that uses any non-default chunking policy SHALL be marked as such and SHALL NOT be used to support a pipeline-parity claim.

#### Scenario: Default chunking reproduces production segmentation
- **WHEN** online mode is invoked without an explicit chunking policy
- **THEN** the speech chunks fed to the streaming processor are those the production pipeline would produce for that audio, and the run is marked as production-faithful

#### Scenario: Ablation chunking is labelled
- **WHEN** online mode is invoked with a non-default chunking policy
- **THEN** the run completes and is marked as an ablation, so its numbers cannot be mistaken for production behavior

#### Scenario: Absolute timestamps survive silence removal
- **WHEN** the audio contains silence gaps between speech regions
- **THEN** the emitted turns carry absolute recording-relative times that account for the removed silence rather than compressed streaming times

#### Scenario: Deterministic replay
- **WHEN** online mode is run twice on the same recording with the same models and the same chunking policy
- **THEN** the emitted turn sequence and the finalized RTTM are identical
