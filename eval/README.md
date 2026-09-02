# Diarization evaluation harness

Quantitative DER evaluation for Meetily's offline speaker diarization against
public benchmarks. Everything lives under `eval/`. Datasets are materialized in
a canonical layout (`wav/` 16 kHz mono PCM + `rttm/ref.rttm` + `uem/ref.uem`),
diarized by the headless `diarize-eval` binary built from the app's own pipeline,
and scored with `pyannote.metrics` DER (Full setup: no collar, overlap counted).

## One-time setup

```powershell
# 1. Python env (uv-managed; deps: dvc, pyannote-metrics, datasets, soundfile, ...)
uv sync --project eval

# 2. DVC remote (already configured in eval/.dvc/config; recreate if moving machine)
uv run --project eval dvc remote add -d storage "C:/Users/vasiliy.kotov/Work/Own/meetily-dvc"

# 3. Harness binary (reuses the app's diarization code path; models must be
#    installed for the app, or pass --models-dir)
cargo build --release --bin diarize-eval -p meetily
```

## Per-dataset commands

Open datasets (download → normalize → diarize → score → report):

```powershell
uv run --project eval download  --dataset voxconverse
uv run --project eval normalize --dataset voxconverse
uv run --project eval run       --dataset voxconverse
uv run --project eval score     --dataset voxconverse
uv run --project eval report

uv run --project eval download  --dataset msdwild      # audio via Google Drive
uv run --project eval normalize --dataset msdwild
# ... same run/score/report
uv run --project eval download  --dataset ru-synthetic # HF, MIT
uv run --project eval normalize --dataset ru-synthetic
uv run --project eval download  --dataset ru-youtube   # HF, Apache-2.0
uv run --project eval normalize --dataset ru-youtube
```

Fast regression gate (≤10 recordings, ru-synthetic + voxconverse, end-to-end):

```powershell
uv run --project eval subset
```

## Gated datasets (manual data drop)

AMI and DIHARD-3 audio cannot be scripted. Place the archives, then ingest:

| Dataset | Drop location | Expected file | Obtained from |
| --- | --- | --- | --- |
| `ami-sdm` | `eval/raw/ami-sdm/` | `ami-sdm-audio.zip` | https://groups.inf.ed.ac.uk/ami/corpus/ (EULA) |
| `ami-headset` | `eval/raw/ami-headset/` | `ami-headset-audio.zip` | same |
| `dihard3` | `eval/raw/dihard3/` | `LDCCeRC2022S03.tar.gz` | https://www.ldc.upenn.edu/ (LDC account) |

```powershell
uv run --project eval ingest --dataset ami-sdm
uv run --project eval run --dataset ami-sdm; uv run --project eval score --dataset ami-sdm
```

## DVC: sharing normalized datasets

Normalized datasets (`eval/data/`) and gated raw archives are DVC-tracked;
re-fetchable open downloads (`eval/cache/`), harness outputs (`eval/out/`) and
reports (`eval/reports/`) are not.

```powershell
uv run --project eval dvc add data/<dataset>   # after normalize/ingest
uv run --project eval dvc push                 # blobs -> local remote folder
uv run --project eval dvc pull data/<dataset>  # on another machine
```

Commit the generated `data/<dataset>.dvc` files. The blob remote is a plain
folder; to relocate it, `dvc remote modify storage url <new-path>` and copy the
old folder's `files/` tree across.

## Disk expectations

- `eval/cache/`: ~4 GB (VoxConverse) + ~7.5 GB (MSDWild) + ~2.3 GB (ru-synthetic) + ~2 GB (ru-youtube)
- `eval/data/`: canonical WAVs (~1.6× the audio hours at 16 kHz mono s16) — tens of GB total
- DVC remote (`meetily-dvc/`): roughly mirrors `eval/data/` + gated raw archives

## Notes

- Baseline DERs (pyannote 3.1 "Full") are declared per dataset in
  `eval/manifests/*.yml`; the report compares against them.
- ru-synthetic uses TTS voices: treat it as a *relative* regression gate only.
- Licenses and eval-only usage terms: see `LICENSES.md`.
- The harness binary is dev-only and never shipped in the app bundle.
