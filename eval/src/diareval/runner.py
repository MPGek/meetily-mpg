"""Dataset-wide orchestration for the harness binaries (offline and online)."""

from __future__ import annotations

import json
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from .paths import DATA_DIR, EVAL_ROOT, OUT_DIR

#: The pipeline each mode measures, and the binary that drives it. Offline is
#: the calibrated batch path; online replays the live (Fast-mode) path
#: (add-online-diarization-eval D1 - separate binaries, no shared tunables).
MODES = {
    "offline": "diarize-eval",
    "online": "online-eval",
}

DEFAULT_MODE = "offline"


def check_mode(mode: str) -> str:
    if mode not in MODES:
        raise SystemExit(f"unknown mode '{mode}' (expected one of {sorted(MODES)})")
    return mode


def harness_binary(mode: str = DEFAULT_MODE) -> Path:
    stem = MODES[check_mode(mode)]
    name = f"{stem}.exe" if _is_windows() else stem
    path = EVAL_ROOT.parent / "target" / "release" / name
    if not path.is_file():
        raise SystemExit(
            f"{stem} binary not found. Build it first:\n"
            f"  cargo build --release --bin {stem} -p meetily"
        )
    return path


def run_dir(
    dataset: str,
    run_id: str,
    mode: str = DEFAULT_MODE,
    out_dir: Path | None = None,
) -> Path:
    """Where a run's artifacts live, scoped by mode (D7).

    Offline keeps its historical path so the recorded baselines stay
    addressable; online nests under an `online/` level, so the two modes for
    one dataset coexist and can never be scored as one set. `out_dir` lets a
    caller supply its own output root (the one definition of the layout, used
    by both the runner and the scorer).
    """
    check_mode(mode)
    base = (out_dir or OUT_DIR) / dataset
    return base / run_id if mode == DEFAULT_MODE else base / mode / run_id


def write_run_meta(
    directory: Path, dataset: str, run_id: str, mode: str, chunking: str | None
) -> None:
    """Record what produced this run, so scoring can refuse a mode mismatch."""
    meta = {
        "dataset": dataset,
        "run_id": run_id,
        "mode": check_mode(mode),
        "binary": MODES[mode],
    }
    if chunking:
        meta["chunking"] = chunking
    (directory / "run.json").write_text(
        json.dumps(meta, indent=1) + "\n", encoding="utf-8"
    )


def read_run_mode(directory: Path) -> str:
    """The mode a run directory was produced in.

    A directory without `run.json` predates mode scoping, so it is an offline
    run by construction (only the offline harness existed then).
    """
    path = directory / "run.json"
    if not path.is_file():
        return DEFAULT_MODE
    try:
        return check_mode(str(json.loads(path.read_text(encoding="utf-8"))["mode"]))
    except (json.JSONDecodeError, KeyError, TypeError) as exc:
        raise SystemExit(f"unreadable run metadata at {path}: {exc}") from exc


def _is_windows() -> bool:
    import os

    return os.name == "nt"


def default_workers() -> int:
    import os

    cores = os.cpu_count() or 1
    return max(1, min(4, cores))


def _valid_hypothesis(rttm: Path) -> bool:
    """A completed harness output has only well-formed SPEAKER lines (an empty
    file is a valid silence-only hypothesis). A truncated last line from a
    killed process makes the file invalid so it is reprocessed on resume."""
    if not rttm.is_file():
        return False
    text = rttm.read_text(encoding="utf-8")
    for line in text.splitlines():
        if not line.strip():
            continue
        f = line.split()
        if len(f) != 10 or f[0] != "SPEAKER":
            return False
        try:
            int(f[2]); float(f[3]); float(f[4])
        except ValueError:
            return False
    return text == "" or text.endswith("\n")


