## 1. Scaffolding

- [x] 1.1 Create `eval/` uv project (`uv init --lib`-style layout, `pyproject.toml` with `dvc`, `pyannote.metrics`, `pyannote.core`, `datasets`, `soundfile`, `pyyaml`, `tqdm`; CLI package exposing subcommands) and verify `uv run --project eval --help` lists download/ingest/normalize/run/score/report/subset
- [x] 1.2 Create `C:\Users\vasiliy.kotov\Work\Own\meetily-dvc`, run `dvc init` rooted at `eval/`, add default remote `storage` with url `file://C:/Users/vasiliy.kotov/Work/Own/meetily-dvc` and verify `dvc status` runs clean and `dvc push` of a throwaway test file lands blobs in the remote folder
- [x] 1.3 Add `.gitignore` entries for `eval/cache/`, `eval/raw/` (except gated-tracked files via `!` negation), `eval/out/`, `eval/.dvc/tmp/` and verify `git status` stays clean after creating sample dirs
- [x] 1.4 Create `eval/manifests/` schema (pydantic or dataclass loader) and `eval/LICENSES.md` listing each dataset's license and eval-only usage note; verify a unit test parses a sample manifest

## 2. Rust harness

- [x] 2.1 Extract a Tauri/DB-free diarization core from `frontend/src-tauri/src/audio/diarization.rs` (`diarize_wav_samples` + explicit-model-dir loader reusing the 3-location fallback) with `start_diarization` refactored to call it; verify `cargo check -p meetily` passes and app offline diarization of one stored meeting produces the same speaker blocks as before the refactor
- [x] 2.2 Add `[[bin]] diarize-eval` in `frontend/src-tauri` (WAV in via symphonia, RTTM out to stdout/file, non-zero exit + diagnostic on decode or model-load failure); verify `cargo build --release --bin diarize-eval` succeeds and running it on a bundled test WAV emits valid `SPEAKER` lines, and on a missing-models dir exits non-zero naming the files
- [x] 2.3 Parity check (integration): diarize one real recorded meeting through the app and through `diarize-eval` on the same machine/models and diff the turn boundaries; verify differences are within the deterministic-identical expectation, else file as defect before proceeding

## 3. Datasets: open sources

- [x] 3.1 Implement `download` with checksum-verified, idempotent fetching into `eval/cache/` and manifests for VoxConverse test, MSDWild, `niobures/synthetic-speech-diarization-ru`, `leshinsky/ru-youtube-diarization` (HF via `datasets`); verify re-running performs no network fetch and `uv run download --dataset voxconverse` populates cache on a fresh dir
- [x] 3.2 Implement `normalize` ffmpeg path (16 kHz mono PCM + per-manifest channel policy) and reference parsers: passthrough RTTM (VoxConverse, MSDWild), parquet `speakers[]` → RTTM (ru-synthetic), Label Studio JSON `channel/start/end/labels` → RTTM+UEM (ru-youtube); verify each produces the canonical `wav/ rttm/ uem/` layout with counts matching source recordings and RTTM lines parse under `pyannote.core.Annotation.load_rttm`
- [x] 3.3 `dvc add` each normalized dataset, `dvc push`, and record commands in `eval/README.md`; verify on a clean checkout `dvc pull eval/data/<dataset>` reproduces byte-identical files (`dvc status` empty)

## 4. Datasets: gated ingest

- [ ] 4.1 Implement `ingest` for AMI (BUTSpeechFIT pyannote-fork RTTM/UEM lists + manually placed AMI audio archive; channel policies `ami-sdm` = array1:1, `ami-headset` = headset mix) with clear failure when the raw archive is absent; verify with a user-supplied AMI drop that both variants normalize to canonical layout and `dvc add`/`push` completes
- [ ] 4.2 Implement `ingest` for DIHARD-3 (manually placed LDC archive, ships RTTM/UEM) with the same absent-file failure mode; verify after a user drop that canonical layout passes RTTM/UEM parse checks and is pushed

## 5. Scoring & reporting

- [x] 5.1 Implement `run` orchestration: locate `target/release/diarize-eval(.exe)`, thread-pool over dataset WAVs (default workers = 4, configurable via `--workers`), resumable skip of existing outputs, `--force` override; verify an interrupted+re-run produces one hypothesis RTTM per recording with no duplicate work
- [x] 5.2 Implement `score`: pyannote.metrics `DiarizationErrorRate(collar=0.0, skip_overlap=False)` over UEM, duration-weighted dataset aggregation, hard failure listing missing hypotheses; verify on a synthetic fixture (reference duplicated as hypothesis) DER = 0 and on a shuffled-speaker fixture Conf > 0
- [x] 5.3 Implement `report`: Markdown table (dataset, files, hours, DER, FA, Miss, Conf, baseline from manifest, delta) written to `eval/reports/<date>-<gitrev>.md`; verify a scored run emits the file with one row per dataset and the "Full setup" footnote
- [x] 5.4 Define the ≤10-file regression subset in manifests (`subset: true` on ru-synthetic + voxconverse entries) and implement `subset` end-to-end command (prepare once, then run+score+print); verify a full subset run completes in under ~10 minutes on this machine and prints per-source DER

## 6. Wrap-up

- [x] 6.1 Write `eval/README.md` quickstart (one-time setup, per-dataset commands, gated-data drop locations, disk expectations, remote relocation note) and add a pointer from `docs/CODEBASE_MAP_OPERATIONS.md`; verify a fresh clone + README steps reproduces the week-1 ladder (voxconverse + ru-synthetic scored) without asking anyone
- [x] 6.2 Run `openspec validate add-diarization-eval-harness --strict` and address findings; verify validation passes
- [x] 6.3 Record the ru-synthetic annotation-timeline artifact (measured: 73.6% of ref-"speech" frames are silence) in `eval/README.md`, `eval/LICENSES.md` usage notes, and a comment in `eval/manifests/ru-synthetic.yml`; keep the subset gate composition spec-mandated (ru-synthetic + voxconverse, ≤10 files) and document that ru-synthetic is read Conf-only
