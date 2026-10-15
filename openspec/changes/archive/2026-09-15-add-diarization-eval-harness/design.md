## Context

See proposal.md - Why. Current state: offline diarization lives in `frontend/src-tauri/src/audio/diarization.rs`, entangled with Tauri `Runtime`, SQLx `Pool`, and `AppHandle` (status updates, transcript DB writes). The reusable core is `create_polyvoice_diarizer_for_app` (model 3-location fallback: app_data → resource → manifest) plus per-channel stream processing (`run_channel_diarization_stream`) producing speaker blocks. Embedding/segmentation models (`segmentation-3.0.onnx`, `titanet_large.onnx`) load via `embedder.rs` over `ort`. There is no Python project in the repo; `uv` 0.11.21 is installed; DVC 3.66.1 runs via `uvx`. The DVC blob remote (`C:\Users\vasiliy.kotov\Work\Own\meetily-dvc`) does not exist yet and must be initialized. Both repo and remote are on the C: drive (NTFS reflink works; copy fallback otherwise).

## Goals / Non-Goals

**Goals:**
- Measure offline diarization DER on public benchmarks with published baselines, reproducibly across machines.
- One-command end-to-end flow per dataset: fetch/ingest → normalize → diarize → score → report.
- A fast (< ~10 min on this machine) regression subset usable after any clustering/threshold change.

**Non-Goals:**
- Evaluating the *online* streaming path (`online_diarization.rs`) — needs a chunked real-time harness; deferred (see Open Questions).
- Voiceprint/identity metrics (EER of `assign_live_speaker`) — separate change.
- CI integration (local remote is single-machine; git-committed subset is the future hook).
- Improving diarization quality itself; WER/ASR-coupled metrics; training/fine-tuning on these datasets (licensing forbids it).

## Decisions

### D1. Rust seam: standalone `diarize-eval` bin reusing a refactored pipeline core
Extract a Tauri/DB-free entry point from `diarization.rs`: `diarize_wav_samples(samples, max_speakers, config) -> Vec<SpeakerBlock>` plus a model loader that takes an explicit models directory (reusing the existing 3-location fallback, minus `AppHandle`). The bin (`frontend/src-tauri/src/bin/diarize_eval.rs`, `[[bin]] name = "diarize-eval"`) decodes WAV (symphonia, already a dep), calls the core, prints RTTM. The existing `start_diarization` command is refactored to call the same core, keeping app behavior byte-identical.
*Alternative considered:* driving the real Tauri app via IPC (higher fidelity, includes DB/transcript path) — rejected: slow, flaky to automate, and couples eval to UI state. *Alternative:* reimplementing the pipeline in Python — rejected: guarantees divergence from shipped code.

### D2. Python project: `eval/` uv project, argparse subcommands
`eval/pyproject.toml` with deps `dvc`, `pyannote.metrics`, `pyannote.core`, `datasets`, `soundfile`, `pyyaml`, `tqdm`. CLI as `uv run --project eval <cmd>`: `download`, `ingest`, `normalize`, `run`, `score`, `report`, `subset`. argparse (stdlib) over click — few commands, no extra dep.
*Alternative:* PEP 723 inline-script (`uv run script.py`) — rejected: shared config/manifest loading wants a real project.

### D3. DVC topology: state in repo under `eval/`, local remote in `meetily-dvc`
`dvc init --no-scm` is not needed — the repo is git; run `dvc init` with `eval/` as project root (DVC supports project root below repo root, `-R eval` from repo root or cd into eval). Config: `.dvc/config` sets `remote "storage" url = file://C:/Users/vasiliy.kotov/Work/Own/meetily-dvc` (created by `mkdir` + `dvc remote add -d storage <url>`). Tracked: `eval/data/<dataset>/` (normalized) and `eval/raw/<gated-archive>` for gated sources. Untracked: `eval/cache/` (re-fetchable raw downloads), `eval/out/`, `eval/reports/`.
*Alternative:* DVC project inside `meetily-dvc` — rejected: separates config from consuming code. *Alternative:* track raw open downloads too — rejected: doubles blob size for zero reproducibility gain since scripts + checksums re-fetch them.

