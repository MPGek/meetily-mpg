"""Dataset-wide orchestration for the diarize-eval harness binary."""

from __future__ import annotations

import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from .paths import DATA_DIR, EVAL_ROOT, OUT_DIR

HINT = (
    "diarize-eval binary not found. Build it first:\n"
    "  cargo build --release --bin diarize-eval -p meetily"
)


def harness_binary() -> Path:
    name = "diarize-eval.exe" if _is_windows() else "diarize-eval"
    path = EVAL_ROOT.parent / "target" / "release" / name
    if not path.is_file():
        raise SystemExit(HINT)
    return path


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
) -> Path:
    """Diarize every WAV of a materialized dataset; resumable per file.

    `harness_args` are appended verbatim to each `diarize-eval` invocation
    (sweep per-candidate overrides, e.g. `["--cluster-threshold=0.35"]`).
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

    exe = harness_binary()
    out_dir = OUT_DIR / dataset / run_id
    out_dir.mkdir(parents=True, exist_ok=True)
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
    print(f"run {dataset}: {total} recordings, {skipped} already done, {len(todo)} to process")
    if not todo:
        return out_dir

    nw = workers or default_workers()
    failures: list[tuple[str, str]] = []

    def one(wav: Path) -> None:
        rttm = out_dir / f"{wav.stem}.rttm"
        tmp = rttm.with_name(rttm.name + ".part")
        cmd = [str(exe), str(wav), "--out", str(tmp), "--uri", wav.stem]
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
    run_dataset(args.dataset, args.run_id, args.workers, args.force)
    return 0
