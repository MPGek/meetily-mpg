# transcript_compare — ASR benchmark on recorded meetings

Hand-corrected reference fragments cut from real meeting recordings, and WER
scoring of every transcript of that meeting (Meetily live/offline, other ASR
engines) against them.

Meeting data — audio, transcripts, references, names and internal terms — is
private and lives only in DVC. Git holds only the code and the `*.dvc` pointers.

## Layout

```
transcript_compare/
  editor.html, prepare.py, score.py   git: shared tools, take a meeting folder
  asr_tools/                          git: engine runners and transcript comparison
  pyproject.toml, uv.lock             git: Python env (uv)
  .dvc/config, .dvcignore             git: this DVC project
  <YYYY-MM-DD_HH-MM>.dvc              git: pointer to one meeting folder
  <YYYY-MM-DD_HH-MM>/                 DVC (git-ignored), one per meeting:
    audio.mp4                           Meetily recording (L = mic, R = system)
    transcripts*.json                   Meetily transcript(s); granola.txt if exported
    fragments.json                      fragments to cut + draft/hint transcript
    glossary.json                       names / terms for prompts and term counts (optional)
    reference/ref_XX.{mp3,txt}          cut audio + hand-corrected text
    asr_runs/asr_<engine>.json          outputs of other engines
```

`transcript_compare/` is its own DVC project, separate from `eval/`:
`cache.type = copy` keeps tracked files plain and writable, so `editor.html`
saves `ref_XX.txt` in place without `dvc unprotect`; `eval/` keeps
`hardlink,copy` for its tens of GB (cache type is per project). Its remote is
the `meetily-asr` folder on the same network share as eval's `meetily`; separate
folders, so `dvc gc --cloud` in one project cannot delete the other's blobs.

## Setup

```powershell
cd transcript_compare
uv sync                 # dvc, jiwer, num2words, rapidfuzz
uv run dvc pull         # needs access to the network share (office network / VPN)
```

Run every command below from `transcript_compare/`. On Windows set
`$env:PYTHONIOENCODING = "utf-8"` first: the scripts print Cyrillic and arrows.

## Adding a meeting

1. Copy `audio.mp4` and `transcripts.json` (and `metadata.json`) from
   `~/Music/meetily-recordings/Meeting <date>_<time>/` into a new folder
   `<date>_<time>/`.
2. Optional, recommended: draft from a stronger engine than the Meetily
   transcript. Extract the channels and run GigaAM:
   ```powershell
   mkdir <m>/asr_runs
   ffmpeg -i <m>/audio.mp4 -af "pan=mono|c0=c0" -ar 16000 <m>/asr_runs/mic.wav
   ffmpeg -i <m>/audio.mp4 -af "pan=mono|c0=c1" -ar 16000 <m>/asr_runs/sys.wav
   uv run --with "onnx-asr[cpu,hub]" --with soundfile python asr_tools/run_asr.py <m> gigaam-v3-e2e-rnnt
   ```
   Delete the WAVs afterwards (they are regenerated in seconds).
3. Write `<m>/fragments.json` (~90 s each; bounds at pauses):
   ```json
   {"draft": "asr_runs/asr_gigaam-v3-e2e-rnnt.json",
    "hints": "transcripts.json",
    "fragments": [{"id": 1, "start": 102.1, "end": 176.5, "channel": "System", "what": "fast turn-taking"}]}
   ```
   `draft` gives the editable lines (and the precise row times in the editor),
   `hints` the `#   whisper:` lines; both accept ASR-run or Meetily JSON.
4. `uv run python prepare.py <m>` cuts `reference/ref_XX.mp3` and writes the
   drafts. Fragments whose `ref_XX.txt` exists are skipped, so adding entries
   to `fragments.json` and re-running never overwrites hand edits.
5. Correct the drafts in `editor.html` (below).
6. `uv run python score.py <m>` prints the WER table
   (`--show <source> <id>` prints the aligned diff for one fragment).
7. Track it: `uv run dvc add <m>`, `uv run dvc push`, commit `<m>.dvc`.

## Editing references

Open `editor.html` in Chrome or Edge, click "Открыть папку…" and pick this
`transcript_compare` folder. Pick the meeting, then a fragment tab; each row
plays from its draft segment's start to the next row's start. Hotkeys are
listed in the page footer. Saving writes `ref_XX.txt` in place and keeps the
pre-edit copy as `ref_XX.txt.bak` (ignored by DVC).

After a round of edits:

```powershell
uv run dvc add <m>       # re-hash the meeting folder
uv run dvc push
git add <m>.dvc; git commit -m "..."
```

## Another machine

```powershell
git pull
cd transcript_compare; uv sync; uv run dvc pull
```

## asr_tools/

All scripts take the meeting folder first and read/write `<m>/asr_runs/`
(`mic.wav`, `sys.wav`, VAD caches, `asr_*.json`). Engine dependencies are not
in `pyproject.toml`; add them per run with `uv run --with ...`.

| script | does |
|---|---|
| `run_asr.py <m> <model>` | faster-whisper (`fw-<model>`) or onnx-asr model over both channels |
| `run_fw_ft.py`, `run_qwen.py`, `run_nemotron.py` | engines over packed VAD windows (`vad_chunks.json`) |
| `run_wcpp.py <m> <tag> <port> <configs…>` | whisper.cpp server per VAD chunk, prompt / anti-hallucination variants |
| `meetily_vad.py <m>`, `seg_exp.py <m> …` | port of Meetily's VAD and segmentation experiments |
| `compare.py <m>`, `compare2.py <m>` | pairwise WER, hallucinations, domain-term hits |
| `windows.py <m> <a-b>…` | what each transcript says in given time windows |

`rover.py <m> <source> <source> <source>…` (next to `score.py`) scores a word-level
majority vote over several sources, the first one breaking ties.

Meetily's own Rust live and offline paths run headless through the `asr-eval`
dev binary (`frontend/src-tauri/src/bin/asr_eval.rs`); build it in the CUDA
environment (`scripts/env-cuda.bat` after `vcvars64.bat`) with
`cargo build --release --features cuda --bin asr-eval`, then
`asr-eval <m>/audio.mp4 --models-dir <dir with ggml-*.bin> --model large-v3 --out-dir <m>/asr_runs`
writes `asr_meetily-rs-<model>-{live,offline}.json`.

Names and internal terms are never in the code: the vocabulary prompts
(`run_wcpp.py … v4domah`, `run_qwen.py … glossary`) and the term counts
(`compare.py`) read `<m>/glossary.json` and stop with an error if it is missing:

```json
{"prompt_terms": ["API", "JSON"], "prompt_names": ["<first names>"],
 "count_terms": {"<label>": "<regex over lower-cased text>"}}
```