### D4. Normalization via ffmpeg subprocess
ffmpeg is the single decoder/normalizer for all sources (already the app's decode path; handles m4a/opus/mp3/flac/multi-channel uniformly): `ffmpeg -i in -ac 1 -ar 16000 -f wav out`. Per-dataset channel policy declared in the dataset manifest (e.g. AMI-SDM = `array1:1`, AMI-headset = headset mix, AISHELL-4 = downmix if added later). RTTM references: VoxConverse/MSDWILD/DIHARD ship RTTM; AMI uses BUTSpeechFIT `pyannote/AMI-diarization-setup` lists+rttm+uem; ru-synthetic parquet `speakers[]` segments → RTTM lines (overlaps included; `simultaneous_segments` ignored as redundant); ru-youtube Label Studio JSON (`channel`, `start`, `end`, labels) → RTTM + per-file UEM.

### D5. Manifest-driven dataset registry
`eval/manifests/<dataset>.yml` per dataset: source URLs + sha256, gated flag + expected raw filename, channel policy, reference format/parser id, baseline DER (pyannote 3.1 Full: voxconverse 11.3, ami-sdm 22.4, ami-headset 18.8, dihard3 21.7, msdwild 25.3), subset membership. Downloaders/parsers are small Python adapters keyed by `parser:` in the manifest; adding a dataset = one manifest + optional adapter function.

### D6. Scoring with pyannote.metrics, strict Full setup
`DiarizationErrorRate(collar=0.0, skip_overlap=False)` over `Timeline` from UEM; per-file annotation↔hypothesis alignment by recording uri; dataset DER = duration-weighted mean (pyannote `metric` accumulates across files). Report written to `eval/reports/<date>-<gitrev>.md` with a fixed table layout (dataset, files, hours, DER, FA, Miss, Conf, baseline, delta).

### D7. Run orchestration calls the cargo-built bin
`uv run run --dataset X` locates `target/release/diarize-eval.exe` (build hint printed if absent: `cargo build --release --bin diarize-eval -p meetily`), runs files with a `multiprocessing`-free thread pool (bin is CPU-bound; default concurrency = 4 processes, configurable upward via `--workers`), resumable per spec (skip existing outputs).

## Risks / Trade-offs

- [Refactor of `diarization.rs` changes app behavior] → Parity check: diarize one stored meeting via app and via harness, diff RTTMs; refactor is pure extraction, no logic edits.
- [Gated data (AMI/DIHARD) can't be scripted] → `ingest` accepts manual drops; VoxConverse + MSDWild + ru-sets already cover the ladder's week-1/3 needs, so gated sets are additive, not blocking.
- [ru-synthetic annotation timeline is broken (measured 2026-09-02): `speakers[]` segments tile the file contiguously while the mixed audio has inter-clip silence — 73.6% of ref-annotated "speech" frames measure < -35 dBFS; Silero v6 and segmentation-3.0 agree to within 2pp on coverage] → its ~43pp Miss is a dataset artifact, not a model failure; treat ru-synthetic as a *Conf-only relative gate*; absolute numbers come from real-data sets (voxconverse, msdwild, ru-youtube).
- [pyannote.metrics collar/overlap convention mismatch inflates/deflates vs baselines] → Baselines recorded in manifests are explicitly the "Full" setup; scoring config hardcodes collar=0, skip_overlap=False; document in report footer.
- [DVC local remote is single-machine; disk grows to tens of GB] → Remote path is config, swappable to a network share/S3 later; `dvc gc` documented.
- [Windows file:// URL quirks in DVC config] → Use forward slashes, drive-letter form `file://C:/...`; verified at init time in tasks.
- [CC-BY-NC / academic-only licenses (MSDWILD, ground-truth set)] → Eval-only usage, no training, no redistribution; licenses listed in `eval/LICENSES.md`.

## Migration Plan

1. Create remote folder, `uv init` project, `dvc init` + remote add + `dvc pull` smoke (no-op).
2. Land Rust refactor + bin behind dev-only target; app regression-checked on one real meeting.
3. Onboard datasets incrementally per the ladder (open first, gated on demand); `dvc add`/`push` each.
4. Rollback: delete `eval/`, revert the two Cargo/lib files touched by the refactor, `git rm -r` the `.dvc` state; remote folder is disposable.

## Open Questions

- Online-streaming evaluation harness (chunked feeding of `online_diarization`, label-stability metrics) — second change once offline DER is trustworthy.
- Closing the measured Conf gap (over-clustering: voxconverse mean 23.0 hypothesis vs 6.5 reference speakers; AHC threshold 0.52 is stricter than polyvoice's 0.45 default with no cluster-count ceiling): tunable threshold + sweep first, then polyvoice `pipeline_v2` (resegmentation, dense embed windows, VBx/NME-SC auto speaker count) — next change.
- Whether to add long-form sets (REPERE, SUMMARY) to stress clustering on >1 h files — decide after first full runs.
- Cross-meeting voiceprint evaluation protocol (enroll on meeting A, test on B using AMI/ICSI database-scope labels) — separate capability.
