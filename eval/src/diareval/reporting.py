"""Markdown baseline-comparison report."""

from __future__ import annotations

import subprocess
from datetime import date
from pathlib import Path

from .manifests import ManifestError, load_manifest
from .paths import OUT_DIR, REPORTS_DIR
from .runner import run_dir


def _git_rev() -> str:
    try:
        return (
            subprocess.run(
                ["git", "rev-parse", "--short", "HEAD"],
                capture_output=True, text=True, check=True,
                cwd=REPORTS_DIR.parent,
            )
            .stdout.strip()
            or "unknown"
        )
    except Exception:
        return "unknown"


def write_report(datasets: list[str] | None, run_id: str = "latest") -> Path:
    if datasets is None:
        datasets = [
            p.parent.name
            for p in OUT_DIR.glob(f"*/{run_id}/score.json")
        ]
    if not datasets:
        raise SystemExit("no scored datasets found under eval/out")

    rows = []
    for ds in datasets:
        score_path = OUT_DIR / ds / run_id / "score.json"
        if not score_path.is_file():
            raise SystemExit(f"{ds}: no score.json — run `score --dataset {ds}` first")
        import json

        s = json.loads(score_path.read_text(encoding="utf-8"))
        try:
            baseline = load_manifest(ds).baseline_der
        except ManifestError:
            baseline = s.get("baseline_der")
        rows.append((s, baseline))

    rev = _git_rev()
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    out = REPORTS_DIR / f"{date.today().isoformat()}-{rev}.md"
    lines = [
        f"# Diarization evaluation report — {date.today().isoformat()} (git {rev})",
        "",
        "| Dataset | Files | Hours | DER % | FA % | Miss % | Conf % | Baseline % | Δ vs baseline |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for s, baseline in rows:
        delta = (
            f"{s['der'] - baseline:+.2f}"
            if baseline is not None
            else "n/a"
        )
        base_str = f"{baseline:.1f}" if baseline is not None else "n/a"
        lines.append(
            f"| {s['dataset']} | {s['files']} | {s['scored_hours']:.2f} | "
            f"{s['der']:.2f} | {s['fa']:.2f} | {s['miss']:.2f} | {s['conf']:.2f} | "
            f"{base_str} | {delta} |"
        )
    lines += [
        "",
        "Full setup: `DiarizationErrorRate(collar=0.0, skip_overlap=False)` over UEM-",
        "annotated regions; overlap counted. Baselines are published pyannote 3.1",
        "\"Full\" numbers declared per dataset in `eval/manifests/`.",
        "",
    ]
    lines += _online_section(datasets, run_id, {s["dataset"]: s for s, _ in rows})
    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"report written: {out.relative_to(REPORTS_DIR.parent)}")
    return out


def _online_section(datasets: list[str], run_id: str, offline: dict) -> list[str]:
    """The live path's own table, for the datasets that have an online run.

    Datasets measured in one mode only are simply absent here, and the
    heading says so: a missing row means "not measured in this mode", never a
    comparison that failed to compute. The delta column is the one number
    that says whether the live path is behind the calibrated batch path.
    """
    import json

    online_rows = []
    for dataset in datasets:
        score_path = run_dir(dataset, run_id, "online") / "score.json"
        if not score_path.is_file():
            continue
        online_rows.append(json.loads(score_path.read_text(encoding="utf-8")))
    if not online_rows:
        return []

    def fmt(value, digits=3):
        return "n/a" if value is None else f"{value:.{digits}f}"

    lines = [
        "## Online (live path, Fast mode)",
        "",
        "Only the datasets measured in online mode appear below; a dataset absent",
        "here was not run through the live path, which is not a missing comparison.",
        "",
        "| Dataset | Files | Online DER % | Δ vs offline | Lag median s | Lag p90 s "
        "| Uncovered turns | Flip rate | Runs/speaker live | Runs/speaker final "
        "| Switches /spk-min | RTF (not gated) |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in online_rows:
        metrics = row.get("streaming") or {}
        base = offline.get(row["dataset"])
        delta = f"{row['der'] - base['der']:+.2f}" if base else "offline run missing"
        faithful = "" if metrics.get("production_faithful", True) else " (ablation)"
        lines.append(
            f"| {row['dataset']}{faithful} | {row['files']} | {row['der']:.2f} | {delta} | "
            f"{fmt(metrics.get('lag_median'))} | {fmt(metrics.get('lag_p90'))} | "
            f"{metrics.get('uncovered_turns', 'n/a')} | {fmt(metrics.get('flip_rate'))} | "
            f"{fmt(metrics.get('live_runs_per_speaker'))} | "
            f"{fmt(metrics.get('final_runs_per_speaker'))} | "
            f"{fmt(metrics.get('switch_rate_per_speaker_minute'))} | "
            f"{fmt(metrics.get('real_time_factor'))} |"
        )
    lines += [
        "",
        "Lag is audio time from a reference turn's start to the first emission that",
        "covered it; an uncovered turn is counted, never scored as zero. Real-time",
        "factor is recorded for information and is never part of a gate.",
        "",
    ]
    return lines