def run_dataset(
    dataset: str,
    run_id: str = "latest",
    workers: int | None = None,
    force: bool = False,
    files: list[str] | None = None,
    harness_args: list[str] | None = None,
    skip_failures: bool = False,
    mode: str = DEFAULT_MODE,
    chunking: str | None = None,
) -> Path:
    """Diarize every WAV of a materialized dataset; resumable per file.

    `mode` selects the pipeline under measurement and therefore the binary and
    the run directory (D7): `offline` is the calibrated batch path, `online`
    replays the live path and writes an event sidecar next to each RTTM.
    `chunking` is the online policy (`production`, or `fixed:<secs>` as an
    ablation) and is rejected for offline runs, which have no chunking.

    `harness_args` are appended verbatim to each harness invocation (sweep
    per-candidate overrides, e.g. `["--cluster-threshold=0.35"]`).
    With `skip_failures`, files whose harness run fails are recorded in
    `failed.txt` inside the run dir and processing continues (the sweep uses
    this so one bad recording cannot abort a multi-hour grid).
    """
    wav_dir = DATA_DIR / dataset / "wav"
    if not wav_dir.is_dir():
        raise SystemExit(
            f"dataset '{dataset}' not materialized at {wav_dir} — run download+normalize first"
        )
    wavs = sorted(wav_dir.glob("*.wav"))
    if files:
        wanted = set(files)
        wavs = [w for w in wavs if w.stem in wanted]
        missing = wanted - {w.stem for w in wavs}
        if missing:
            raise SystemExit(f"requested files not in dataset: {sorted(missing)}")
    if not wavs:
        raise SystemExit(f"no WAV files for dataset '{dataset}'")

    check_mode(mode)
    if chunking and mode == DEFAULT_MODE:
        raise SystemExit(
            "--chunking applies to online runs only; the offline path has no chunking policy"
        )
    exe = harness_binary(mode)
    out_dir = run_dir(dataset, run_id, mode)
    out_dir.mkdir(parents=True, exist_ok=True)
    write_run_meta(out_dir, dataset, run_id, mode, chunking)
    failed_file = out_dir / "failed.txt"
    already_failed: set[str] = set()
    if force:
        failed_file.unlink(missing_ok=True)
    elif failed_file.is_file():
        already_failed = {
            ln for ln in failed_file.read_text(encoding="utf-8").splitlines() if ln
        }

    todo: list[Path] = []
    skipped = 0
    for wav in wavs:
        if wav.stem in already_failed:
            continue
        rttm = out_dir / f"{wav.stem}.rttm"
        if not force and _valid_hypothesis(rttm):
            skipped += 1
            continue
        if rttm.exists():
            # stale/partial output from an interrupted run: reprocess it
            rttm.unlink(missing_ok=True)
        todo.append(wav)

    total = len(wavs)
    print(
        f"run {dataset} [{mode}]: {total} recordings, {skipped} already done, "
        f"{len(todo)} to process -> {out_dir}"
    )
    if not todo:
        return out_dir

    nw = workers or default_workers()
    failures: list[tuple[str, str]] = []

    def one(wav: Path) -> None:
        rttm = out_dir / f"{wav.stem}.rttm"
        tmp = rttm.with_name(rttm.name + ".part")
        cmd = [str(exe), str(wav), "--out", str(tmp), "--uri", wav.stem]
        if mode != DEFAULT_MODE:
            # The online harness writes its sidecar and timing file itself,
            # next to the RTTM this run resumes on.
            cmd.extend(["--out-dir", str(out_dir)])
            if chunking:
                cmd.extend(["--chunking", chunking])
        if harness_args:
            cmd.extend(harness_args)
        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode == 0 and _valid_hypothesis(tmp):
            tmp.replace(rttm)
        else:
            failures.append((wav.name, (proc.stderr or "").strip()[-400:] or "invalid output"))
            tmp.unlink(missing_ok=True)

    done = 0
    with ThreadPoolExecutor(max_workers=nw) as pool:
        for _ in pool.map(one, todo):
            done += 1
            if done % 10 == 0 or done == len(todo):
                print(f"  {dataset}: {done}/{len(todo)} files")
    if failures:
        print(f"{len(failures)} file(s) failed:")
        for name, err in failures[:10]:
            print(f"  {name}: {err}")
        if skip_failures:
            with failed_file.open("a", encoding="utf-8") as f:
                for name, _ in failures:
                    f.write(f"{Path(name).stem}\n")
            print(f"{dataset}: recorded {len(failures)} failure(s) in {failed_file.name}, continuing")
        else:
            raise SystemExit(1)
    return out_dir


def run(args) -> int:
    run_dataset(
        args.dataset,
        args.run_id,
        args.workers,
        args.force,
        harness_args=getattr(args, "harness_arg", None),
        mode=getattr(args, "mode", DEFAULT_MODE),
        chunking=getattr(args, "chunking", None),
    )
    return 0
