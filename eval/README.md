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

## Clustering parameter sweep (diarization-param-tuning)

The offline clustering parameters are runtime values shared by app and harness.
Built-in defaults (numeric values sweep-selected 2026-09-04, extended grid +
held-out validation; kind selected 2026-09-07 by the pipeline-v2 6.2 sweep):
clusterer kind **ahc** (fixed threshold 0.60; `nmesc` selectable but
under-clusters dense TitaNet windows — dev Conf 37.27 vs AHC 14.80; `vbx`
gated to an actionable error on the 192-d enhanced family), speaker-count
ceiling **128**, same-speaker gap-merge **0.3 s**. Dense embedding windows
(`embed_window_secs=5.0`, w/2 hop) and calibrated hysteresis binarization
(`onset=0.5, offset=0.4, min_on=0.2, min_off=0.2`) are compiled-in constants,
harness-sweepable only.
Harness overrides need no rebuild:

```powershell
uv run --project eval run --dataset ru-youtube --run-id cand1 `
  --harness-arg=--cluster-threshold=0.35   # also: --max-clusters=N, --gap-merge=SECS, --clusterer=nmesc|ahc|vbx, --embed-window=5.0, --binarization=0.5,0.4,0.2,0.2 (or 'off')
```

Sweep a grid over the tuning pools (voxconverse-dev, ru-youtube, ru-synthetic —
the latter read Conf-only; see Notes) and score the top-2 on held-out
validation (voxconverse test, msdwild):

```powershell
uv run --project eval sweep --grid cluster_threshold=0.55,0.60 `
  --grid cluster_ceiling=64,128 --grid gap_merge_secs=0.0,0.3 --run-prefix sweepext
uv run --project eval sweep --grid clusterer=nmesc,ahc `
  --grid embed_window=3.0,5.0 --grid binarization="0.5,0.4,0.2,0.2" --run-prefix v2kind
```

Per-candidate hypotheses land under `eval/out/<dataset>/sweep-NNN-.../`; the
report is `eval/reports/sweep-<date>-<gitrev>.md` with tuning and validation
columns kept separate. Selection rule: minimize held-out DER among candidates
whose tuning components never regress >2 pp from their best.

The grid history lives in `eval/reports/`: grid-1 (thr 0.35–0.55 × ceil 12/20)
winner failed held-out validation — tight ceilings force below-threshold merges
once active clusters exceed the cap — so the extended grid (thr 0.55/0.60 ×
ceil 64/128 × gap 0.0/0.3) selected the shipped defaults
(thr 0.60 / ceil 128 / gap 0.3).

**Behavior change note:** meetings diarized before this tuning will produce
different speaker labels when re-diarized (higher merge threshold, always-on
ceiling, gap-merge on; pipeline-v2 adds dense embedding windows, hysteresis
binarization, and overlap-aware two-speaker output). Rollback to pre-adoption
AHC behavior without a code revert: store overrides
`diarizationClusterThreshold=0.52`, `diarizationGapMergeSecs=0.0` (the
default kind is already `ahc`; the ceiling can also be raised via
`diarizationClusterCeiling`).

### App settings keys (persisted, optional)

Power users can override the defaults without a rebuild; unset keys fall back
to the built-in defaults. Keys live in the browser settings store (localStorage)
and are mirrored to the backend via `set_diarization_clustering_settings` on
startup:

| Key | Type | Built-in default | Effect |
| --- | --- | --- | --- |
| `diarizationClusterer` | vbx\|nmesc\|ahc | ahc | Clusterer kind: fixed-threshold AHC (default, 6.2 sweep) or automatic-count NME-SC / VBx. `vbx` fails actionably on the bundled 192-d family (vendored PLDA is 256-d-locked); threshold override is inert under automatic count. |
| `diarizationClusterThreshold` | float | 0.60 | AHC merge criterion (AHC only): minimum cosine similarity to merge two clusters. Lower → more merging, fewer speakers. |
| `diarizationClusterCeiling` | int | 128 | Hard cap on distinct speaker labels per channel per pass. User `max_speakers` wins when smaller; the ceiling is always enforced (clamped to 255). |
| `diarizationGapMergeSecs` | float | 0.3 | Merge consecutive same-speaker output segments whose silence gap ≤ this window (0 = off; cross-speaker boundaries and overlaps untouched). |

### Subset regression gate ranges (re-baselined 2026-09-07, pipeline-v2 core + AHC default)

`uv run --project eval subset` enforces the recorded ranges (declared as
`subset_gate:` in the manifests) and exits non-zero with a regressed-source
message when a metric exceeds its bound:

| Dataset | Gated metric | Measured | Gate max |
| --- | --- | ---: | ---: |
| voxconverse (5 files) | DER | 8.40 | 12.0 |
| voxconverse (5 files) | Conf | 3.89 | 7.0 |
| ru-synthetic (5 files) | Conf (DER is artifact-inflated, see Notes) | 12.66 | 16.0 |

(ru-synthetic Conf rose vs the 2026-09-05 4.00 from short-TTS-blip
fragmentation under dense window-mean aggregates — full-set Conf moved only
8.59 → 9.82, within noise on this artifact set. The gate still guards
relative regressions from the new baseline.)

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
- **ru-synthetic annotation-timeline artifact** (measured 2026-09-02): its
  `speakers[]` segments tile the file contiguously while the mixed audio has
  inter-clip silence — 73.6% of reference-"speech" frames measure < -35 dBFS
  (Silero v6 and segmentation-3.0 agree within 2pp on coverage). The resulting
  ~43pp Miss is a dataset artifact, not a model failure: **read ru-synthetic
  Conf-only** as a relative gate; absolute DER numbers come from real-data sets
  (voxconverse, msdwild, ru-youtube). The subset gate composition stays
  spec-mandated (ru-synthetic + voxconverse, ≤10 files).
- Licenses and eval-only usage terms: see `LICENSES.md`.
- The harness binary is dev-only and never shipped in the app bundle.
