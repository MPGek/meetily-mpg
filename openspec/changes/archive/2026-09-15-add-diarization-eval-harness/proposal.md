# Proposal: add-diarization-eval-harness

## Why

Speaker diarization (offline `audio/diarization.rs`, online `audio/online_diarization.rs`, TitaNet clustering, voiceprint registry) currently has zero quantitative evaluation: changes are validated by listening to meetings, so regressions in DER go unnoticed and hyperparameters (clustering/recognition thresholds) are tuned by feel. Public benchmarks with published baselines (pyannote 3.1 scores VoxConverse 11.3, AMI-SDM 22.4, DIHARD-3 21.7, MSDWild 25.3 DER under the "Full" no-collar/overlap-counted setup) let us measure our pipeline objectively and catch regressions.

## What Changes

- Add a self-contained Python (uv) project under `eval/` that downloads public diarization benchmark datasets, normalizes them to a canonical form (16 kHz mono WAV + reference RTTM + UEM), runs the Meetily diarization pipeline on them via a new headless Rust harness binary, and scores output with `pyannote.metrics` DER (0 s collar, overlap counted) into a Markdown report compared against published baselines.
- Add `diarize-eval` headless binary target in `frontend/src-tauri` that reuses the existing offline diarization code path (segmentation-3.0 + TitaNet + clustering) on a WAV file and writes an RTTM, without requiring the Tauri app, DB, or recording session.
- Adopt DVC for dataset storage: DVC state (`.dvc/`, `*.dvc`) lives in this repo under `eval/`; the blob remote is a local folder `C:\Users\vasiliy.kotov\Work\Own\meetily-dvc`. Normalized datasets and manually-fetched gated archives (AMI, DIHARD) are DVC-tracked; open-source raw downloads are script-re-fetchable and not tracked.
- Initial dataset scope: VoxConverse test, AMI (SDM + headset mix), MSDWild, DIHARD-3 (gated/manual), `niobures/synthetic-speech-diarization-ru` (Russian, MIT), `leshinsky/ru-youtube-diarization` (Russian, real). AliShell-4/AliMeeting optional follow-up.
- No changes to the shipping app behavior; the harness binary is a dev-only target.

## Capabilities

### New Capabilities

- `diarization-eval-datasets`: download, manual-ingest, normalize, and DVC-track benchmark datasets in a canonical wav/rttm/uem layout.
- `diarization-eval-runner`: headless Rust harness that produces RTTM diarization for an input WAV using the app's production pipeline, plus Python orchestration that runs it over a dataset.
- `diarization-eval-scoring`: DER computation (pyannote.metrics, Full setup) and baseline-comparison reporting, including a fast regression subset.

### Modified Capabilities

<!-- None: existing diarization specs describe app behavior, which is unchanged. -->

## Impact

- New directory `eval/` (uv project: `pyproject.toml`, `src/`, `dvc.yaml` later; `.gitignore` for cache/data).
- New bin target in `frontend/src-tauri/Cargo.toml` (`src/bin/diarize_eval.rs` or equivalent) refactoring shared logic out of `audio/diarization.rs` without changing its runtime behavior.
- New dev dependencies: Python (uv-managed): `dvc`, `pyannote.metrics`, `pyannote.core`, `datasets`, `soundfile`, `pyyaml`; Rust: reuses existing `ort`/model-loading crates already in the tree.
- Local disk: DVC remote folder `meetily-dvc` (tens of GB), `eval/cache` raw downloads.
- Repo hygiene: `*.dvc` files and `.dvc/config` committed; dataset blobs never in git.
